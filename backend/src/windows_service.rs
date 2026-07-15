use std::{
    ffi::c_void,
    io, ptr,
    sync::{
        Mutex,
        atomic::{AtomicPtr, AtomicU32, Ordering},
    },
};

use tokio::sync::oneshot;
use windows_sys::Win32::{
    Foundation::{ERROR_CALL_NOT_IMPLEMENTED, ERROR_SERVICE_SPECIFIC_ERROR, NO_ERROR},
    System::Services::{
        RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
        SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP,
        SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_HANDLE,
        SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
        SetServiceStatus, StartServiceCtrlDispatcherW,
    },
};

const SERVICE_NAME: &str = "DmxServerManager";
const START_WAIT_HINT_MS: u32 = 120_000;
const STOP_WAIT_HINT_MS: u32 = 120_000;

pub fn run() -> anyhow::Result<()> {
    let mut service_name = wide_null(SERVICE_NAME);
    let service_table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: service_name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];

    // SAFETY: the table and its UTF-16 service name remain alive while the blocking
    // dispatcher call is active, and the table is terminated by an empty entry.
    let started = unsafe { StartServiceCtrlDispatcherW(service_table.as_ptr()) };
    if started == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

unsafe extern "system" fn service_main(_argument_count: u32, _arguments: *mut *mut u16) {
    if let Err(error) = service_main_inner() {
        // A subscriber is not necessarily available when configuration/bootstrap fails.
        // The authoritative failure is also returned to SCM through SERVICE_STOPPED.
        eprintln!("DmxServerManager Windows service failed: {error:#}");
    }
}

fn service_main_inner() -> anyhow::Result<()> {
    let (stop_sender, stop_receiver) = oneshot::channel();
    let context = Box::new(ControlContext::new(stop_sender));
    let context_pointer = (&*context as *const ControlContext).cast::<c_void>();
    let service_name = wide_null(SERVICE_NAME);

    // SAFETY: the callback context is boxed and kept alive until SERVICE_STOPPED has
    // been reported and service_main_inner returns.
    let status_handle = unsafe {
        RegisterServiceCtrlHandlerExW(
            service_name.as_ptr(),
            Some(service_control_handler),
            context_pointer,
        )
    };
    if status_handle.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    context
        .status_handle
        .store(status_handle, Ordering::Release);
    context.report(SERVICE_START_PENDING, NO_ERROR, 0, 1, START_WAIT_HINT_MS)?;

    let result = super::runtime().and_then(|runtime| {
        runtime.block_on(async {
            let panel = super::Panel::bootstrap().await?;
            context.report_ready()?;
            panel
                .serve(async move {
                    let _ = stop_receiver.await;
                })
                .await
        })
    });

    let status_result = context.report_stopped(result.is_err());
    status_result?;
    result
}

struct ControlContext {
    status_handle: AtomicPtr<c_void>,
    current_state: AtomicU32,
    win32_exit_code: AtomicU32,
    service_exit_code: AtomicU32,
    checkpoint: AtomicU32,
    wait_hint: AtomicU32,
    lifecycle: Mutex<ServiceLifecycle>,
    stop_sender: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Default)]
struct ServiceLifecycle {
    stopping: bool,
    finished: bool,
}

impl ControlContext {
    fn new(stop_sender: oneshot::Sender<()>) -> Self {
        Self {
            status_handle: AtomicPtr::new(ptr::null_mut()),
            current_state: AtomicU32::new(SERVICE_START_PENDING),
            win32_exit_code: AtomicU32::new(NO_ERROR),
            service_exit_code: AtomicU32::new(0),
            checkpoint: AtomicU32::new(0),
            wait_hint: AtomicU32::new(START_WAIT_HINT_MS),
            lifecycle: Mutex::new(ServiceLifecycle::default()),
            stop_sender: Mutex::new(Some(stop_sender)),
        }
    }

    fn request_stop(&self) {
        {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if lifecycle.stopping || lifecycle.finished {
                return;
            }
            lifecycle.stopping = true;
            let _ = self.report(SERVICE_STOP_PENDING, NO_ERROR, 0, 1, STOP_WAIT_HINT_MS);
        }
        let mut sender = self
            .stop_sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(sender) = sender.take() {
            let _ = sender.send(());
        }
    }

    fn report_ready(&self) -> io::Result<()> {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if lifecycle.stopping {
            self.report(SERVICE_STOP_PENDING, NO_ERROR, 0, 1, STOP_WAIT_HINT_MS)
        } else {
            self.report(SERVICE_RUNNING, NO_ERROR, 0, 0, 0)
        }
    }

    fn report_stopped(&self, failed: bool) -> io::Result<()> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lifecycle.finished = true;
        if failed {
            self.report(SERVICE_STOPPED, ERROR_SERVICE_SPECIFIC_ERROR, 1, 0, 0)
        } else {
            self.report(SERVICE_STOPPED, NO_ERROR, 0, 0, 0)
        }
    }

    fn report(
        &self,
        state: u32,
        win32_exit_code: u32,
        service_exit_code: u32,
        checkpoint: u32,
        wait_hint: u32,
    ) -> io::Result<()> {
        self.current_state.store(state, Ordering::Release);
        self.win32_exit_code
            .store(win32_exit_code, Ordering::Release);
        self.service_exit_code
            .store(service_exit_code, Ordering::Release);
        self.checkpoint.store(checkpoint, Ordering::Release);
        self.wait_hint.store(wait_hint, Ordering::Release);
        self.report_current()
    }

    fn report_current(&self) -> io::Result<()> {
        let handle: SERVICE_STATUS_HANDLE = self.status_handle.load(Ordering::Acquire);
        if handle.is_null() {
            return Err(io::Error::other(
                "Windows service status handle is unavailable",
            ));
        }
        let state = self.current_state.load(Ordering::Acquire);
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
            } else {
                0
            },
            dwWin32ExitCode: self.win32_exit_code.load(Ordering::Acquire),
            dwServiceSpecificExitCode: self.service_exit_code.load(Ordering::Acquire),
            dwCheckPoint: self.checkpoint.load(Ordering::Acquire),
            dwWaitHint: self.wait_hint.load(Ordering::Acquire),
        };

        // SAFETY: handle was returned by RegisterServiceCtrlHandlerExW and status
        // points to a fully initialized SERVICE_STATUS for the duration of the call.
        if unsafe { SetServiceStatus(handle, &status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

unsafe extern "system" fn service_control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    context: *mut c_void,
) -> u32 {
    if context.is_null() {
        return ERROR_CALL_NOT_IMPLEMENTED;
    }
    // SAFETY: RegisterServiceCtrlHandlerExW received a pointer to a boxed
    // ControlContext whose lifetime covers all service control callbacks.
    let context = unsafe { &*context.cast::<ControlContext>() };
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            context.request_stop();
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => {
            let _ = context.report_current();
            NO_ERROR
        }
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
