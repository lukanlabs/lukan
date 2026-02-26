use base64::Engine;
use portable_pty::PtySize;
use std::io::Write;
use tauri::{AppHandle, State};

use crate::terminal_state::{TerminalSessionInfo, TerminalState};

#[tauri::command]
pub async fn terminal_create(
    app: AppHandle,
    state: State<'_, TerminalState>,
    cwd: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<TerminalSessionInfo, String> {
    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);
    let mut sessions = state.sessions.lock().await;
    state
        .create_session(&app, &mut sessions, cwd, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn terminal_input(
    state: State<'_, TerminalState>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("invalid base64: {e}"))?;
    session
        .writer
        .write_all(&bytes)
        .map_err(|e| format!("write failed: {e}"))?;
    session
        .writer
        .flush()
        .map_err(|e| format!("flush failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, TerminalState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    if cols == session.cols && rows == session.rows {
        return Ok(());
    }
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize failed: {e}"))?;
    session.cols = cols;
    session.rows = rows;
    Ok(())
}

#[tauri::command]
pub async fn terminal_destroy(
    state: State<'_, TerminalState>,
    session_id: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().await;
    if let Some(mut session) = sessions.remove(&session_id) {
        // Kill the child process; ignore errors if already exited
        let _ = session.child.kill();
    }
    Ok(())
}

#[tauri::command]
pub async fn terminal_list(
    state: State<'_, TerminalState>,
) -> Result<Vec<TerminalSessionInfo>, String> {
    let sessions = state.sessions.lock().await;
    Ok(sessions
        .values()
        .map(|s| TerminalSessionInfo {
            id: s.id.clone(),
            cols: s.cols,
            rows: s.rows,
        })
        .collect())
}
