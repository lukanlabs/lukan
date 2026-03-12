use std::collections::HashMap;
use std::process::Stdio;

use anyhow::Context;
use base64::Engine;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, broadcast};

use crate::protocol::{ServerMessage, TerminalSessionInfoDto};

/// Prefix for all lukan-managed tmux sessions.
const TMUX_PREFIX: &str = "lukan-";

/// Build a `Command` for tmux isolated on the `lukan` socket with no config file.
///
/// Uses `setsid()` in `pre_exec` so tmux doesn't inherit the server's
/// controlling terminal, and pipes stderr/stdout to null to avoid blocking.
fn tmux_cmd() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["-L", "lukan", "-f", "/dev/null"]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

/// Like [`tmux_cmd`] but keeps stdout so we can read output.
fn tmux_cmd_with_output() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["-L", "lukan", "-f", "/dev/null"]);
    cmd.stdin(Stdio::null());
    cmd.stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

/// A tmux-backed terminal session.
struct TmuxSession {
    id: String,
    cols: u16,
    rows: u16,
    name: Option<String>,
    /// FIFO path used by `tmux pipe-pane` to stream output.
    fifo_path: String,
    /// Handle to the async task reading the FIFO.
    reader_handle: tokio::task::JoinHandle<()>,
}

/// Manages terminal sessions backed by tmux.
///
/// Each session maps 1:1 to a tmux session with a `lukan-` prefix.
/// Sessions survive WebSocket disconnects and server restarts because tmux
/// keeps the PTY alive independently.
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, TmuxSession>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl TerminalManager {
    /// Spawn a new tmux session. Output is streamed via a FIFO and broadcast
    /// as `ServerMessage::TerminalOutput` / `ServerMessage::TerminalExited`.
    pub async fn create_session(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfoDto> {
        let session_name = format!("{TMUX_PREFIX}{}", uuid::Uuid::new_v4());
        let working_dir = cwd.unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| "/".into())
                .to_string_lossy()
                .into_owned()
        });

        // Ensure xterm-256color for proper escape-sequence support
        let _ = tmux_cmd()
            .args(["set-option", "-g", "default-terminal", "xterm-256color"])
            .status()
            .await;

        let status = tmux_cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
                "-c",
                &working_dir,
            ])
            .status()
            .await
            .context("failed to run tmux new-session")?;

        anyhow::ensure!(status.success(), "tmux new-session exited with error");

        let fifo_path = format!("/tmp/lukan-tmux-{session_name}");
        let reader_handle =
            Self::spawn_output_reader(session_name.clone(), fifo_path.clone(), output_tx);

        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            session_name.clone(),
            TmuxSession {
                id: session_name.clone(),
                cols,
                rows,
                name: None,
                fifo_path,
                reader_handle,
            },
        );

        Ok(TerminalSessionInfoDto {
            id: session_name,
            cols,
            rows,
            name: None,
        })
    }

    /// Write base64-encoded input to a tmux session via `tmux send-keys -H`.
    pub async fn write_input(&self, session_id: &str, data: &str) -> anyhow::Result<()> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("invalid base64 input")?;

        if bytes.is_empty() {
            return Ok(());
        }

        // send-keys -H accepts space-separated hex bytes — this is the most
        // reliable way to send arbitrary bytes (including control chars) to tmux.
        let hex_args: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();

        let mut cmd = tmux_cmd();
        cmd.arg("send-keys").arg("-t").arg(session_id).arg("-H");
        for h in &hex_args {
            cmd.arg(h);
        }

        let status = cmd
            .status()
            .await
            .context("failed to run tmux send-keys")?;

        if !status.success() {
            anyhow::bail!("tmux send-keys failed for session {session_id}");
        }

        Ok(())
    }

    /// Resize a tmux session's window.
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let status = tmux_cmd()
            .args([
                "resize-window",
                "-t",
                session_id,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
            ])
            .status()
            .await
            .context("tmux resize-window failed")?;

        if !status.success() {
            anyhow::bail!("tmux resize-window failed for session {session_id}");
        }

        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(session_id) {
            s.cols = cols;
            s.rows = rows;
        }

        Ok(())
    }

    /// Destroy a terminal session — kills the tmux session and cleans up the FIFO.
    pub async fn destroy(&self, session_id: &str) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            session.reader_handle.abort();
            let _ = tokio::fs::remove_file(&session.fifo_path).await;
        }

        let _ = tmux_cmd()
            .args(["kill-session", "-t", session_id])
            .status()
            .await;

        Ok(())
    }

    /// Rename a terminal session's user-facing label.
    pub async fn rename_session(&self, session_id: &str, name: String) {
        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(session_id) {
            s.name = Some(name);
        }
    }

    /// List all tracked terminal sessions.
    pub async fn list(&self) -> Vec<TerminalSessionInfoDto> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| TerminalSessionInfoDto {
                id: s.id.clone(),
                cols: s.cols,
                rows: s.rows,
                name: s.name.clone(),
            })
            .collect()
    }

    /// Discover and re-adopt orphaned tmux sessions with the `lukan-` prefix.
    ///
    /// Called on server startup or when a WebSocket client connects. For each
    /// tmux session found that we aren't already tracking, we re-attach a FIFO
    /// reader so output flows again.
    pub async fn recover_sessions(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
    ) -> Vec<TerminalSessionInfoDto> {
        let output = tmux_cmd_with_output()
            .args([
                "list-sessions",
                "-F",
                "#{session_name}:#{window_width}:#{window_height}",
            ])
            .output()
            .await;

        let Ok(output) = output else {
            return vec![];
        };
        if !output.status.success() {
            // tmux server not running → no sessions to recover
            return vec![];
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut recovered = vec![];
        let mut sessions = self.sessions.lock().await;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() != 3 {
                continue;
            }

            let name = parts[0];
            if !name.starts_with(TMUX_PREFIX) {
                continue;
            }

            // Already tracking this session
            if sessions.contains_key(name) {
                continue;
            }

            let cols: u16 = parts[1].parse().unwrap_or(80);
            let rows: u16 = parts[2].parse().unwrap_or(24);

            let fifo_path = format!("/tmp/lukan-tmux-{name}");
            let reader_handle =
                Self::spawn_output_reader(name.to_string(), fifo_path.clone(), output_tx.clone());

            let id = name.to_string();
            sessions.insert(
                id.clone(),
                TmuxSession {
                    id: id.clone(),
                    cols,
                    rows,
                    name: None,
                    fifo_path,
                    reader_handle,
                },
            );

            recovered.push(TerminalSessionInfoDto {
                id,
                cols,
                rows,
                name: None,
            });
        }

        recovered
    }

    /// Capture the current visible pane content + scrollback as base64.
    ///
    /// Used during reconnection to replay terminal state to the client.
    pub async fn capture_scrollback(&self, session_id: &str) -> anyhow::Result<String> {
        let output = tmux_cmd_with_output()
            .args([
                "capture-pane",
                "-t",
                session_id,
                "-p",         // print to stdout
                "-e",         // include escape sequences (colors)
                "-S", "-500", // last 500 lines of scrollback
            ])
            .output()
            .await
            .context("tmux capture-pane failed")?;

        anyhow::ensure!(
            output.status.success(),
            "tmux capture-pane exited with error"
        );

        // Only trim trailing whitespace — preserve leading content and escape sequences
        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text.trim_end();
        let b64 = base64::engine::general_purpose::STANDARD.encode(trimmed.as_bytes());
        Ok(b64)
    }

    /// Spawn an async task that reads from a FIFO (created by `tmux pipe-pane`)
    /// and broadcasts output as `ServerMessage::TerminalOutput`.
    fn spawn_output_reader(
        session_id: String,
        fifo_path: String,
        tx: broadcast::Sender<ServerMessage>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Disable any stale pipe-pane left from a previous server run.
            // Without this, tmux thinks a pipe is already active and the new
            // pipe-pane command below becomes a no-op.
            let _ = tmux_cmd()
                .args(["pipe-pane", "-t", &session_id, ""])
                .status()
                .await;

            // Clean up any stale FIFO from a previous run
            let _ = tokio::fs::remove_file(&fifo_path).await;

            // Create FIFO
            let fifo_c = std::ffi::CString::new(fifo_path.as_str()).unwrap();
            let rc = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
            if rc != 0 {
                tracing::error!(session_id, fifo_path, "failed to create FIFO");
                return;
            }

            // Tell tmux to pipe pane output into the FIFO
            let pipe_status = tmux_cmd()
                .args([
                    "pipe-pane",
                    "-t",
                    &session_id,
                    &format!("cat >> '{fifo_path}'"),
                ])
                .status()
                .await;

            if pipe_status.is_err() || !pipe_status.unwrap().success() {
                tracing::error!(session_id, "tmux pipe-pane failed");
                let _ = tokio::fs::remove_file(&fifo_path).await;
                return;
            }

            // Open the FIFO for reading (blocks until tmux opens the write end)
            let file = match tokio::fs::File::open(&fifo_path).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!(session_id, error = %e, "failed to open FIFO");
                    return;
                }
            };

            let mut reader = tokio::io::BufReader::new(file);
            let mut buf = [0u8; 4096];

            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => {
                        // FIFO closed — tmux session likely exited
                        let _ = tx.send(ServerMessage::TerminalExited {
                            session_id: session_id.clone(),
                        });
                        break;
                    }
                    Ok(n) => {
                        let b64 =
                            base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        let _ = tx.send(ServerMessage::TerminalOutput {
                            session_id: session_id.clone(),
                            data: b64,
                        });
                    }
                    Err(e) => {
                        tracing::error!(session_id, error = %e, "FIFO read error");
                        break;
                    }
                }
            }

            // Cleanup
            let _ = tokio::fs::remove_file(&fifo_path).await;
        })
    }
}
