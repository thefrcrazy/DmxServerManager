#![recursion_limit = "512"]

use std::{
    ffi::OsString,
    future::{Future, IntoFuture},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::Response,
    routing::get_service,
};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod core;
mod domain;
mod services;
#[cfg(windows)]
mod windows_service;

use core::{AppState, Settings, database, events::EventHub};
use services::{
    backups, catalog, instance_storage,
    profiles::ProfileRegistry,
    releases::{ReleaseMonitor, ReleaseMonitorWorker},
    runtime::RuntimeManager,
    schedules::SchedulerService,
    secrets::SecretStore,
    webhooks::WebhookDispatcher,
};

const HTTP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const TOKIO_WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupMode {
    Console,
    WindowsService,
    PrintOpenApi,
}

fn main() -> anyhow::Result<()> {
    match startup_mode(std::env::args_os().skip(1))? {
        StartupMode::Console => run_console(),
        StartupMode::WindowsService => run_windows_service(),
        StartupMode::PrintOpenApi => print_openapi(),
    }
}

fn startup_mode(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<StartupMode> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(StartupMode::Console),
        [argument] if argument == "--service" => Ok(StartupMode::WindowsService),
        [argument] if argument == "--print-openapi" => Ok(StartupMode::PrintOpenApi),
        _ => anyhow::bail!("usage: dmx-server-manager [--service|--print-openapi]"),
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(TOKIO_WORKER_STACK_BYTES)
        .enable_all()
        .build()?)
}

fn run_console() -> anyhow::Result<()> {
    runtime()?.block_on(async {
        let panel = Panel::bootstrap().await?;
        panel.serve(console_shutdown_signal()).await
    })
}

fn print_openapi() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&api::openapi::document())?
    );
    Ok(())
}

#[cfg(windows)]
fn run_windows_service() -> anyhow::Result<()> {
    windows_service::run()
}

#[cfg(not(windows))]
fn run_windows_service() -> anyhow::Result<()> {
    anyhow::bail!("--service is only supported on Windows")
}

struct Panel {
    app: Router,
    listener: tokio::net::TcpListener,
    runtime: RuntimeManager,
    scheduler: SchedulerService,
    webhooks: WebhookDispatcher,
    releases: ReleaseMonitorWorker,
}

impl Panel {
    async fn bootstrap() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let settings = Settings::from_env()?;
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(&settings.log))
            .with(tracing_subscriber::fmt::layer())
            .try_init()?;

        std::fs::create_dir_all(&settings.data_dir)?;
        std::fs::create_dir_all(settings.instances_dir())?;

        let pool = database::init_pool(&settings.database_url).await?;
        database::run_migrations(&pool).await?;
        instance_storage::cleanup_interrupted_imports(&settings).await?;
        backups::recover_interrupted_restores(&pool, &settings).await?;
        catalog::cleanup_interrupted(&pool, &settings).await?;
        let profiles = Arc::new(ProfileRegistry::builtins());
        profiles.persist_builtins(&pool).await?;
        profiles.load_persisted(&pool).await?;
        let secrets = SecretStore::load_or_create(&settings.master_key_file)?;
        let settings = Arc::new(settings);
        let events = EventHub::new(2_048);
        let runtime = RuntimeManager::new(
            pool.clone(),
            settings.clone(),
            events.clone(),
            secrets.clone(),
        );
        let releases = ReleaseMonitor::new(settings.clone())?;
        let state = AppState {
            pool,
            settings: settings.clone(),
            profiles,
            events,
            secrets,
            runtime: runtime.clone(),
            releases: releases.clone(),
        };
        runtime.reconcile_boot().await?;
        let app = build_app(state.clone())?;

        let listener = tokio::net::TcpListener::bind(settings.bind).await?;
        let webhooks = WebhookDispatcher::start(state.clone())?;
        let scheduler = SchedulerService::start(state);
        let releases = ReleaseMonitorWorker::start(releases);

        info!(
            version = env!("CARGO_PKG_VERSION"),
            bind = %settings.bind,
            data_dir = %settings.data_dir.display(),
            config_file = %settings.config_file.display(),
            import_roots = settings.import_roots.len(),
            "starting DmxServerManager"
        );
        Ok(Self {
            app,
            listener,
            runtime,
            scheduler,
            webhooks,
            releases,
        })
    }

    async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let Self {
            app,
            listener,
            runtime,
            mut scheduler,
            mut webhooks,
            mut releases,
        } = self;
        let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel();
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = http_shutdown_rx.await;
        })
        .into_future();
        tokio::pin!(server);

        tokio::select! {
            result = &mut server => {
                scheduler.shutdown().await;
                releases.shutdown().await;
                runtime.shutdown().await;
                webhooks.shutdown().await;
                result?;
            }
            () = shutdown => {
                info!("panel shutdown requested");
                let _ = http_shutdown_tx.send(());
                scheduler.shutdown().await;
                releases.shutdown().await;
                let (http_result, ()) = tokio::join!(
                    tokio::time::timeout(HTTP_SHUTDOWN_TIMEOUT, &mut server),
                    runtime.shutdown(),
                );
                webhooks.shutdown().await;
                match http_result {
                    Ok(result) => result?,
                    Err(_) => warn!(
                        timeout_seconds = HTTP_SHUTDOWN_TIMEOUT.as_secs(),
                        "HTTP graceful shutdown timed out; remaining connections were closed"
                    ),
                }
            }
        }
        Ok(())
    }
}

async fn console_shutdown_signal() {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        match terminate {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result {
                            warn!(%error, "failed to install Ctrl+C handler");
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                warn!(%error, "failed to install SIGTERM handler");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }

    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "failed to install Ctrl+C handler");
    }
}

fn build_app(state: AppState) -> anyhow::Result<Router> {
    let static_dir = state.settings.static_dir.clone();
    let index_file = static_dir.join("index.html");
    let mut app = Router::new()
        .nest("/api/v1", api::routes(state.clone()))
        .fallback_service(get_service(
            ServeDir::new(static_dir).fallback(tower_http::services::ServeFile::new(index_file)),
        ))
        // Buffered extractors remain capped. Streaming upload handlers enforce their
        // own stricter byte quotas while consuming the body incrementally.
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers,
        ))
        .with_state(state.clone());

    if let Some(origin) = &state.settings.dev_origin {
        let origin = HeaderValue::from_str(origin)?;
        app = app.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::IF_MATCH,
                    header::HeaderName::from_static("x-csrf-token"),
                    header::HeaderName::from_static("x-setup-token"),
                    header::HeaderName::from_static("idempotency-key"),
                    header::HeaderName::from_static("x-dmx-archive-sha256"),
                    header::HeaderName::from_static("x-dmx-package-sha256"),
                ])
                .allow_credentials(true),
        );
    }
    Ok(app)
}

async fn security_headers(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https://shared.akamai.steamstatic.com; connect-src 'self'",
        ),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    if state.settings.reverse_proxy {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    response
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn no_arguments_selects_console_mode() {
        let mode = startup_mode(std::iter::empty()).expect("console mode");

        assert_eq!(mode, StartupMode::Console);
    }

    #[test]
    fn explicit_service_flag_selects_windows_service_mode() {
        let mode = startup_mode([OsString::from("--service")]).expect("service mode");

        assert_eq!(mode, StartupMode::WindowsService);
    }

    #[test]
    fn explicit_openapi_flag_selects_export_mode() {
        let mode = startup_mode([OsString::from("--print-openapi")]).expect("OpenAPI mode");

        assert_eq!(mode, StartupMode::PrintOpenApi);
    }

    #[test]
    fn unknown_or_extra_arguments_are_rejected() {
        assert!(startup_mode([OsString::from("--unknown")]).is_err());
        assert!(startup_mode([OsString::from("--service"), OsString::from("extra")]).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn service_mode_cannot_run_on_a_non_windows_host() {
        let error = run_windows_service().expect_err("service mode must be rejected");

        assert!(error.to_string().contains("only supported on Windows"));
    }
}
