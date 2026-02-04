use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use tracing::info;

use super::player_detection::PlayerDetectionPatterns;

use crate::error::AppError;
use walkdir::WalkDir;



/// Manages game server processes
use crate::db::DbPool;

#[derive(Clone)]
pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, ServerProcess>>>,
    pool: Option<DbPool>,
}

pub struct ServerProcess {
    child: Option<Child>,
    install_task: Option<tokio::task::AbortHandle>,
    log_tx: broadcast::Sender<String>,
    players: Arc<std::sync::RwLock<HashSet<String>>>,
    pub last_metrics: Arc<std::sync::RwLock<Option<String>>>,
    pub last_cpu: Arc<std::sync::RwLock<f32>>,
    pub last_cpu_normalized: Arc<std::sync::RwLock<f32>>,
    pub last_memory: Arc<std::sync::RwLock<u64>>,
    pub last_disk: Arc<std::sync::RwLock<u64>>,
    pub working_dir: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub auth_required: Arc<std::sync::RwLock<bool>>,
}

impl ProcessManager {
    pub fn new(pool: Option<DbPool>) -> Self {
        let processes = Arc::new(RwLock::new(HashMap::<String, ServerProcess>::new()));
        
        // Spawn metrics loop
        let processes_clone = processes.clone();
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            let mut system = sysinfo::System::new_all();
            let mut tick_count: u64 = 0;
            loop {
                // Refresh first so we have accurate CPU readings even on first iteration
                system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                
                {
                    let procs = processes_clone.read().await;
                    for (server_id, server_proc) in procs.iter() {
                        if let Some(child) = &server_proc.child {
                            let pid = sysinfo::Pid::from_u32(child.id());
                            if let Some(process) = system.process(pid) {
                                let cpu = process.cpu_usage();
                                let cores = system.cpus().len() as f32;
                                let cpu_normalized = if cores > 0.0 { cpu / cores } else { 0.0 };
                                let memory = process.memory(); // in bytes
                                let players_list: Vec<String> = server_proc.players.read()
                                    .map(|p| p.iter().cloned().collect())
                                    .unwrap_or_default();
                                let player_count = players_list.len();
                                
                                let mut metrics_json = serde_json::json!({
                                    "cpu": cpu,
                                    "cpu_normalized": cpu_normalized,
                                    "memory": memory,
                                    "players": player_count,
                                    "players_list": players_list
                                });

                                // Calculate disk size every ~30 seconds (15 ticks) OR at tick 0
                                let mut disk_size: u64 = 0;
                                if tick_count.is_multiple_of(15) {
                                    let server_path = &server_proc.working_dir;
                                    let size: u64 = WalkDir::new(server_path)
                                        .into_iter()
                                        .filter_map(|entry| entry.ok())
                                        .filter_map(|entry| entry.metadata().ok())
                                        .filter(|metadata| metadata.is_file())
                                        .map(|metadata| metadata.len())
                                        .sum();
                                    
                                    if let Some(obj) = metrics_json.as_object_mut() {
                                        obj.insert("disk_bytes".to_string(), serde_json::Value::Number(serde_json::Number::from(size)));
                                    }
                                    if let Ok(mut disk_cache) = server_proc.last_disk.write() {
                                        *disk_cache = size;
                                    }
                                    disk_size = size;
                                } else {
                                    // Use cached disk value
                                    if let Ok(disk_cache) = server_proc.last_disk.read() {
                                        disk_size = *disk_cache;
                                    }
                                }

                                let metrics_msg = format!("[METRICS]: {metrics_json}");
                                let _ = server_proc.log_tx.send(metrics_msg.clone());
                                if let Ok(mut cache) = server_proc.last_metrics.write() {
                                    *cache = Some(metrics_msg);
                                }
                                if let Ok(mut cpu_cache) = server_proc.last_cpu.write() {
                                    *cpu_cache = cpu;
                                }
                                if let Ok(mut cpu_norm_cache) = server_proc.last_cpu_normalized.write() {
                                    *cpu_norm_cache = cpu_normalized;
                                }
                                if let Ok(mut mem_cache) = server_proc.last_memory.write() {
                                    *mem_cache = memory;
                                }

                                // Save metrics to DB every 30 seconds (15 ticks)
                                if tick_count.is_multiple_of(15) {
                                    if let Some(pool) = &pool_clone {
                                        let pool = pool.clone();
                                        let server_id = server_id.clone();
                                        let player_count = if let Ok(players) = server_proc.players.read() {
                                            players.len() as i32
                                        } else {
                                            0
                                        };
                                        
                                        tokio::spawn(async move {
                                            let _ = crate::api::metrics::insert_metric(
                                                &pool,
                                                &server_id,
                                                cpu_normalized as f64,
                                                memory as i64,
                                                disk_size as i64,
                                                player_count,
                                            ).await;
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // Cleanup old metrics every hour (1800 ticks at 2s interval)
                if tick_count.is_multiple_of(1800) && tick_count > 0 {
                    if let Some(pool) = &pool_clone {
                        let pool = pool.clone();
                        tokio::spawn(async move {
                            let deleted = crate::api::metrics::cleanup_old_metrics(&pool, 7).await;
                            if let Ok(count) = deleted {
                                if count > 0 {
                                    tracing::info!("Cleaned up {} old metric records", count);
                                }
                            }
                        });
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                tick_count += 1;
            }
        });

        Self {
            processes,
            pool,
        }
    }

    pub fn is_running(&self, server_id: &str) -> bool {
        // We need to check if the process is actually alive, not just in the map
        if let Ok(mut processes) = self.processes.try_write() {
            if let Some(proc) = processes.get_mut(server_id) {
                // If child is None, it means it's a virtual process (installing/updating)
                // So it is technically "running" if install_task is active
                if proc.child.is_none() {
                    return true;
                }
                
                if let Some(child) = &mut proc.child {
                    // Check if process is still running
                    match child.try_wait() {
                        Ok(None) => {
                            // Process is still running
                            return true;
                        }
                        Ok(Some(_status)) => {
                            // Process has exited, remove from map
                            info!("Server {} process has exited, cleaning up", server_id);
                            processes.remove(server_id);
                            return false;
                        }
                        Err(_) => {
                            // Error checking status, assume not running
                            return false;
                        }
                    }
                }
            }
        }
        false
    }
    
    pub fn is_installing(&self, server_id: &str) -> bool {
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                return proc.child.is_none();
            }
        }
        false
    }

    pub fn is_auth_required(&self, server_id: &str) -> bool {
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                if let Ok(mut auth) = proc.auth_required.write() {
                    if *auth {
                        // Check if auth.enc exists in working dir
                        // If it exists, it means we are authenticated
                        let auth_file = std::path::Path::new(&proc.working_dir).join("auth.enc");
                        if auth_file.exists() {
                             // Update state to false since we found the file
                            *auth = false;
                            return false;
                        }
                    }
                    return *auth;
                }
            }
        }
        false
    }

    pub fn set_auth_required(&self, server_id: &str, required: bool) {
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                if let Ok(mut auth) = proc.auth_required.write() {
                    *auth = required;
                }
            }
        }
    }

    pub fn subscribe_logs(&self, server_id: &str) -> broadcast::Receiver<String> {
        let (_tx, rx) = broadcast::channel(1000);
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                return proc.log_tx.subscribe();
            }
        }
        rx
    }

    pub async fn register_installing(&self, server_id: &str, working_dir: &str, abort_handle: Option<tokio::task::AbortHandle>) -> Result<(), AppError> {
        let mut processes = self.processes.write().await;
         if processes.contains_key(server_id) {
             return Err(AppError::BadRequest("Server already active".into()));
         }

         let (log_tx, _) = broadcast::channel::<String>(1000);
         // Broadcast initial installing status
         let _ = log_tx.send("[STATUS]: installing".to_string());

         let players = Arc::new(std::sync::RwLock::new(HashSet::new()));

         processes.insert(
             server_id.to_string(),
             ServerProcess { 
                 child: None,
                 install_task: abort_handle,
                 log_tx, 
                 players,
                 last_metrics: Arc::new(std::sync::RwLock::new(None)),
                 last_cpu: Arc::new(std::sync::RwLock::new(0.0)),
                 last_cpu_normalized: Arc::new(std::sync::RwLock::new(0.0)),
                 last_memory: Arc::new(std::sync::RwLock::new(0)),
                 last_disk: Arc::new(std::sync::RwLock::new(0)),
                 working_dir: working_dir.to_string(),
                 started_at: Some(chrono::Utc::now()),
                 auth_required: Arc::new(std::sync::RwLock::new(false)),
             },
         );
         Ok(())
    }

    pub async fn broadcast_log(&self, server_id: &str, message: String) {
        let processes = self.processes.read().await;
        if let Some(proc) = processes.get(server_id) {
            let _ = proc.log_tx.send(message);
        }
    }

    /// Remove a process from manager (used when installation finishes)
    pub async fn remove(&self, server_id: &str) {
        let mut processes = self.processes.write().await;
        processes.remove(server_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        server_id: &str,
        executable_path: &str,
        working_dir: &str,
        java_path: Option<&str>,
        _min_memory: Option<&str>,
        max_memory: Option<&str>,
        extra_args: Option<&str>,
        config: Option<&serde_json::Value>,
        game_type: &str,
    ) -> Result<(), AppError> {
        let mut processes = self.processes.write().await;

        if processes.contains_key(server_id) {
            return Err(AppError::BadRequest("Server already running".into()));
        }

        // Build command based on game type (Hytale uses Java)
        let java = java_path.unwrap_or("java");
        let max_mem = max_memory.unwrap_or("8G");

        // Config is generated by servers.rs (Hytale config.json)
        // Legacy server.properties/world-config.json generation removed.


        let final_working_dir = std::path::PathBuf::from(working_dir);
        let assets_path = "Assets.zip".to_string();

        let mut cmd = Command::new(java);
        cmd.current_dir(&final_working_dir);

        // Smart Memory Adjustment: User provided max_mem is now the HEAP SIZE (-Xmx)
        // We calculate Xms based on this.
        let heap_target_bytes = parse_memory_to_bytes(max_mem);
        let (xms, xmx) = calculate_jvm_tokens(heap_target_bytes);

        cmd.arg(format!("-Xms{xms}"))
            .arg(format!("-Xmx{xmx}"))
            .arg("-Dterminal.jline=true")
            .arg("-Dterminal.ansi=true")
            .arg("-XX:AOTCache=HytaleServer.aot");

        // Pass port and bind address via --bind
        // Note: These are program arguments, but we'll put them before -jar as well 
        // to keep logic clean, Or better: move them after.
        // Actually, JVM flags MUST be before -jar. Program args MUST be after.
        // Hytale's --bind and --assets are program args.
        
        if let Some(args) = extra_args {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }

        cmd.arg("-jar")
            .arg(executable_path)
            .arg("--assets")
            .arg(assets_path);

        if let Some(cfg) = config {
            let port = cfg.get("port")
                .or(cfg.get("Port"))
                .and_then(|v| v.as_u64())
                .unwrap_or(5520);
             
            let bind_ip = cfg.get("bind_address")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0.0");

            cmd.arg("--bind");
            cmd.arg(format!("{bind_ip}:{port}"));
        } else {
            // Default to standard port if no config
            cmd.arg("--bind");
            cmd.arg("0.0.0.0:5520");
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to start server: {e}")))?;

        info!("Started server {} with PID {:?}", server_id, child.id());

        // Create log file
        let logs_dir = std::path::Path::new(working_dir).join("logs");
        if !logs_dir.exists() {
             let _ = std::fs::create_dir_all(&logs_dir);
        }
        let log_path = logs_dir.join("console.log");
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log_path)
            .ok()
            .map(|f| Arc::new(std::sync::Mutex::new(f)));

        // Create log broadcaster
        let (log_tx, _) = broadcast::channel::<String>(1000);
        let _ = log_tx.send("[STATUS]: running".to_string());

        // Create players tracker
        let players = Arc::new(std::sync::RwLock::new(HashSet::new()));

        let auth_required = Arc::new(std::sync::RwLock::new(false));

        // Spawn task to read stdout
        if let Some(stdout) = child.stdout.take() {
            let tx = log_tx.clone();
            let players_clone = players.clone();
            let server_id_clone = server_id.to_string();
            let log_file_clone = log_file.clone();
            let pool_clone_opt = self.pool.clone();
            let auth_required_clone = auth_required.clone();
            
            let game_type_clone = game_type.to_string();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                // Use game-specific detection patterns
                let patterns = PlayerDetectionPatterns::for_game_type(&game_type_clone);
                let join_re = patterns.join_regex;
                let leave_re = patterns.leave_regex;
                let server_started_re = patterns.server_ready_regex;

                let pool_clone = pool_clone_opt; // Capture optional pool

                for line in reader.lines().map_while(Result::ok) {
                    // tracing::debug!("Server {} log line: {}", server_id_clone, line);
                    
                    // Write to file
                    if let Some(f) = &log_file_clone {
                        if let Ok(mut guard) = f.lock() {
                            let _ = writeln!(guard, "{line}");
                        }
                    }

                    // Try to match player events
                    if let Some(caps) = join_re.captures(&line) {
                        if let Some(name) = caps.get(1) {
                            let player_name = name.as_str().to_string();
                            info!("Player joined server {}: {}", server_id_clone, player_name);
                            if let Ok(mut p) = players_clone.write() {
                                p.insert(player_name.clone());
                            }
                            
                            // DB Update: Connect
                            if let Some(pool) = &pool_clone {
                                let pool = pool.clone();
                                let s_id = server_id_clone.clone();
                                let p_name = player_name.clone();
                                tokio::spawn(async move {
                                    let now = chrono::Utc::now().to_rfc3339();
                                    let _ = sqlx::query(
                                        "INSERT INTO server_players (server_id, player_name, first_seen, last_seen, is_online) 
                                         VALUES (?, ?, ?, ?, 1)
                                         ON CONFLICT(server_id, player_name) DO UPDATE SET 
                                         last_seen = excluded.last_seen, 
                                         is_online = 1"
                                    )
                                    .bind(s_id)
                                    .bind(p_name)
                                    .bind(&now) // first_seen
                                    .bind(&now) // last_seen
                                    .execute(&pool)
                                    .await;
                                });
                            }
                        }
                    } else if let Some(caps) = leave_re.captures(&line) {
                        if let Some(name) = caps.get(1) {
                            let player_name = name.as_str().to_string();
                            if let Ok(mut p) = players_clone.write() {
                                p.remove(&player_name);
                            }

                            // DB Update: Disconnect
                            if let Some(pool) = &pool_clone {
                                let pool = pool.clone();
                                let s_id = server_id_clone.clone();
                                let p_name = player_name.clone();
                                tokio::spawn(async move {
                                    let now = chrono::Utc::now().to_rfc3339();
                                    let _ = sqlx::query(
                                        "UPDATE server_players SET is_online = 0, last_seen = ? WHERE server_id = ? AND player_name = ?"
                                    )
                                    .bind(now)
                                    .bind(s_id)
                                    .bind(p_name)
                                    .execute(&pool)
                                    .await;
                                });
                            }
                        }
                    } else if server_started_re.is_match(&line) {
                         let _ = tx.send("[STATUS]: running".to_string());
                    }

                     // Runtime Auth Detection
                    if (line.contains("IMPORTANT") && (line.contains("authentifier") || line.contains("authenticate"))) ||
                       (line.contains("[HytaleServer] No server tokens configured")) ||
                       (line.contains("/auth login to authenticate")) {
                         if let Ok(mut auth) = auth_required_clone.write() {
                             *auth = true;
                             let _ = tx.send("[STATUS]: auth_required".to_string());
                         }
                    }

                    let _ = tx.send(line);
                }
                
                info!("Server {} stdout stream ended", server_id_clone);
                let _ = tx.send("[STATUS]: stopped".to_string());
                
                 // Write stop marker to file
                if let Some(f) = &log_file_clone {
                    if let Ok(mut guard) = f.lock() {
                        let _ = writeln!(guard, "[Server Stopped]");
                    }
                }
            });
        }

        // Spawn task to read stderr
        if let Some(stderr) = child.stderr.take() {
            let tx = log_tx.clone();
            let server_id_clone = server_id.to_string();
            let log_file_clone = log_file.clone();
            let auth_required_clone = auth_required.clone();

            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let log_line = format!("[STDERR] {line}");
                     // Write to file
                    if let Some(f) = &log_file_clone {
                        if let Ok(mut guard) = f.lock() {
                            let _ = writeln!(guard, "{log_line}");
                        }
                    }
                    let _ = tx.send(log_line);
                    
                    // Runtime Auth Detection (stderr)
                     if (line.contains("IMPORTANT") && (line.contains("authentifier") || line.contains("authenticate"))) ||
                       (line.contains("[HytaleServer] No server tokens configured")) ||
                       (line.contains("/auth login to authenticate")) {
                            if let Ok(mut auth) = auth_required_clone.write() {
                             *auth = true;
                             let _ = tx.send("[STATUS]: auth_required".to_string());
                         }
                    }
                }
                info!("Server {} stderr stream ended", server_id_clone);
            });
        }

        processes.insert(
            server_id.to_string(),
            ServerProcess { 
                child: Some(child), 
                install_task: None,
                log_tx, 
                players,
                last_metrics: Arc::new(std::sync::RwLock::new(None)),
                last_cpu: Arc::new(std::sync::RwLock::new(0.0)),
                last_cpu_normalized: Arc::new(std::sync::RwLock::new(0.0)),
                last_memory: Arc::new(std::sync::RwLock::new(0)),
                last_disk: Arc::new(std::sync::RwLock::new(0)),
                working_dir: working_dir.to_string(),
                started_at: Some(chrono::Utc::now()),
                auth_required,
            },
        );

        Ok(())
    }

    pub async fn stop(&self, server_id: &str) -> Result<(), AppError> {
        let mut processes = self.processes.write().await;

        let proc = processes
            .get_mut(server_id)
            .ok_or_else(|| AppError::NotFound("Server not running".into()))?;

        // If it's an installation task, abort it
        if let Some(task) = &proc.install_task {
            task.abort();
            info!("Aborted installation task for server {}", server_id);
            // We can return early or continue to cleanup
        }

        if let Some(child) = &mut proc.child {
             // Try graceful shutdown first (send quit command)
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = writeln!(stdin, "/shutdown");
            }

            // Wait a bit for graceful shutdown
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            // Force kill if still running
            if child.try_wait().map_err(|e| AppError::Internal(e.to_string()))?.is_none() {
                child
                    .kill()
                    .map_err(|e| AppError::Internal(format!("Failed to kill server: {e}")))?;
            }
        }
        
        processes.remove(server_id);
        info!("Stopped server {}", server_id);

        Ok(())
    }

    /// Force kill a server immediately without graceful shutdown
    pub async fn kill(&self, server_id: &str) -> Result<(), AppError> {
        let mut processes = self.processes.write().await;

        let proc = processes
            .get_mut(server_id)
            .ok_or_else(|| AppError::NotFound("Server not running".into()))?;

        if let Some(child) = &mut proc.child {
            // Force kill immediately
            child
                .kill()
                .map_err(|e| AppError::Internal(format!("Failed to kill server: {e}")))?;
        }

        processes.remove(server_id);
        info!("Killed server {}", server_id);

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn restart(
        &self,
        server_id: &str,
        executable_path: &str,
        working_dir: &str,
        java_path: Option<&str>,
        min_memory: Option<&str>,
        max_memory: Option<&str>,
        extra_args: Option<&str>,
        config: Option<&serde_json::Value>,
        game_type: &str,
    ) -> Result<(), AppError> {
        // Stop if running
        if self.is_running(server_id) {
            self.stop(server_id).await?;
        }

        // Start again
        self.start(
            server_id,
            executable_path,
            working_dir,
            java_path,
            min_memory,
            max_memory,
            extra_args,
            config,
            game_type,
        )
        .await
    }

    pub async fn send_command(&self, server_id: &str, command: &str) -> Result<(), AppError> {
        let mut processes = self.processes.write().await;

        let proc = processes
            .get_mut(server_id)
            .ok_or_else(|| AppError::NotFound("Server not running".into()))?;

        if let Some(child) = &mut proc.child {
            if let Some(stdin) = child.stdin.as_mut() {
                writeln!(stdin, "{command}")
                    .map_err(|e| AppError::Internal(format!("Failed to send command: {e}")))?;
            }
        }

        Ok(())
    }





    pub async fn get_online_players(&self, server_id: &str) -> Option<Vec<String>> {
        let processes = self.processes.read().await;
        if let Some(proc) = processes.get(server_id) {
            if let Ok(players) = proc.players.read() {
                return Some(players.iter().cloned().collect());
            }
        }
        None
    }

    pub async fn get_server_started_at(&self, server_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                return proc.started_at;
            }
        }
        None
    }

    pub async fn get_total_online_players(&self) -> u32 {
        let mut total = 0;
        let processes = self.processes.read().await;
        for proc in processes.values() {
             if let Ok(players) = proc.players.read() {
                 total += players.len() as u32;
             }
        }
        total
    }

    pub async fn get_server_pid(&self, server_id: &str) -> Option<u32> {
        if let Ok(processes) = self.processes.try_read() {
            if let Some(proc) = processes.get(server_id) {
                if let Some(child) = &proc.child {
                    return Some(child.id());
                }
            }
        }
        None
    }

    pub async fn get_last_metrics(&self, server_id: &str) -> Option<String> {
        let processes = self.processes.read().await;
        if let Some(proc) = processes.get(server_id) {
            if let Ok(cache) = proc.last_metrics.read() {
                return cache.clone();
            }
        }
        None
    }

    pub async fn get_metrics_data(&self, server_id: &str) -> (f32, f32, u64, u64) {
        let processes = self.processes.read().await;
        if let Some(proc) = processes.get(server_id) {
            let cpu = proc.last_cpu.read().map(|g| *g).unwrap_or(0.0);
            let cpu_norm = proc.last_cpu_normalized.read().map(|g| *g).unwrap_or(0.0);
            let mem = proc.last_memory.read().map(|g| *g).unwrap_or(0);
            let disk = proc.last_disk.read().map(|g| *g).unwrap_or(0);
            return (cpu, cpu_norm, mem, disk);
        }
        (0.0, 0.0, 0, 0)
    }

    pub async fn get_processes_read_guard(&self) -> tokio::sync::RwLockReadGuard<'_, HashMap<String, ServerProcess>> {
        self.processes.read().await
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new(None)
    }
}


use crate::utils::memory::{parse_memory_to_bytes, calculate_jvm_tokens};

