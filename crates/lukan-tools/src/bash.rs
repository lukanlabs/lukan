use std::sync::Arc;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use tokio::process::Command;
use tracing::debug;

use crate::bg_processes;
use crate::{Tool, ToolContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashCommandClass {
    Read,
    Search,
    List,
    Network,
    Mutating,
    Destructive,
    Unknown,
}

impl BashCommandClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Search => "search",
            Self::List => "list",
            Self::Network => "network",
            Self::Mutating => "mutating",
            Self::Destructive => "destructive",
            Self::Unknown => "unknown",
        }
    }
}

pub fn classify_bash_command(command: &str) -> BashCommandClass {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return BashCommandClass::Unknown;
    }

    let lower = trimmed.to_ascii_lowercase();

    let destructive_fragments = [
        "rm -rf",
        "rm -fr",
        "mkfs",
        "dd if=",
        "shutdown",
        "reboot",
        "poweroff",
        "git reset --hard",
        "git clean -fd",
        "git clean -xdf",
    ];
    if destructive_fragments.iter().any(|frag| lower.contains(frag)) {
        return BashCommandClass::Destructive;
    }

    let mutating_fragments = [
        ">",
        ">>",
        "touch ",
        "mkdir ",
        "rmdir ",
        "mv ",
        "cp ",
        "sed -i",
        "perl -pi",
        "git add",
        "git commit",
        "git stash",
        "git apply",
        "npm install",
        "bun install",
        "pnpm install",
        "yarn install",
        "cargo build",
        "cargo test",
        "make ",
        "python -c",
        "python3 -c",
    ];
    if mutating_fragments.iter().any(|frag| lower.contains(frag)) {
        return BashCommandClass::Mutating;
    }

    let first = lower.split_whitespace().next().unwrap_or_default();
    match first {
        "ls" | "tree" | "du" | "pwd" => BashCommandClass::List,
        "find" | "grep" | "rg" | "fd" | "which" | "whereis" => BashCommandClass::Search,
        "cat" | "head" | "tail" | "less" | "more" | "stat" | "file" | "git" => {
            if lower.starts_with("git status")
                || lower.starts_with("git diff")
                || lower.starts_with("git log")
                || lower.starts_with("git show")
                || lower == "git branch"
                || lower.starts_with("git branch --show-current")
            {
                BashCommandClass::Read
            } else {
                BashCommandClass::Unknown
            }
        }
        "curl" | "wget" | "ping" | "nslookup" | "dig" | "ssh" | "scp" => {
            BashCommandClass::Network
        }
        _ => BashCommandClass::Unknown,
    }
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
            "required": []
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("run shell commands and terminal tasks; read/search/list commands are lower risk than mutating or destructive ones")
    }

    fn activity_label(&self, input: &serde_json::Value) -> Option<String> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|cmd| cmd.trim())
            .filter(|cmd| !cmd.is_empty());
        let class = command.map(classify_bash_command);
        match (command, class) {
            (Some(cmd), Some(class)) => Some(format!("[{}] {cmd}", class.as_str())),
            _ => Some("Running command".to_string()),
        }
    }

    fn validate_input(&self, input: &serde_json::Value, _ctx: &ToolContext) -> Result<(), String> {
        if let Some(wait_pid) = input.get("wait_pid") {
            if wait_pid.as_u64().is_none() {
                return Err("wait_pid must be an integer PID.".to_string());
            }
            return Ok(());
        }

        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing required field: command".to_string())?;

        if command.trim().is_empty() {
            return Err("Command is empty. Provide a shell command to execute.".to_string());
        }

        Ok(())
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

        // Determine if we should use OS-level sandbox (bwrap on Linux, sandbox-exec on macOS)
        let use_sandbox = ctx
            .sandbox
            .as_ref()
            .map(|s| s.enabled && crate::sandbox::is_sandbox_available())
            .unwrap_or(false);

        // Foreground mode: spawn with piped stdout/stderr
        let mut child = if use_sandbox {
            let sandbox = ctx.sandbox.as_ref().unwrap();
            let sandbox_args = crate::sandbox::build_sandbox_args(&crate::sandbox::BwrapConfig {
                allowed_dirs: sandbox.allowed_dirs.clone(),
                sensitive_patterns: sandbox.sensitive_patterns.clone(),
                cwd: ctx.cwd.to_string_lossy().to_string(),
            });
            Command::new(&sandbox_args[0])
                .args(&sandbox_args[1..])
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .envs(&ctx.extra_env)
                .current_dir(&ctx.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else {
            // Create a new session via pre_exec(setsid) so the child bash
            // becomes its own session leader AND process group leader (PGID =
            // child PID). This means:
            // 1. Detached from our controlling terminal (SSH/git can't steal tty)
            // 2. kill(-child_pid, sig) correctly targets the entire process tree
            let mut cmd = Command::new("bash");
            cmd.arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .envs(&ctx.extra_env)
                .current_dir(&ctx.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            cmd.spawn()?
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
            let drainer =
                OutputDrainer::start(child_pid, stdout_pipe, stderr_pipe, &log_path, true);

            // Race: child.wait() vs Alt+B vs cancellation
            enum RaceResult {
                Finished(std::io::Result<std::process::ExitStatus>),
                Timeout,
                Background,
                Cancelled,
            }

            let timeout_dur = std::time::Duration::from_millis(timeout_ms);
            let cancel_token = ctx.cancel.clone();
            let race = tokio::select! {
                res = tokio::time::timeout(timeout_dur, child.wait()) => {
                    match res {
                        Ok(status) => RaceResult::Finished(status),
                        Err(_) => RaceResult::Timeout,
                    }
                }
                _ = bg_signal.changed() => RaceResult::Background,
                _ = async {
                    match &cancel_token {
                        Some(t) => t.cancelled().await,
                        None => std::future::pending().await,
                    }
                } => RaceResult::Cancelled,
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
                RaceResult::Cancelled => {
                    if child_pid > 0 {
                        bg_processes::kill_process_group_force(child_pid).await;
                    }
                    let _ = tokio::fs::remove_file(&log_path).await;
                    Ok(ToolResult::error("Cancelled by user."))
                }
                RaceResult::Background => {
                    if child_pid > 0 {
                        // Stop accumulating in memory — the drain tasks will
                        // continue writing to the log file only.
                        drainer.stop_buffering();

                        // Drain tasks are already running and writing to the log
                        // file — they'll continue until the process exits.
                        // Just register the process and spawn a reaper.
                        tokio::spawn(async move {
                            let _ = child.wait().await;
                        });

                        let log_display = log_path.display().to_string();
                        bg_processes::add_bg_process(
                            child_pid,
                            command_str,
                            log_path,
                            ctx.session_id.clone(),
                            ctx.agent_label.clone(),
                            ctx.tab_id.clone(),
                        );

                        Ok(ToolResult::success(format!(
                            "The user pressed Alt+B to send this command to background. \
                             The process is still running — do NOT kill or restart it. \
                             IMPORTANT: do NOT automatically call Bash({{ wait_pid: {child_pid} }}) and do NOT keep following this background job unless the user explicitly asks you to check it later. \
                             Stop here so the chat stays free for other work.\n\
                             PID: {child_pid}\n\
                             Log file: {log_display}\n\
                             If the user later asks to inspect output, you may use ReadFiles(\"{log_display}\").\n\
                             If the user later explicitly asks you to wait for completion, you may use Bash({{ wait_pid: {child_pid} }}).\n\
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

        // No bg_signal — foreground with timeout + cancellation.
        // Still use OutputDrainer + log file so subagent processes have
        // visible output in the /bg picker.
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let log_path = bg_processes::log_file_path(child_pid);
        let drainer = OutputDrainer::start(child_pid, stdout_pipe, stderr_pipe, &log_path, true);

        // Register as bg process so it's visible in /bg while running
        if child_pid > 0 {
            bg_processes::add_bg_process(
                child_pid,
                command.to_string(),
                log_path.clone(),
                ctx.session_id.clone(),
                ctx.agent_label.clone(),
                ctx.tab_id.clone(),
            );
        }

        let cancel_token = ctx.cancel.clone();
        let timeout_dur = std::time::Duration::from_millis(timeout_ms);
        let race = tokio::select! {
            res = tokio::time::timeout(timeout_dur, child.wait()) => {
                res.ok()
            }
            _ = async {
                match &cancel_token {
                    Some(t) => t.cancelled().await,
                    None => std::future::pending().await,
                }
            } => {
                if child_pid > 0 {
                    bg_processes::kill_process_group_force(child_pid).await;
                }
                bg_processes::remove_bg_process(child_pid);
                let _ = tokio::fs::remove_file(&log_path).await;
                return Ok(ToolResult::error("Cancelled by user."));
            }
        };

        // Process completed or timed out — collect output
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (stdout_data, stderr_data) = drainer.collect().await;
        bg_processes::remove_bg_process(child_pid);
        let _ = tokio::fs::remove_file(&log_path).await;

        match race {
            Some(Ok(status)) => build_foreground_result_from_parts(
                &stdout_data,
                &stderr_data,
                status.code().unwrap_or(-1),
            ),
            Some(Err(e)) => Ok(ToolResult::error(format!("Failed to execute command: {e}"))),
            None => {
                if child_pid > 0 {
                    let _ = child.kill().await;
                }
                Ok(ToolResult::error(format!(
                    "Command timed out after {timeout_ms}ms"
                )))
            }
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

        // Determine if we should use OS-level sandbox (bwrap on Linux, sandbox-exec on macOS)
        let use_sandbox = ctx
            .sandbox
            .as_ref()
            .map(|s| s.enabled && crate::sandbox::is_sandbox_available())
            .unwrap_or(false);

        let mut child = if use_sandbox {
            let sandbox = ctx.sandbox.as_ref().unwrap();
            let sandbox_args = crate::sandbox::build_sandbox_args(&crate::sandbox::BwrapConfig {
                allowed_dirs: sandbox.allowed_dirs.clone(),
                sensitive_patterns: sandbox.sensitive_patterns.clone(),
                cwd: ctx.cwd.to_string_lossy().to_string(),
            });
            Command::new(&sandbox_args[0])
                .args(&sandbox_args[1..])
                .arg("bash")
                .arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .envs(&ctx.extra_env)
                .current_dir(&ctx.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        } else {
            let mut cmd = Command::new("bash");
            cmd.arg("-c")
                .arg(command)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LUKAN_AGENT", "1")
                .envs(&ctx.extra_env)
                .current_dir(&ctx.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            cmd.spawn()?
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
        OutputDrainer::start(pid, stdout, stderr, &log_file, false);

        // Spawn a task to wait for the child (so it doesn't become a zombie)
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        let log_display = log_file.display().to_string();

        bg_processes::add_bg_process(
            pid,
            command.to_string(),
            log_file,
            ctx.session_id.clone(),
            ctx.agent_label.clone(),
            ctx.tab_id.clone(),
        );

        if let (Some(event_tx), Some(tool_call_id)) = (ctx.event_tx.as_ref(), ctx.tool_call_id.as_ref()) {
            let event_tx = event_tx.clone();
            let _tool_call_id = tool_call_id.clone();
            let command = command.to_string();
            let tab_id = ctx.tab_id.clone();
            tokio::spawn(async move {
                loop {
                    if !bg_processes::is_process_alive(pid) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }

                let log = bg_processes::get_bg_log(pid, 200)
                    .unwrap_or_else(|| "No log available.".to_string());
                let compact_log = log.replace('\r', "").trim().to_string();
                let sanitized_command = command.replace('\n', " ").replace('\r', " ").trim().to_string();
                let summary = if compact_log.is_empty() {
                    format!(
                        "Background Bash process already completed. Do not call Bash again and do not call wait_pid. Continue using the agent normally with this final result.\nPID: {pid}\nCommand: {sanitized_command}\nFinal output: (no output captured)"
                    )
                } else {
                    format!(
                        "Background Bash process already completed. Do not call Bash again and do not call wait_pid. Continue using the agent normally with this final result.\nPID: {pid}\nCommand: {sanitized_command}\nFinal output:\n{compact_log}"
                    )
                };
                let display_summary = format!(
                    "Background Bash process completed. PID: {pid}."
                );
                let queue_payload = serde_json::json!({
                    "text": summary,
                    "display_text": display_summary,
                })
                .to_string();
                let _ = event_tx
                    .send(lukan_core::models::events::StreamEvent::QueuedMessageInjected {
                        text: summary.clone(),
                        display_text: Some(display_summary.clone()),
                    })
                    .await;
                if let Some(tab_id) = tab_id {
                    let _ = crate::bg_processes::enqueue_session_completion(&tab_id, queue_payload);
                }
            });
        }

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
/// Architecture: lightweight tokio tasks read chunks from the pipes and forward
/// them through a std::sync::mpsc channel to a dedicated OS writer thread.
/// This keeps the async runtime load minimal (just reads + channel sends)
/// while all file I/O happens off the runtime entirely.
struct OutputDrainer {
    stdout_buf: Arc<std::sync::Mutex<Vec<u8>>>,
    stderr_buf: Arc<std::sync::Mutex<Vec<u8>>>,
    /// Shared flag — set to `false` to stop buffering in memory (e.g. after Alt+B).
    buffer_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl OutputDrainer {
    fn start(
        pid: u32,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
        log_path: &std::path::Path,
        buffer_in_memory: bool,
    ) -> Self {
        let stdout_buf = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let stderr_buf = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let buffer_flag = Arc::new(std::sync::atomic::AtomicBool::new(buffer_in_memory));

        // Channel: async reader tasks → OS writer thread
        let (log_tx, log_rx) = std::sync::mpsc::channel::<Vec<u8>>();

        // OS writer thread — all file I/O happens here, completely off tokio.
        {
            let log_path = log_path.to_path_buf();
            std::thread::Builder::new()
                .name("log-writer".into())
                .spawn(move || {
                    use std::io::Write;
                    let file = match std::fs::File::create(&log_path) {
                        Ok(f) => f,
                        Err(e) => {
                            tracing::warn!(pid, error = %e, "Failed to create bg log file");
                            return;
                        }
                    };
                    let mut writer = std::io::BufWriter::new(file);
                    while let Ok(data) = log_rx.recv() {
                        let _ = writer.write_all(&data);
                        // Flush after each write so /bg can see live output
                        let _ = writer.flush();
                    }
                })
                .ok();
        }

        // Spawn a lightweight async task that reads chunks from a pipe and
        // forwards them to the writer thread via the mpsc channel.
        fn spawn_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
            reader: R,
            mem_buf: Arc<std::sync::Mutex<Vec<u8>>>,
            buffer_flag: Arc<std::sync::atomic::AtomicBool>,
            log_tx: std::sync::mpsc::Sender<Vec<u8>>,
        ) {
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut reader = reader;
                let mut chunk = vec![0u8; 8192];
                loop {
                    match reader.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = chunk[..n].to_vec();

                            if buffer_flag.load(std::sync::atomic::Ordering::Relaxed)
                                && let Ok(mut b) = mem_buf.lock()
                            {
                                b.extend_from_slice(&data);
                            }

                            // Non-blocking send to writer thread
                            if log_tx.send(data).is_err() {
                                break; // Writer thread gone
                            }

                            // Yield so the TUI event loop gets CPU time
                            tokio::task::yield_now().await;
                        }
                        Err(_) => break,
                    }
                }
                // Drop sender so writer thread flushes and exits
                drop(log_tx);
            });
        }

        if let Some(stdout) = stdout {
            spawn_reader(
                stdout,
                Arc::clone(&stdout_buf),
                Arc::clone(&buffer_flag),
                log_tx.clone(),
            );
        }

        if let Some(stderr) = stderr {
            spawn_reader(
                stderr,
                Arc::clone(&stderr_buf),
                Arc::clone(&buffer_flag),
                log_tx.clone(),
            );
        }

        // Drop our copy so writer thread exits when both reader tasks finish
        drop(log_tx);

        Self {
            stdout_buf,
            stderr_buf,
            buffer_flag,
        }
    }

    /// Stop accumulating output in memory (e.g. when command is sent to background).
    fn stop_buffering(&self) {
        self.buffer_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Collect the buffered stdout and stderr data.
    async fn collect(self) -> (Vec<u8>, Vec<u8>) {
        let stdout = std::mem::take(&mut *self.stdout_buf.lock().unwrap());
        let stderr = std::mem::take(&mut *self.stderr_buf.lock().unwrap());
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
