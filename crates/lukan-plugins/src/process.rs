use std::process::Stdio;

use anyhow::{Context, Result};
use lukan_core::config::LukanPaths;
use lukan_core::models::plugin::{HostMessage, PluginManifest, PluginMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// A running plugin child process.
///
/// Communicates with the plugin via JSON lines over stdin/stdout.
/// stderr is redirected to the plugin's log file.
pub struct PluginProcess {
    name: String,
    manifest: PluginManifest,
    child: Option<Child>,
}

impl PluginProcess {
    pub fn new(name: String, manifest: PluginManifest) -> Self {
        Self {
            name,
            manifest,
            child: None,
        }
    }

    /// Spawn the plugin process based on its manifest run config.
    pub async fn spawn(&mut self) -> Result<()> {
        let run = self
            .manifest
            .run
            .as_ref()
            .context("Plugin has no [run] config — cannot spawn a process")?;

        // Open log file for stderr
        let log_path = LukanPaths::plugin_log(&self.name);
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

        let plugin_dir = LukanPaths::plugin_dir(&self.name);

        let mut cmd = Command::new(&run.command);
        cmd.args(&run.args)
            .current_dir(&plugin_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_file))
            .kill_on_drop(true);

        // Inject env vars from manifest
        for (k, v) in &run.env {
            cmd.env(k, v);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn plugin '{}': {} {:?}", self.name, run.command, run.args))?;

        // Save PID
        if let Some(pid) = child.id() {
            let pid_path = LukanPaths::plugin_pid(&self.name);
            tokio::fs::write(&pid_path, pid.to_string()).await.ok();
        }

        info!(plugin = %self.name, "Plugin process spawned");
        self.child = Some(child);
        Ok(())
    }

    /// Send a HostMessage to the plugin via stdin (JSON line).
    pub async fn send(&mut self, msg: &HostMessage) -> Result<()> {
        let child = self.child.as_mut().context("Plugin process not running")?;
        let stdin = child.stdin.as_mut().context("Plugin stdin not available")?;
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Start the I/O loop: reads stdout lines -> plugin_tx, reads host_rx -> writes to stdin.
    ///
    /// Returns channels for communicating with the plugin:
    /// - `plugin_rx`: receives PluginMessages from the plugin
    /// - `host_tx`: sends HostMessages to the plugin
    pub fn run_io_loop(&mut self) -> Result<(mpsc::Receiver<PluginMessage>, mpsc::Sender<HostMessage>)> {
        let child = self.child.as_mut().context("Plugin process not running")?;

        let stdout = child.stdout.take().context("Plugin stdout not available")?;
        let stdin = child.stdin.take().context("Plugin stdin not available")?;

        let (plugin_tx, plugin_rx) = mpsc::channel::<PluginMessage>(256);
        let (host_tx, mut host_rx) = mpsc::channel::<HostMessage>(256);

        let name = self.name.clone();

        // Stdout reader task: reads JSON lines from plugin stdout
        let name_reader = name.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<PluginMessage>(&line) {
                    Ok(msg) => {
                        if plugin_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(plugin = %name_reader, line = %line, "Failed to parse plugin message: {e}");
                    }
                }
            }
            info!(plugin = %name_reader, "Plugin stdout reader ended");
        });

        // Stdin writer task: writes HostMessages as JSON lines to plugin stdin
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(msg) = host_rx.recv().await {
                match serde_json::to_string(&msg) {
                    Ok(json) => {
                        let line = format!("{json}\n");
                        if let Err(e) = stdin.write_all(line.as_bytes()).await {
                            error!(plugin = %name, "Failed to write to plugin stdin: {e}");
                            break;
                        }
                        let _ = stdin.flush().await;
                    }
                    Err(e) => {
                        error!(plugin = %name, "Failed to serialize host message: {e}");
                    }
                }
            }
        });

        Ok((plugin_rx, host_tx))
    }

    /// Graceful shutdown: send Shutdown, wait up to 5s, then SIGTERM/SIGKILL.
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            // Try to send Shutdown via the child's stdin if still available
            if let Some(ref mut stdin) = child.stdin {
                let msg = serde_json::to_string(&HostMessage::Shutdown).unwrap_or_default();
                let line = format!("{msg}\n");
                let _ = stdin.write_all(line.as_bytes()).await;
                let _ = stdin.flush().await;
            }

            // Wait up to 5 seconds for graceful exit
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) => {
                    info!(plugin = %self.name, ?status, "Plugin exited gracefully");
                }
                _ => {
                    warn!(plugin = %self.name, "Plugin did not exit in time, killing");
                    let _ = child.kill().await;
                }
            }
        }

        // Clean up PID file
        let pid_path = LukanPaths::plugin_pid(&self.name);
        let _ = tokio::fs::remove_file(&pid_path).await;

        self.child = None;
        Ok(())
    }

    /// Run the plugin with auto-restart on crash.
    pub async fn run_with_restart(
        &mut self,
        auto_restart: bool,
        max_restarts: u32,
        delay_secs: u64,
    ) -> Result<()> {
        let mut restart_count = 0u32;

        loop {
            self.spawn().await?;

            if let Some(ref mut child) = self.child {
                match child.wait().await {
                    Ok(status) => {
                        info!(plugin = %self.name, ?status, "Plugin process exited");
                    }
                    Err(e) => {
                        error!(plugin = %self.name, "Plugin process error: {e}");
                    }
                }
            }

            if !auto_restart || restart_count >= max_restarts {
                break;
            }

            restart_count += 1;
            warn!(
                plugin = %self.name,
                restart_count,
                "Restarting plugin in {delay_secs}s..."
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
        }

        Ok(())
    }

    /// Check if the child process is still running.
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            child.try_wait().ok().flatten().is_none()
        } else {
            false
        }
    }
}
