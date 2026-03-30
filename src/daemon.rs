use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use tokio::io::AsyncWriteExt;
use tokio::signal;
use tracing::{error, info, warn};

use lukan_agent::{PipelineScheduler, WorkerScheduler};
use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig};

/// JSON structure written to daemon.lock
#[derive(serde::Serialize, serde::Deserialize)]
struct DaemonLock {
    pid: u32,
    port: u16,
    #[serde(default)]
    local_only: bool,
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Start the worker daemon (foreground by default)
    Start {
        /// Detach and run in the background
        #[arg(long, short)]
        detach: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Check if the daemon is running
    Status,
}

pub async fn handle_daemon_command(command: DaemonCommands) -> Result<()> {
    match command {
        DaemonCommands::Start { detach } => {
            // Read local_only from config or LUKAN_LOCAL_ONLY env var
            let local_only = std::env::var("LUKAN_LOCAL_ONLY").is_ok_and(|v| v == "1")
                || std::fs::read_to_string(LukanPaths::config_file())
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("localOnly").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
            if detach {
                start_detached_with_opts(local_only)?;
            } else {
                run_daemon_with_opts(local_only).await?;
            }
        }
        DaemonCommands::Stop => {
            stop_daemon()?;
        }
        DaemonCommands::Status => {
            if let Some(lock) = read_lock_file() {
                if is_process_alive(lock.pid) {
                    println!("  Daemon is running (PID {}, port {})", lock.pid, lock.port);
                } else {
                    cleanup_lock_files();
                    println!("  Daemon is not running (cleaned stale lock file)");
                }
            } else if is_daemon_running() {
                let pid = read_pid_file().unwrap_or(0);
                println!("  Worker daemon is running (PID {pid}) — legacy PID file, no port info");
            } else {
                println!("  Daemon is not running");
            }
        }
    }
    Ok(())
}

/// Run the daemon in the current process (foreground).
/// Starts both the web server and the worker scheduler.
async fn run_daemon_with_opts(local_only: bool) -> Result<()> {
    if is_daemon_running() {
        let pid = read_pid_file().unwrap_or(0);
        bail!("Daemon already running (PID {pid})");
    }

    let pid = std::process::id();

    // Write legacy PID file (for backward compat during transition)
    let pid_path = LukanPaths::daemon_pid_file();
    std::fs::write(&pid_path, pid.to_string()).context("Failed to write daemon PID file")?;

    info!(pid, "Daemon starting");

    // Load config
    let config = ConfigManager::load().await?;
    let credentials = CredentialsManager::load().await?;
    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    // ── Start web server ──
    let preferred_port = std::env::var("LUKAN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .or(resolved.config.web_port)
        .unwrap_or(3000u16);

    // Try preferred port first, then fall back to nearby ports
    let (actual_port, _web_handle) = {
        let mut last_err = None;
        let mut result = None;
        for port in preferred_port..preferred_port.saturating_add(10) {
            match lukan_web::start_daemon_server_with_opts(resolved.clone(), port, local_only).await
            {
                Ok(r) => {
                    result = Some(r);
                    break;
                }
                Err(e) => {
                    info!(port, error = %e, "Port unavailable, trying next");
                    last_err = Some(e);
                }
            }
        }
        match result {
            Some(r) => r,
            None => return Err(last_err.unwrap().context("All ports 3000-3009 unavailable")),
        }
    };

    info!(port = actual_port, "Web server started inside daemon");

    // Write JSON lock file with PID + port
    let lock_path = LukanPaths::daemon_lock_file();
    let lock = DaemonLock {
        pid,
        port: actual_port,
        local_only,
    };
    std::fs::write(
        &lock_path,
        serde_json::to_string(&lock).context("Failed to serialize daemon lock")?,
    )
    .context("Failed to write daemon lock file")?;

    // ── Start worker scheduler ──
    let scheduler = WorkerScheduler::new(resolved.clone());
    scheduler.start().await;

    // Spawn notification writer: subscribes to scheduler broadcast and appends to JSONL file
    let mut notify_rx = scheduler.subscribe();
    tokio::spawn(async move {
        let path = LukanPaths::worker_notifications_file();
        while let Ok(notification) = notify_rx.recv().await {
            if let Ok(line) = serde_json::to_string(&notification) {
                match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    Ok(mut file) => {
                        let _ = file.write_all(format!("{line}\n").as_bytes()).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to write worker notification to file");
                    }
                }
            }
        }
    });

    // ── Start pipeline scheduler ──
    let pipeline_scheduler = PipelineScheduler::new(resolved);
    pipeline_scheduler.start().await;

    // Spawn pipeline notification writer
    let mut pipeline_notify_rx = pipeline_scheduler.subscribe();
    tokio::spawn(async move {
        let path = LukanPaths::pipeline_notifications_file();
        while let Ok(notification) = pipeline_notify_rx.recv().await {
            if let Ok(line) = serde_json::to_string(&notification) {
                match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    Ok(mut file) => {
                        let _ = file.write_all(format!("{line}\n").as_bytes()).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to write pipeline notification to file");
                    }
                }
            }
        }
    });

    // ── Start relay bridge if relay credentials exist ──
    let mut relay_bridge = None;
    if let Some(relay_config) = lukan_core::relay::RelayConfig::load_if_enabled().await {
        info!(
            relay_url = %relay_config.relay_url,
            "Starting relay bridge on port {actual_port}"
        );
        let mut bridge = crate::relay_bridge::RelayBridge::new(relay_config, actual_port);
        bridge.start();
        relay_bridge = Some(bridge);
    }

    info!(
        pid,
        port = actual_port,
        "Daemon running (web + workers + relay)"
    );

    // ── Poll workers.json and pipelines.json for changes ──
    let workers_file = LukanPaths::workers_file();
    let mut last_mtime = file_mtime(&workers_file);
    let pipelines_file = LukanPaths::pipelines_file();
    let mut last_pipelines_mtime = file_mtime(&pipelines_file);

    let mut poll_interval = tokio::time::interval(Duration::from_secs(3));
    poll_interval.tick().await; // skip first immediate tick

    let shutdown = async {
        let ctrl_c = signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
        }
    };
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                info!("Daemon received shutdown signal");
                break;
            }
            _ = poll_interval.tick() => {
                let current_mtime = file_mtime(&workers_file);
                if current_mtime != last_mtime {
                    info!("workers.json changed, reloading");
                    scheduler.reload().await;
                    last_mtime = current_mtime;
                }
                let current_pipelines_mtime = file_mtime(&pipelines_file);
                if current_pipelines_mtime != last_pipelines_mtime {
                    info!("pipelines.json changed, reloading");
                    pipeline_scheduler.reload().await;
                    last_pipelines_mtime = current_pipelines_mtime;
                }
            }
        }
    }

    // Cleanup
    if let Some(mut bridge) = relay_bridge {
        bridge.stop();
    }
    scheduler.stop();
    pipeline_scheduler.stop();
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&lock_path);
    info!("Daemon stopped");

    Ok(())
}

/// Spawn the daemon as a detached background process.
fn start_detached_with_opts(local_only: bool) -> Result<()> {
    if is_daemon_running() {
        let pid = read_pid_file().unwrap_or(0);
        println!("  Daemon already running (PID {pid})");
        return Ok(());
    }

    let self_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let log_path = LukanPaths::daemon_log_file();

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .context("Failed to create daemon log file")?;

    let mut cmd = std::process::Command::new(&self_exe);
    cmd.args(["daemon", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file);
    if local_only {
        cmd.env("LUKAN_LOCAL_ONLY", "1");
    }
    let child = cmd.spawn().context("Failed to spawn daemon process")?;

    println!("  Daemon started (PID {})", child.id());
    println!("  Log: {}", log_path.display());

    Ok(())
}

/// Stop the running daemon by sending SIGTERM.
pub fn stop_daemon() -> Result<()> {
    let pid = match read_pid_file() {
        Some(pid) => pid,
        None => {
            println!("  Daemon is not running");
            return Ok(());
        }
    };

    if !is_process_alive(pid) {
        cleanup_lock_files();
        println!("  Daemon is not running (cleaned stale lock files)");
        return Ok(());
    }

    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        bail!("Daemon stop is only supported on Unix systems. Kill PID {pid} manually.");
    }

    // Wait a moment for graceful shutdown
    std::thread::sleep(Duration::from_millis(500));

    if is_process_alive(pid) {
        println!("  Sent SIGTERM to daemon (PID {pid}), waiting for shutdown...");
    } else {
        cleanup_lock_files();
        println!("  Daemon stopped (PID {pid})");
    }

    Ok(())
}

/// Restart the daemon (start detached after stop).
pub fn restart_daemon() -> Result<()> {
    let local_only = std::env::var("LUKAN_LOCAL_ONLY").is_ok_and(|v| v == "1")
        || std::fs::read_to_string(LukanPaths::config_file())
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("localOnly").and_then(|v| v.as_bool()))
            .unwrap_or(false);
    start_detached_with_opts(local_only)
}

/// Ensure the daemon is running; start it detached if not.
/// Returns the port the daemon's web server is listening on.
/// Called automatically from UIs and CLI worker commands.
pub fn ensure_daemon_running() -> Result<u16> {
    let want_local = std::env::var("LUKAN_LOCAL_ONLY").is_ok_and(|v| v == "1")
        || std::fs::read_to_string(LukanPaths::config_file())
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("localOnly").and_then(|v| v.as_bool()))
            .unwrap_or(false);

    // Check lock file first (new format with port)
    if let Some(lock) = read_lock_file() {
        if is_process_alive(lock.pid) {
            // Restart if local_only mode changed
            if lock.local_only != want_local {
                info!("Daemon bind mode changed, restarting");
                let _ = stop_daemon();
            } else {
                return Ok(lock.port);
            }
        } else {
            // Stale lock file
            cleanup_lock_files();
        }
    }

    // Check legacy PID file
    if let Some(pid) = read_pid_file() {
        if is_process_alive(pid) {
            // Legacy daemon without port info — return default
            warn!("Daemon running with legacy PID file (no port info), assuming port 3000");
            return Ok(3000);
        }
        cleanup_lock_files();
    }

    // Not running — spawn it
    let self_exe = std::env::current_exe()
        .context("Cannot auto-start daemon: failed to get executable path")?;
    let log_path = LukanPaths::daemon_log_file();

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .context("Cannot auto-start daemon: failed to create log file")?;

    let log_clone = log_file
        .try_clone()
        .context("Cannot auto-start daemon: failed to clone log file handle")?;

    let mut cmd = std::process::Command::new(&self_exe);
    cmd.args(["daemon", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(log_clone)
        .stderr(log_file);
    if want_local {
        cmd.env("LUKAN_LOCAL_ONLY", "1");
    }
    let child = cmd.spawn().context("Failed to auto-start daemon")?;

    info!(pid = child.id(), "Auto-started daemon");

    // Wait for the lock file to appear (up to 10s)
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > deadline {
            // Timeout — daemon may still be starting, return default port
            warn!("Daemon lock file not found after 10s, assuming port 3000");
            return Ok(3000);
        }
        std::thread::sleep(Duration::from_millis(100));
        if let Some(lock) = read_lock_file()
            && is_process_alive(lock.pid)
        {
            info!(pid = lock.pid, port = lock.port, "Daemon is ready");
            return Ok(lock.port);
        }
    }
}

/// Check if the daemon is alive by reading lock/PID files and checking the process.
pub fn is_daemon_running() -> bool {
    if let Some(lock) = read_lock_file() {
        return is_process_alive(lock.pid);
    }
    match read_pid_file() {
        Some(pid) => is_process_alive(pid),
        None => false,
    }
}

/// Read the new JSON lock file.
fn read_lock_file() -> Option<DaemonLock> {
    let path = LukanPaths::daemon_lock_file();
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read the legacy PID file.
fn read_pid_file() -> Option<u32> {
    let path = LukanPaths::daemon_pid_file();
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Remove both lock and PID files.
fn cleanup_lock_files() {
    let _ = std::fs::remove_file(LukanPaths::daemon_lock_file());
    let _ = std::fs::remove_file(LukanPaths::daemon_pid_file());
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}
