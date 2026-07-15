use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::watch;

use crate::core::{DbPool, error::AppError, events::EventHub};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
const RETENTION_DAYS: i64 = 7;
const MAX_POINTS_PER_INSTANCE: i64 = 10_080;
const MAX_DISK_SCAN_ENTRIES: usize = 200_000;

#[derive(Debug, Clone, serde::Serialize)]
struct ProcessSnapshot {
    cpu_usage: f64,
    memory_bytes: u64,
    uptime_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MetricsSnapshot {
    cpu_usage: f64,
    memory_bytes: u64,
    disk_bytes: u64,
    uptime_seconds: u64,
}

/// Starts one bounded collector for a supervised process tree. Dropping the
/// returned sender stops the task; no process handle is retained or reattached.
pub fn spawn_collector(
    pool: DbPool,
    events: EventHub,
    instance_id: String,
    root_pid: u32,
    instance_root: PathBuf,
) -> watch::Sender<bool> {
    let (stop_tx, mut stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut system = System::new();
        let mut disk_bytes = 0_u64;
        loop {
            tokio::select! {
                biased;
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = interval.tick() => {
                    let scan_root = instance_root.clone();
                    let refreshed = tokio::task::spawn_blocking(move || {
                        let process = refresh_process_tree(system, root_pid);
                        let disk = directory_size_no_follow(
                            &scan_root,
                            MAX_DISK_SCAN_ENTRIES,
                        );
                        (process.0, process.1, disk)
                    }).await;
                    let Ok((next_system, process, disk)) = refreshed else {
                        tracing::warn!(instance_id, "metrics collector worker failed");
                        break;
                    };
                    system = next_system;
                    let Some(process) = process else {
                        break;
                    };
                    match disk {
                        Ok(value) => disk_bytes = value,
                        Err(error) => tracing::warn!(instance_id, %error, "instance disk metric unavailable"),
                    }
                    let snapshot = MetricsSnapshot {
                        cpu_usage: process.cpu_usage,
                        memory_bytes: process.memory_bytes,
                        disk_bytes,
                        uptime_seconds: process.uptime_seconds,
                    };
                    if let Err(error) = persist(&pool, &instance_id, &snapshot).await {
                        tracing::warn!(instance_id, %error, "failed to persist server metrics");
                        continue;
                    }
                    events.publish(
                        "server.metrics",
                        Some(instance_id.clone()),
                        serde_json::to_value(&snapshot).unwrap_or_default(),
                    );
                }
            }
        }
    });
    stop_tx
}

fn refresh_process_tree(mut system: System, root_pid: u32) -> (System, Option<ProcessSnapshot>) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .without_tasks(),
    );
    let root = Pid::from_u32(root_pid);
    let root_process = match system.process(root) {
        Some(process) => process,
        None => return (system, None),
    };
    let uptime_seconds = root_process.run_time();
    let mut tree = HashSet::from([root]);
    loop {
        let before = tree.len();
        for (pid, process) in system.processes() {
            if process
                .parent()
                .is_some_and(|parent| tree.contains(&parent))
            {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            break;
        }
    }
    let mut cpu_usage = 0_f64;
    let mut memory_bytes = 0_u64;
    for pid in tree {
        if let Some(process) = system.process(pid) {
            cpu_usage += f64::from(process.cpu_usage());
            memory_bytes = memory_bytes.saturating_add(process.memory());
        }
    }
    (
        system,
        Some(ProcessSnapshot {
            cpu_usage,
            memory_bytes,
            uptime_seconds,
        }),
    )
}

async fn persist(
    pool: &DbPool,
    instance_id: &str,
    snapshot: &MetricsSnapshot,
) -> Result<(), AppError> {
    let memory_bytes = i64::try_from(snapshot.memory_bytes)
        .map_err(|_| AppError::Internal("metric value overflow".into()))?;
    let disk_bytes = i64::try_from(snapshot.disk_bytes)
        .map_err(|_| AppError::Internal("metric value overflow".into()))?;
    let uptime_seconds = i64::try_from(snapshot.uptime_seconds)
        .map_err(|_| AppError::Internal("metric value overflow".into()))?;
    let now = chrono::Utc::now();
    let mut transaction = pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO server_metrics
            (id, server_id, cpu_usage, memory_bytes, disk_bytes, uptime_seconds, recorded_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(instance_id)
    .bind(snapshot.cpu_usage)
    .bind(memory_bytes)
    .bind(disk_bytes)
    .bind(uptime_seconds)
    .bind(now.to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM server_metrics WHERE server_id = ? AND recorded_at < ?")
        .bind(instance_id)
        .bind((now - chrono::Duration::days(RETENTION_DAYS)).to_rfc3339())
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        r#"
        DELETE FROM server_metrics
        WHERE server_id = ? AND id IN (
            SELECT id FROM server_metrics WHERE server_id = ?
            ORDER BY recorded_at DESC LIMIT -1 OFFSET ?
        )
        "#,
    )
    .bind(instance_id)
    .bind(instance_id)
    .bind(MAX_POINTS_PER_INSTANCE)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

fn directory_size_no_follow(root: &Path, max_entries: usize) -> Result<u64, std::io::Error> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "instance root is not a regular directory",
        ));
    }
    let mut total = 0_u64;
    let mut visited = 0_usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            visited = visited.saturating_add(1);
            if visited > max_entries {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "instance contains too many entries",
                ));
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if is_link_like(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.checked_add(metadata.len()).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "disk metric overflow")
                })?;
            }
        }
    }
    Ok(total)
}

fn is_link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_scan_is_bounded_and_does_not_follow_links() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("world.dat"), [0_u8; 32]).unwrap();
        fs::create_dir(directory.path().join("region")).unwrap();
        fs::write(directory.path().join("region/r.0.0.mca"), [0_u8; 64]).unwrap();
        assert_eq!(directory_size_no_follow(directory.path(), 10).unwrap(), 96);
        assert!(directory_size_no_follow(directory.path(), 1).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", directory.path().join("host")).unwrap();
            assert_eq!(directory_size_no_follow(directory.path(), 10).unwrap(), 96);
        }
    }

    #[test]
    fn current_process_can_be_sampled_without_panicking() {
        let (_, snapshot) = refresh_process_tree(System::new(), std::process::id());
        let snapshot = snapshot.expect("current process must be visible");
        assert!(snapshot.memory_bytes > 0);
    }
}
