use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use tokio::io::AsyncWriteExt;
use tokio::signal;
use tracing::{error, info};

use lukan_agent::WorkerScheduler;
use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig};

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
            if detach {
                start_detached()?;
            } else {
                run_daemon().await?;
            }
        }
        DaemonCommands::Stop => {
            stop_daemon()?;
        }
        DaemonCommands::Status => {
            if is_daemon_running() {
                let pid = read_pid_file().unwrap_or(0);
                println!("  Worker daemon is running (PID {pid})");
            } else {
                println!("  Worker daemon is not running");
            }
        }
    }
    Ok(())
}

/// Run the daemon in the current process (foreground).
async fn run_daemon() -> Result<()> {
    if is_daemon_running() {
        let pid = read_pid_file().unwrap_or(0);
        bail!("Worker daemon already running (PID {pid})");
    }

    // Write PID file
    let pid = std::process::id();
    let pid_path = LukanPaths::daemon_pid_file();
    std::fs::write(&pid_path, pid.to_string()).context("Failed to write daemon PID file")?;

    info!(pid, "Worker daemon starting");

    // Load config
    let config = ConfigManager::load().await?;
    let credentials = CredentialsManager::load().await?;
    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    // Create and start scheduler
    let scheduler = WorkerScheduler::new(resolved);
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

    // Start relay bridge if relay credentials exist
    let mut relay_bridge = None;
    if let Some(relay_config) = lukan_core::relay::RelayConfig::load().await {
        let relay_port = std::env::var("LUKAN_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000u16);
        info!(
            relay_url = %relay_config.relay_url,
            "Starting relay bridge"
        );
        let mut bridge = crate::relay_bridge::RelayBridge::new(relay_config, relay_port);
        bridge.start();
        relay_bridge = Some(bridge);
    }

    info!("Worker daemon running, polling workers.json for changes");

    // Track file mtime for change detection
    let workers_file = LukanPaths::workers_file();
    let mut last_mtime = file_mtime(&workers_file);

    let mut poll_interval = tokio::time::interval(Duration::from_secs(3));
    poll_interval.tick().await; // skip first immediate tick

    let shutdown = async {
        // Wait for either SIGINT or SIGTERM
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
                info!("Worker daemon received shutdown signal");
                break;
            }
            _ = poll_interval.tick() => {
                let current_mtime = file_mtime(&workers_file);
                if current_mtime != last_mtime {
                    info!("workers.json changed, reloading");
                    scheduler.reload().await;
                    last_mtime = current_mtime;
                }
            }
        }
    }

    // Cleanup
    if let Some(mut bridge) = relay_bridge {
        bridge.stop();
    }
    scheduler.stop();
    let _ = std::fs::remove_file(&pid_path);
    info!("Worker daemon stopped");

    Ok(())
}

/// Spawn the daemon as a detached background process.
fn start_detached() -> Result<()> {
    if is_daemon_running() {
        let pid = read_pid_file().unwrap_or(0);
        println!("  Worker daemon already running (PID {pid})");
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

    let child = std::process::Command::new(&self_exe)
        .args(["daemon", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .spawn()
        .context("Failed to spawn daemon process")?;

    println!("  Worker daemon started (PID {})", child.id());
    println!("  Log: {}", log_path.display());

    Ok(())
}

/// Stop the running daemon by sending SIGTERM.
pub fn stop_daemon() -> Result<()> {
    let pid = match read_pid_file() {
        Some(pid) => pid,
        None => {
            println!("  Worker daemon is not running");
            return Ok(());
        }
    };

    if !is_process_alive(pid) {
        // Stale PID file
        let _ = std::fs::remove_file(LukanPaths::daemon_pid_file());
        println!("  Worker daemon is not running (cleaned stale PID file)");
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
        let _ = std::fs::remove_file(LukanPaths::daemon_pid_file());
        println!("  Worker daemon stopped (PID {pid})");
    }

    Ok(())
}

/// Ensure the daemon is running; start it detached if not.
/// Called automatically from UIs and CLI worker commands.
pub fn ensure_daemon_running() {
    if is_daemon_running() {
        return;
    }

    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "Cannot auto-start daemon: failed to get executable path");
            return;
        }
    };

    let log_path = LukanPaths::daemon_log_file();

    let log_file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, "Cannot auto-start daemon: failed to create log file");
            return;
        }
    };

    let log_clone = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, "Cannot auto-start daemon: failed to clone log file handle");
            return;
        }
    };

    match std::process::Command::new(&self_exe)
        .args(["daemon", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(log_clone)
        .stderr(log_file)
        .spawn()
    {
        Ok(child) => {
            info!(pid = child.id(), "Auto-started worker daemon");
        }
        Err(e) => {
            error!(error = %e, "Failed to auto-start worker daemon");
        }
    }
}

/// Check if the daemon is alive by reading the PID file and checking the process.
pub fn is_daemon_running() -> bool {
    match read_pid_file() {
        Some(pid) => is_process_alive(pid),
        None => false,
    }
}

fn read_pid_file() -> Option<u32> {
    let path = LukanPaths::daemon_pid_file();
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks if process exists without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // On non-Unix, assume alive if PID file exists
        let _ = pid;
        true
    }
}

fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}
