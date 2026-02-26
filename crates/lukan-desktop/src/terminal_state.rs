#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;

use base64::Engine;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

/// A single PTY session.
pub struct TerminalSession {
    pub id: String,
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
    pub cols: u16,
    pub rows: u16,
}

/// Event payload emitted to the frontend per session.
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalOutputEvent {
    /// Base64-encoded raw bytes from the PTY.
    Data { data: String },
    /// The shell process exited.
    Exited,
}

/// Info returned to frontend when a session is created.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionInfo {
    pub id: String,
    pub cols: u16,
    pub rows: u16,
}

/// Shared state for all terminal sessions, managed via `tauri::State`.
pub struct TerminalState {
    pub sessions: Mutex<HashMap<String, TerminalSession>>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl TerminalState {
    /// Spawn a new PTY session and start a reader thread that forwards output
    /// as base64-encoded Tauri events.
    pub fn create_session(
        &self,
        app: &AppHandle,
        sessions: &mut HashMap<String, TerminalSession>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfo> {
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
        cmd.arg("-l"); // login shell for proper env
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

        // Spawn a blocking reader thread that emits output as Tauri events.
        let event_name = format!("terminal-output-{id}");
        let app_handle = app.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        let event = TerminalOutputEvent::Data { data: b64 };
                        if app_handle.emit(&event_name, &event).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // PTY closed — notify frontend
            let _ = app_handle.emit(&event_name, &TerminalOutputEvent::Exited);
        });

        let info = TerminalSessionInfo {
            id: id.clone(),
            cols,
            rows,
        };

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
}
