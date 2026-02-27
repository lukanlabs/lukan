use std::path::Path;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::debug;

use crate::bg_processes;
use crate::{Tool, ToolContext};

/// Cached result of setsid availability check.
static SETSID_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Check if `setsid` is available on this system.
/// Used as a fallback when bwrap is not available, to detach the child process
/// from the controlling terminal (prevents SSH/git from stealing tty input).
fn is_setsid_available() -> bool {
    *SETSID_AVAILABLE.get_or_init(|| Path::new("/usr/bin/setsid").exists())
}

const MAX_OUTPUT_BYTES: usize = 30_000;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command. Use for system commands, git operations, and terminal tasks. \
         Set background=true to run in background (returns PID). \
         Use wait_pid to wait for a background process to finish."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)",
                    "default": 120000
                },
                "background": {
                    "type": "boolean",
                    "description": "Run the command in background. Returns PID immediately.",
                    "default": false
                },
                "wait_pid": {
                    "type": "integer",
                    "description": "Wait for a background process (by PID) to finish and return its output"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        // Handle wait_pid mode
        if let Some(wait_pid) = input.get("wait_pid").and_then(|v| v.as_u64()) {
            let pid = wait_pid as u32;
            let timeout_ms = input
                .get("timeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TIMEOUT_MS);

            debug!(pid, timeout_ms, "Waiting for background process");

            if !bg_processes::is_process_alive(pid) {
                // Already dead — return log
                return match bg_processes::get_bg_log(pid, usize::MAX) {
                    Some(log) => Ok(ToolResult::success(format!(
                        "Process {pid} already finished.\n{log}"
                    ))),
                    None => Ok(ToolResult::success(format!(
                        "Process {pid} already finished. No log available."
                    ))),
                };
            }

            return match bg_processes::wait_bg_process(pid, timeout_ms).await {
                Some(log) => Ok(ToolResult::success(format!(
                    "Process {pid} finished.\n{log}"
                ))),
                None => Ok(ToolResult::error(format!(
                    "Timed out waiting for process {pid} after {timeout_ms}ms. \
                     Process is still running."
                ))),
            };
        }

        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: command"))?;

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_MS);

        let background = input
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Background mode: spawn and return PID immediately
        if background {
            return self.execute_background(command, ctx).await;
        }

        debug!(command, timeout_ms, "Executing bash command");

        // Determine if we should use bwrap sandbox
        let use_bwrap = ctx
            .sandbox
            .as_ref()
            .map(|s| s.enabled && crate::sandbox::is_bwrap_available())
            .unwrap_or(false);

        // Foreground mode: spawn with piped stdout/stderr
        let mut child = if use_bwrap {
            let sandbox = ctx.sandbox.as_ref().unwrap();
            let bwrap_args = crate::sandbox::build_bwrap_args(&crate::sandbox::BwrapConfig {
                allowed_dirs: sandbox.allowed_dirs.clone(),
                sensitive_patterns: sandbox.sensitive_patterns.clone(),
                cwd: ctx.cwd.to_string_lossy().to_string(),
            });
            // bwrap_args[0] is "bwrap", rest are args, then append "-- bash -c <command>"
            Command::new(&bwrap_args[0])
                .args(&bwrap_args[1..])
                .arg("--")
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else if is_setsid_available() {
            // Use setsid to create a new session -- detaches child from our
            // controlling terminal so SSH/git can't open /dev/tty to steal input.
            Command::new("setsid")
                .arg("--wait")
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else {
            Command::new("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        };

        let child_pid = child.id().unwrap_or(0);

        // If we have a bg_signal, race the command against Alt+B.
        // We MUST drain stdout/stderr concurrently to avoid deadlock:
        // if the process fills the OS pipe buffer (~64KB), it blocks on write
        // while we block on wait() → deadlock.
        if let Some(bg_signal) = &ctx.bg_signal {
            let mut bg_signal = bg_signal.clone();
            let command_str = command.to_string();

            // Mark the initial value as seen so changed() only fires on new sends
            bg_signal.mark_unchanged();

            // Take pipes out of child — we'll drain them in spawned tasks
            let stdout_pipe = child.stdout.take();
            let stderr_pipe = child.stderr.take();

            // Shared buffers: drain tasks write here continuously.
            // Also write to log file so /bg can show live output.
            let log_path = bg_processes::log_file_path(child_pid);
            let drainer = OutputDrainer::start(child_pid, stdout_pipe, stderr_pipe, &log_path);

            // Race: child.wait() vs Alt+B
            enum RaceResult {
                Finished(std::io::Result<std::process::ExitStatus>),
                Timeout,
                Background,
            }

            let timeout_dur = std::time::Duration::from_millis(timeout_ms);
            let race = tokio::select! {
                res = tokio::time::timeout(timeout_dur, child.wait()) => {
                    match res {
                        Ok(status) => RaceResult::Finished(status),
                        Err(_) => RaceResult::Timeout,
                    }
                }
                _ = bg_signal.changed() => RaceResult::Background,
            };

            return match race {
                RaceResult::Finished(Ok(status)) => {
                    // Command finished — give drain tasks a moment to flush
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    let (stdout_data, stderr_data) = drainer.collect().await;
                    // Clean up the temp log file (not needed for foreground)
                    let _ = tokio::fs::remove_file(&log_path).await;
                    build_foreground_result_from_parts(
                        &stdout_data,
                        &stderr_data,
                        status.code().unwrap_or(-1),
                    )
                }
                RaceResult::Finished(Err(e)) => {
                    let _ = tokio::fs::remove_file(&log_path).await;
                    Ok(ToolResult::error(format!("Failed to execute command: {e}")))
                }
                RaceResult::Timeout => {
                    let _ = child.kill().await;
                    let _ = tokio::fs::remove_file(&log_path).await;
                    Ok(ToolResult::error(format!(
                        "Command timed out after {timeout_ms}ms"
                    )))
                }
                RaceResult::Background => {
                    if child_pid > 0 {
                        // Drain tasks are already running and writing to the log
                        // file — they'll continue until the process exits.
                        // Just register the process and spawn a reaper.
                        tokio::spawn(async move {
                            let _ = child.wait().await;
                        });

                        let log_display = log_path.display().to_string();
                        bg_processes::add_bg_process(child_pid, command_str, log_path);

                        Ok(ToolResult::success(format!(
                            "The user pressed Alt+B to send this command to background. \
                             The process is still running — do NOT kill or restart it.\n\
                             PID: {child_pid}\n\
                             Log file: {log_display}\n\
                             To check output later: ReadFiles(\"{log_display}\")\n\
                             To wait for completion: Bash({{ wait_pid: {child_pid} }})\n\
                             To stop (only if user asks): Bash({{ command: \"kill {child_pid}\" }})"
                        )))
                    } else {
                        Ok(ToolResult::error(
                            "Failed to get process PID for backgrounding.",
                        ))
                    }
                }
            };
        }

        // No bg_signal — simple foreground with timeout (wait_with_output
        // drains pipes internally, so no deadlock risk)
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            child.wait_with_output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => build_foreground_result(output),
            Ok(Err(e)) => Ok(ToolResult::error(format!("Failed to execute command: {e}"))),
            Err(_) => Ok(ToolResult::error(format!(
                "Command timed out after {timeout_ms}ms"
            ))),
        }
    }
}

impl BashTool {
    /// Spawn a command in background, drain output to a log file, return PID
    async fn execute_background(
        &self,
        command: &str,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        debug!(command, "Spawning background command");

        // Determine if we should use bwrap sandbox
        let use_bwrap = ctx
            .sandbox
            .as_ref()
            .map(|s| s.enabled && crate::sandbox::is_bwrap_available())
            .unwrap_or(false);

        let mut child = if use_bwrap {
            let sandbox = ctx.sandbox.as_ref().unwrap();
            let bwrap_args = crate::sandbox::build_bwrap_args(&crate::sandbox::BwrapConfig {
                allowed_dirs: sandbox.allowed_dirs.clone(),
                sensitive_patterns: sandbox.sensitive_patterns.clone(),
                cwd: ctx.cwd.to_string_lossy().to_string(),
            });
            Command::new(&bwrap_args[0])
                .args(&bwrap_args[1..])
                .arg("--")
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else if is_setsid_available() {
            Command::new("setsid")
                .arg("--wait")
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else {
            Command::new("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .current_dir(&ctx.cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        };

        let pid = match child.id() {
            Some(pid) => pid,
            None => {
                return Ok(ToolResult::error("Failed to get PID of spawned process."));
            }
        };

        let log_file = bg_processes::log_file_path(pid);

        // Drain stdout/stderr to log file in background tasks
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        OutputDrainer::start(pid, stdout, stderr, &log_file);

        // Spawn a task to wait for the child (so it doesn't become a zombie)
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        let log_display = log_file.display().to_string();

        bg_processes::add_bg_process(pid, command.to_string(), log_file);

        Ok(ToolResult::success(format!(
            "Background process started. PID: {pid}\n\
             Log file: {log_display}\n\
             To check output: ReadFiles(\"{log_display}\")\n\
             To wait for completion: Bash({{ wait_pid: {pid} }})\n\
             To stop: Bash({{ command: \"kill {pid}\" }})"
        )))
    }
}

// ── Output drainer ────────────────────────────────────────────────────────

/// Drains stdout/stderr from a child process concurrently into:
/// - Shared in-memory buffers (for foreground return value)
/// - A log file on disk (for /bg live viewing and background processes)
///
/// This prevents the OS pipe buffer (~64KB) from filling up and deadlocking
/// the child process.
struct OutputDrainer {
    stdout_buf: Arc<Mutex<Vec<u8>>>,
    stderr_buf: Arc<Mutex<Vec<u8>>>,
}

impl OutputDrainer {
    /// Start draining. Returns immediately; drain happens in spawned tasks.
    fn start(
        pid: u32,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
        log_path: &std::path::Path,
    ) -> Self {
        let stdout_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::<u8>::new()));

        // Shared log file handle
        let log_file: Arc<Mutex<Option<tokio::fs::File>>> = Arc::new(Mutex::new(None));
        let log_ready = Arc::new(tokio::sync::Notify::new());

        // Create log file
        {
            let log_file = Arc::clone(&log_file);
            let log_ready = Arc::clone(&log_ready);
            let log_path = log_path.to_path_buf();
            tokio::spawn(async move {
                match tokio::fs::File::create(&log_path).await {
                    Ok(f) => *log_file.lock().await = Some(f),
                    Err(e) => tracing::warn!(pid, error = %e, "Failed to create bg log file"),
                }
                log_ready.notify_waiters();
            });
        }

        // Drain stdout
        if let Some(stdout) = stdout {
            let buf = Arc::clone(&stdout_buf);
            let log = Arc::clone(&log_file);
            let ready = Arc::clone(&log_ready);
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                ready.notified().await;
                let reader = tokio::io::BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    // Append to in-memory buffer
                    {
                        let mut b = buf.lock().await;
                        b.extend_from_slice(line.as_bytes());
                        b.push(b'\n');
                    }
                    // Append to log file
                    let mut guard = log.lock().await;
                    if let Some(f) = guard.as_mut() {
                        let _ = f.write_all(line.as_bytes()).await;
                        let _ = f.write_all(b"\n").await;
                        let _ = f.flush().await;
                    }
                }
            });
        }

        // Drain stderr
        if let Some(stderr) = stderr {
            let buf = Arc::clone(&stderr_buf);
            let log = Arc::clone(&log_file);
            let ready = Arc::clone(&log_ready);
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                ready.notified().await;
                let reader = tokio::io::BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    // Append to in-memory buffer
                    {
                        let mut b = buf.lock().await;
                        b.extend_from_slice(line.as_bytes());
                        b.push(b'\n');
                    }
                    // Append to log file
                    let mut guard = log.lock().await;
                    if let Some(f) = guard.as_mut() {
                        let _ = f.write_all(line.as_bytes()).await;
                        let _ = f.write_all(b"\n").await;
                        let _ = f.flush().await;
                    }
                }
            });
        }

        Self {
            stdout_buf,
            stderr_buf,
        }
    }

    /// Collect the buffered stdout and stderr data.
    async fn collect(self) -> (Vec<u8>, Vec<u8>) {
        let stdout = std::mem::take(&mut *self.stdout_buf.lock().await);
        let stderr = std::mem::take(&mut *self.stderr_buf.lock().await);
        (stdout, stderr)
    }
}

// ── Result builders ───────────────────────────────────────────────────────

/// Build a ToolResult from buffered stdout/stderr and exit code
fn build_foreground_result_from_parts(
    stdout_bytes: &[u8],
    stderr_bytes: &[u8],
    exit_code: i32,
) -> anyhow::Result<ToolResult> {
    let mut combined = String::new();

    let stdout = String::from_utf8_lossy(stdout_bytes);
    let stderr = String::from_utf8_lossy(stderr_bytes);

    if !stdout.is_empty() {
        combined.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }

    if combined.len() > MAX_OUTPUT_BYTES {
        combined.truncate(MAX_OUTPUT_BYTES);
        combined.push_str("\n... (output truncated)");
    }

    let content = if combined.is_empty() {
        format!("(exit code: {exit_code})")
    } else {
        format!("{combined}\n(exit code: {exit_code})")
    };

    if exit_code == 0 {
        Ok(ToolResult::success(content))
    } else {
        Ok(ToolResult::error(content))
    }
}

/// Build a ToolResult from a completed foreground process
fn build_foreground_result(output: std::process::Output) -> anyhow::Result<ToolResult> {
    build_foreground_result_from_parts(
        &output.stdout,
        &output.stderr,
        output.status.code().unwrap_or(-1),
    )
}
