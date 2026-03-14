use std::collections::HashMap;
use std::process::Stdio;

use anyhow::Context;
use base64::Engine;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, broadcast};

use portable_pty::PtySystem;

use lukan_core::config::LukanPaths;

use crate::protocol::{ServerMessage, TerminalSessionInfoDto};

/// Prefix for all lukan-managed tmux sessions.
const TMUX_PREFIX: &str = "lukan-";

/// Load terminal names from disk.
fn load_terminal_names() -> HashMap<String, String> {
    let path = LukanPaths::terminal_names_file();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Save terminal names to disk.
fn save_terminal_names(names: &HashMap<String, String>) {
    let path = LukanPaths::terminal_names_file();
    if let Ok(data) = serde_json::to_string_pretty(names) {
        let _ = std::fs::write(&path, data);
    }
}

/// Check if tmux is available in PATH.
fn has_tmux() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build a `Command` for tmux isolated on the `lukan` socket with no config file.
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

// ── Session types ────────────────────────────────────────────────────

/// A PTY-backed terminal session (fallback when tmux is not installed).
struct PtySession {
    id: String,
    cols: u16,
    rows: u16,
    name: Option<String>,
    writer: Box<dyn std::io::Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

/// A tmux-backed terminal session.
struct TmuxSession {
    id: String,
    cols: u16,
    rows: u16,
    name: Option<String>,
    fifo_path: String,
    reader_handle: tokio::task::JoinHandle<()>,
}

/// A terminal session — either PTY-direct or tmux-backed.
enum Session {
    Pty(PtySession),
    Tmux(TmuxSession),
}

impl Session {
    fn id(&self) -> &str {
        match self {
            Self::Pty(s) => &s.id,
            Self::Tmux(s) => &s.id,
        }
    }

    fn cols(&self) -> u16 {
        match self {
            Self::Pty(s) => s.cols,
            Self::Tmux(s) => s.cols,
        }
    }

    fn rows(&self) -> u16 {
        match self {
            Self::Pty(s) => s.rows,
            Self::Tmux(s) => s.rows,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Self::Pty(s) => s.name.as_deref(),
            Self::Tmux(s) => s.name.as_deref(),
        }
    }

    fn to_dto(&self) -> TerminalSessionInfoDto {
        TerminalSessionInfoDto {
            id: self.id().to_string(),
            cols: self.cols(),
            rows: self.rows(),
            name: self.name().map(|s| s.to_string()),
        }
    }
}

// ── Terminal Manager ─────────────────────────────────────────────────

/// Manages terminal sessions — uses tmux when available, falls back to PTY.
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Session>>,
    tmux_available: bool,
}

impl Default for TerminalManager {
    fn default() -> Self {
        let tmux_available = has_tmux();
        if tmux_available {
            tracing::info!("tmux detected — terminal sessions will persist across reconnects");
        } else {
            tracing::info!("tmux not found — using direct PTY (sessions won't persist)");
        }
        Self {
            sessions: Mutex::new(HashMap::new()),
            tmux_available,
        }
    }
}

impl TerminalManager {
    /// Create a new terminal session.
    pub async fn create_session(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfoDto> {
        if self.tmux_available {
            self.create_tmux_session(output_tx, cwd, cols, rows).await
        } else {
            self.create_pty_session(output_tx, cwd, cols, rows).await
        }
    }

    /// Write input to a terminal session.
    pub async fn write_input(&self, session_id: &str, data: &str) -> anyhow::Result<()> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("invalid base64 input")?;

        if bytes.is_empty() {
            return Ok(());
        }

        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

        match session {
            Session::Pty(s) => {
                s.writer
                    .write_all(&bytes)
                    .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
                s.writer
                    .flush()
                    .map_err(|e| anyhow::anyhow!("flush failed: {e}"))?;
            }
            Session::Tmux(_) => {
                drop(sessions); // release lock before spawning tmux process
                let hex_args: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
                let mut cmd = tmux_cmd();
                cmd.arg("send-keys").arg("-t").arg(session_id).arg("-H");
                for h in &hex_args {
                    cmd.arg(h);
                }
                let status = cmd.status().await.context("failed to run tmux send-keys")?;
                if !status.success() {
                    anyhow::bail!("tmux send-keys failed for session {session_id}");
                }
            }
        }

        Ok(())
    }

    /// Resize a terminal session.
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

        match session {
            Session::Pty(s) => {
                s.master
                    .resize(portable_pty::PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| anyhow::anyhow!("resize failed: {e}"))?;
                s.cols = cols;
                s.rows = rows;
            }
            Session::Tmux(s) => {
                let sid = s.id.clone();
                s.cols = cols;
                s.rows = rows;
                drop(sessions);
                let _ = tmux_cmd()
                    .args([
                        "resize-window",
                        "-t",
                        &sid,
                        "-x",
                        &cols.to_string(),
                        "-y",
                        &rows.to_string(),
                    ])
                    .status()
                    .await;
            }
        }

        Ok(())
    }

    /// Destroy a terminal session.
    pub async fn destroy(&self, session_id: &str) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            // Remove persisted name
            let mut names = load_terminal_names();
            if names.remove(session_id).is_some() {
                save_terminal_names(&names);
            }

            match session {
                Session::Pty(mut s) => {
                    let _ = s.child.kill();
                }
                Session::Tmux(s) => {
                    s.reader_handle.abort();
                    let _ = tokio::fs::remove_file(&s.fifo_path).await;
                    drop(sessions);
                    let _ = tmux_cmd()
                        .args(["kill-session", "-t", session_id])
                        .status()
                        .await;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Rename a terminal session (persisted to disk).
    pub async fn rename_session(&self, session_id: &str, name: String) {
        let mut sessions = self.sessions.lock().await;
        match sessions.get_mut(session_id) {
            Some(Session::Tmux(s)) => s.name = Some(name.clone()),
            Some(Session::Pty(s)) => s.name = Some(name.clone()),
            None => return,
        }
        drop(sessions);

        let mut names = load_terminal_names();
        names.insert(session_id.to_string(), name);
        save_terminal_names(&names);
    }

    /// List all tracked terminal sessions.
    pub async fn list(&self) -> Vec<TerminalSessionInfoDto> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.to_dto()).collect()
    }

    /// Recover orphaned tmux sessions (tmux backend only).
    pub async fn recover_sessions(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
    ) -> Vec<TerminalSessionInfoDto> {
        if !self.tmux_available {
            return vec![];
        }

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
            return vec![];
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut recovered = vec![];
        let mut sessions = self.sessions.lock().await;
        let saved_names = load_terminal_names();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() != 3 {
                continue;
            }

            let name = parts[0];
            if !name.starts_with(TMUX_PREFIX) {
                continue;
            }
            if sessions.contains_key(name) {
                continue;
            }

            let cols: u16 = parts[1].parse().unwrap_or(80);
            let rows: u16 = parts[2].parse().unwrap_or(24);

            let fifo_path = format!("/tmp/lukan-tmux-{name}");
            let reader_handle =
                Self::spawn_output_reader(name.to_string(), fifo_path.clone(), output_tx.clone());

            let id = name.to_string();
            let saved_name = saved_names.get(&id).cloned();
            sessions.insert(
                id.clone(),
                Session::Tmux(TmuxSession {
                    id: id.clone(),
                    cols,
                    rows,
                    name: saved_name.clone(),
                    fifo_path,
                    reader_handle,
                }),
            );

            recovered.push(TerminalSessionInfoDto {
                id,
                cols,
                rows,
                name: saved_name,
            });
        }

        recovered
    }

    /// Reset the pipe-pane output reader (tmux backend only).
    pub async fn reset_output_reader(
        &self,
        session_id: &str,
        output_tx: broadcast::Sender<ServerMessage>,
    ) {
        let mut sessions = self.sessions.lock().await;
        if let Some(Session::Tmux(session)) = sessions.get_mut(session_id) {
            session.reader_handle.abort();
            let _ = tokio::fs::remove_file(&session.fifo_path).await;
            session.reader_handle = Self::spawn_output_reader(
                session_id.to_string(),
                session.fifo_path.clone(),
                output_tx,
            );
        }
    }

    /// Capture scrollback (tmux backend only).
    pub async fn capture_scrollback(&self, session_id: &str) -> anyhow::Result<String> {
        if !self.tmux_available {
            // No scrollback for PTY sessions
            return Ok(String::new());
        }

        let output = tmux_cmd_with_output()
            .args(["capture-pane", "-t", session_id, "-p", "-e", "-S", "-500"])
            .output()
            .await
            .context("tmux capture-pane failed")?;

        anyhow::ensure!(
            output.status.success(),
            "tmux capture-pane exited with error"
        );

        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text.trim_end();
        // Drop the last line (usually the current prompt) because the
        // pipe reader will re-emit it once the new FIFO is established.
        // This prevents the prompt from appearing twice on reconnect.
        let without_last_line = match trimmed.rfind('\n') {
            Some(pos) => &trimmed[..pos],
            None => "", // single line = just the prompt, skip entirely
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(without_last_line.as_bytes());
        Ok(b64)
    }

    // ── PTY backend ──────────────────────────────────────────────────

    async fn create_pty_session(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfoDto> {
        let id = uuid::Uuid::new_v4().to_string();

        let pty_system = portable_pty::NativePtySystem::default();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("failed to open PTY: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = portable_pty::CommandBuilder::new(&shell);
        cmd.arg("-l");
        let working_dir = cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));
        cmd.cwd(working_dir);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow::anyhow!("failed to spawn shell: {e}"))?;
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("failed to take PTY writer: {e}"))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow::anyhow!("failed to clone PTY reader: {e}"))?;

        // Spawn blocking reader thread
        let session_id = id.clone();
        let tx = output_tx;
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        if tx
                            .send(ServerMessage::TerminalOutput {
                                session_id: session_id.clone(),
                                data: b64,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(ServerMessage::TerminalExited {
                session_id: session_id.clone(),
            });
        });

        let dto = TerminalSessionInfoDto {
            id: id.clone(),
            cols,
            rows,
            name: None,
        };

        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            id.clone(),
            Session::Pty(PtySession {
                id,
                cols,
                rows,
                name: None,
                writer,
                master: pair.master,
                child,
            }),
        );

        Ok(dto)
    }

    // ── Tmux backend ─────────────────────────────────────────────────

    async fn create_tmux_session(
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
            Session::Tmux(TmuxSession {
                id: session_name.clone(),
                cols,
                rows,
                name: None,
                fifo_path,
                reader_handle,
            }),
        );

        Ok(TerminalSessionInfoDto {
            id: session_name,
            cols,
            rows,
            name: None,
        })
    }

    /// Spawn async task that reads from a FIFO and broadcasts output.
    fn spawn_output_reader(
        session_id: String,
        fifo_path: String,
        tx: broadcast::Sender<ServerMessage>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let _ = tmux_cmd()
                .args(["pipe-pane", "-t", &session_id, ""])
                .status()
                .await;

            let _ = tokio::fs::remove_file(&fifo_path).await;

            let fifo_c = std::ffi::CString::new(fifo_path.as_str()).unwrap();
            let rc = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
            if rc != 0 {
                tracing::error!(session_id, fifo_path, "failed to create FIFO");
                return;
            }

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
                        let _ = tx.send(ServerMessage::TerminalExited {
                            session_id: session_id.clone(),
                        });
                        break;
                    }
                    Ok(n) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
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

            let _ = tokio::fs::remove_file(&fifo_path).await;
        })
    }
}
