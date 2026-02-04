use axum::{
    routing::get,
    extract::State,
    Json, Router,
};
use serde::Serialize;
use std::process::Command;
use std::sync::{Arc, Mutex};
use sysinfo::{Disks, System};
use walkdir::WalkDir;
use tokio::sync::RwLock;

use crate::core::AppState;
use crate::core::error::AppError;

#[derive(Debug, Serialize)]
pub struct SystemStatsResponse {
    pub cpu: f32,
    pub ram: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk: f32,
    pub disk_used: u64,
    pub disk_total: u64,
    pub players_current: u32,
    pub players_max: u32,
    pub cpu_cores: usize,
    pub managed_cpu: f32,
    pub managed_cpu_normalized: f32,
    pub managed_ram: u64,
    pub managed_disk: u64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct JavaVersion {
    pub path: String,
    pub version: String,
}

// Keep a static System instance for accurate CPU readings
lazy_static::lazy_static! {
    static ref SYSTEM: Arc<Mutex<System>> = Arc::new(Mutex::new(System::new_all()));
}

// System monitoring cache to reduce lock contention
lazy_static::lazy_static! {
    static ref SYSTEM_CACHE: Arc<RwLock<SystemStatsCache>> = Arc::new(RwLock::new(SystemStatsCache::default()));
}

#[derive(Debug)]
struct SystemStatsCache {
    last_update: std::time::Instant,
    cpu_usage: f32,
    ram_percent: f32,
    ram_used: u64,
    ram_total: u64,
    cpu_cores: usize,
}

impl Default for SystemStatsCache {
    fn default() -> Self {
        Self {
            last_update: std::time::Instant::now(),
            cpu_usage: 0.0,
            ram_percent: 0.0,
            ram_used: 0,
            ram_total: 0,
            cpu_cores: 0,
        }
    }
}

const CACHE_DURATION: std::time::Duration = std::time::Duration::from_secs(1);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/stats", get(get_system_stats))
        .route("/java-versions", get(get_java_versions))
}

async fn get_java_versions() -> Result<Json<Vec<JavaVersion>>, AppError> {
    let mut versions = Vec::new();
    let mut checked_paths = std::collections::HashSet::new();

    // 1. Check JAVA_HOME
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_bin = std::path::Path::new(&java_home).join("bin").join("java");
        if java_bin.exists() {
            if let Some(v) = check_java_version(&java_bin) {
                if checked_paths.insert(java_bin.to_string_lossy().to_string()) {
                    versions.push(v);
                }
            }
        }
    }

    // 2. Check PATH (via "java" command)
    if let Ok(path_var) = std::env::var("PATH") {
        for path in std::env::split_paths(&path_var) {
            let java_bin = path.join("java");
            if java_bin.exists() {
                let real_path = std::fs::canonicalize(&java_bin).unwrap_or_else(|_| java_bin.clone());
                if !checked_paths.contains(&real_path.to_string_lossy().to_string()) {
                    if let Some(v) = check_java_version(&real_path) {
                        checked_paths.insert(real_path.to_string_lossy().to_string());
                        versions.push(v);
                    }
                }
            }
        }
    }

    // 3. Scan common directories
    let common_dirs = [
        "/usr/lib/jvm",
        "/usr/java",
        "/opt/java",
        "/Library/Java/JavaVirtualMachines",
        "C:\\Program Files\\Java",
        "C:\\Program Files (x86)\\Java",
    ];

    for dir in common_dirs {
        let path = std::path::Path::new(dir);
        if path.exists() && path.is_dir() {
            for entry in WalkDir::new(path).max_depth(3).into_iter().filter_map(|e| e.ok()) {
                if entry.file_name() == "java" || entry.file_name() == "java.exe" {
                    let java_path = entry.path();
                    if java_path.parent().map(|p| p.file_name().unwrap_or_default() == "bin").unwrap_or(false) {
                        let real_path = std::fs::canonicalize(java_path).unwrap_or_else(|_| java_path.to_path_buf());
                        if !checked_paths.contains(&real_path.to_string_lossy().to_string()) {
                            if let Some(v) = check_java_version(&real_path) {
                                checked_paths.insert(real_path.to_string_lossy().to_string());
                                versions.push(v);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Json(versions))
}

fn check_java_version(path: &std::path::Path) -> Option<JavaVersion> {
    let output = Command::new(path).arg("-version").output().ok()?;
    let output_str = String::from_utf8_lossy(&output.stderr);
    for line in output_str.lines() {
        if line.contains("version") {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 2 {
                return Some(JavaVersion {
                    path: path.to_string_lossy().to_string(),
                    version: parts[1].to_string(),
                });
            }
        }
    }
    None
}

async fn get_system_stats(State(state): State<AppState>) -> Result<Json<SystemStatsResponse>, AppError> {
    let pm = &state.process_manager;
    let (cpu_usage, ram_percent, ram_used, ram_total, cpu_cores) = get_cached_system_stats().await;

    let disks = Disks::new_with_refreshed_list();
    let mut disk_total: u64 = 0;
    let mut disk_used: u64 = 0;

    for disk in disks.list() {
        let mount_point = disk.mount_point().to_string_lossy();
        if mount_point == "/" {
            disk_total = disk.total_space();
            disk_used = disk.total_space() - disk.available_space();
            break;
        }
    }

    if disk_total == 0 && !disks.list().is_empty() {
        let first_disk = &disks.list()[0];
        disk_total = first_disk.total_space();
        disk_used = first_disk.total_space() - first_disk.available_space();
    }

    let disk_percent = if disk_total > 0 {
        (disk_used as f64 / disk_total as f64 * 100.0) as f32
    } else {
        0.0
    };

    let players_current = pm.get_total_online_players().await;
    let players_max = 0; 

    let mut managed_cpu = 0.0;
    let mut managed_ram = 0;
    
    let procs = pm.get_processes_read_guard().await;
    for proc in procs.values() {
        managed_cpu += proc.last_cpu.read().map(|g| *g).unwrap_or(0.0);
        managed_ram += proc.last_memory.read().map(|g| *g).unwrap_or(0);
    }

    let mut managed_disk = 0;
    if let Ok(server_rows) = sqlx::query!("SELECT working_dir FROM servers").fetch_all(&state.pool).await {
        for row in server_rows {
            managed_disk += get_dir_size(&row.working_dir).await;
        }
    }

    Ok(Json(SystemStatsResponse {
        cpu: cpu_usage,
        ram: ram_percent,
        ram_used,
        ram_total,
        disk: disk_percent,
        disk_used,
        disk_total,
        players_current,
        players_max,
        cpu_cores,
        managed_cpu,
        managed_cpu_normalized: if cpu_cores > 0 { managed_cpu / cpu_cores as f32 } else { 0.0 },
        managed_ram,
        managed_disk,
    }))
}

async fn get_cached_system_stats() -> (f32, f32, u64, u64, usize) {
    {
        let cache = SYSTEM_CACHE.read().await;
        if cache.last_update.elapsed() < CACHE_DURATION {
            return (cache.cpu_usage, cache.ram_percent, cache.ram_used, cache.ram_total, cache.cpu_cores);
        }
    }

    let (cpu_usage, ram_percent, ram_used, ram_total, cpu_cores) = {
        let sys_lock = SYSTEM.lock();
        let mut sys = match sys_lock {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        sys.refresh_all();

        let cpu: f32 = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len().max(1) as f32;
        let total = sys.total_memory();
        let used = sys.used_memory();
        let percent = if total > 0 { (used as f64 / total as f64 * 100.0) as f32 } else { 0.0 };
        let cores = sys.cpus().len();
        (cpu, percent, used, total, cores)
    };

    {
        let mut cache = SYSTEM_CACHE.write().await;
        cache.cpu_usage = cpu_usage;
        cache.ram_percent = ram_percent;
        cache.ram_used = ram_used;
        cache.ram_total = ram_total;
        cache.cpu_cores = cpu_cores;
        cache.last_update = std::time::Instant::now();
    }

    (cpu_usage, ram_percent, ram_used, ram_total, cpu_cores)
}

/// Recursively calculates the size of a directory in bytes
async fn get_dir_size(path: &str) -> u64 {
    let path_buf = std::path::PathBuf::from(path);
    if !path_buf.exists() || !path_buf.is_dir() {
        return 0;
    }

    tokio::task::spawn_blocking(move || {
        WalkDir::new(path_buf)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    })
    .await
    .unwrap_or(0)
}