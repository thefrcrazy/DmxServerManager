use axum::{
    routing::get,
    extract::State,
    Json, Router,
};
use serde::Serialize;
use std::process::Command;
use std::sync::Mutex;
use sysinfo::{Disks, System};
use walkdir::WalkDir;

use crate::AppState;
use crate::error::AppError;

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
    pub managed_cpu_normalized: f32, // New field for normalized display (0-100%)
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
    static ref SYSTEM: Mutex<System> = Mutex::new(System::new_all());
}

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
                // Resolve symlink to get real path
                let real_path = std::fs::canonicalize(&java_bin).unwrap_or(java_bin.clone());
                
                if !checked_paths.contains(&real_path.to_string_lossy().to_string()) {
                    if let Some(v) = check_java_version(&real_path) {
                        checked_paths.insert(real_path.to_string_lossy().to_string());
                        versions.push(v);
                    }
                }
            }
        }
    }

    // 3. Scan common directories (Linux/macOS)
    let common_dirs = [
        "/usr/lib/jvm",                        // Linux standard
        "/usr/java",                           // Linux alternative
        "/opt/java",                           // Linux opt
        "/Library/Java/JavaVirtualMachines",   // macOS
        "C:\\Program Files\\Java",             // Windows
        "C:\\Program Files (x86)\\Java",       // Windows x86
    ];

    for dir in common_dirs {
        let path = std::path::Path::new(dir);
        if path.exists() && path.is_dir() {
            // Find "java" binaries recursively but with limited depth
            for entry in WalkDir::new(path).max_depth(3).into_iter().filter_map(|e| e.ok()) {
                if entry.file_name() == "java" || entry.file_name() == "java.exe" {
                    let java_path = entry.path();
                    // Ensure it is executable/binary (rudimentary check by path name ending in bin/java)
                    if java_path.parent().map(|p| p.file_name().unwrap_or_default() == "bin").unwrap_or(false) {
                        let real_path = std::fs::canonicalize(java_path).unwrap_or(java_path.to_path_buf());
                         
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
    let output = Command::new(path)
        .arg("-version")
        .output()
        .ok()?;
    
    // Java version info is often in stderr
    let output_str = String::from_utf8_lossy(&output.stderr);
    
    // Parse version from string like: "openjdk version \"17.0.8\" 2023-07-18"
    // or "java version \"1.8.0_381\""
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
    let (cpu_usage, ram_percent, ram_used, ram_total) = {
        let mut sys = SYSTEM.lock().unwrap();
        sys.refresh_all();
        
        // CPU usage (average across all CPUs)
        let cpu: f32 = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>()
            / sys.cpus().len().max(1) as f32;

        // RAM usage
        let total = sys.total_memory();
        let used = sys.used_memory();
        let percent = if total > 0 {
            (used as f64 / total as f64 * 100.0) as f32
        } else {
            0.0
        };
        
        (cpu, percent, used, total)
    };

    // Disk usage - only count the main disk (mounted at "/" on macOS/Linux)
    let disks = Disks::new_with_refreshed_list();
    let mut disk_total: u64 = 0;
    let mut disk_used: u64 = 0;

    for disk in disks.list() {
        let mount_point = disk.mount_point().to_string_lossy();
        // Only count the root filesystem
        if mount_point == "/" {
            disk_total = disk.total_space();
            disk_used = disk.total_space() - disk.available_space();
            break;
        }
    }

    // Fallback: if no root disk found, use the first disk
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

    // Players
    let players_current = pm.get_total_online_players().await;
    let players_max = 0; 

    let cpu_cores = {
        let sys = SYSTEM.lock().unwrap();
        sys.cpus().len()
    };

    // Managed resource usage
    let mut managed_cpu = 0.0;
    let mut managed_ram = 0;
    let mut managed_disk = 0;

    let procs = pm.get_processes_read_guard().await;
    for proc in procs.values() {
        managed_cpu += *proc.last_cpu.read().unwrap();
        managed_ram += *proc.last_memory.read().unwrap();
        managed_disk += *proc.last_disk.read().unwrap();
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
