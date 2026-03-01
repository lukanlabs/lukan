use std::collections::HashMap;
use std::io::Write;

use base64::Engine;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::{Mutex, broadcast};

use crate::protocol::{ServerMessage, TerminalSessionInfoDto};

/// A single PTY session.
pub struct TerminalSession {
    pub id: String,
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
    pub cols: u16,
    pub rows: u16,
}

/// Manages all terminal sessions for the web server.
pub struct TerminalManager {
    pub sessions: Mutex<HashMap<String, TerminalSession>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl TerminalManager {
    /// Spawn a new PTY session. Output is sent via the broadcast channel
    /// as `ServerMessage::TerminalOutput` or `ServerMessage::TerminalExited`.
    pub async fn create_session(
        &self,
        output_tx: broadcast::Sender<ServerMessage>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfoDto> {
        let id = uuid::Uuid::new_v4().to_string();

        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("failed to open PTY: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
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

        // Spawn blocking reader thread that sends output via broadcast
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
                        let msg = ServerMessage::TerminalOutput {
                            session_id: session_id.clone(),
                            data: b64,
                        };
                        if tx.send(msg).is_err() {
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

        let info = TerminalSessionInfoDto {
            id: id.clone(),
            cols,
            rows,
        };

        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            id.clone(),
            TerminalSession {
                id,
                writer,
                master: pair.master,
                child,
                cols,
                rows,
            },
        );

        Ok(info)
    }

    /// Write input data to a terminal session.
    pub async fn write_input(&self, session_id: &str, data: &str) -> anyhow::Result<()> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| anyhow::anyhow!("invalid base64: {e}"))?;

        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

        session
            .writer
            .write_all(&bytes)
            .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| anyhow::anyhow!("flush failed: {e}"))?;

        Ok(())
    }

    /// Resize a terminal session.
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("resize failed: {e}"))?;

        session.cols = cols;
        session.rows = rows;

        Ok(())
    }

    /// Destroy a terminal session, killing the child process.
    pub async fn destroy(&self, session_id: &str) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
        }
        Ok(())
    }

    /// List all active terminal sessions.
    pub async fn list(&self) -> Vec<TerminalSessionInfoDto> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| TerminalSessionInfoDto {
                id: s.id.clone(),
                cols: s.cols,
                rows: s.rows,
            })
            .collect()
    }
}
