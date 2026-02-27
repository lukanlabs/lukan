use crate::state::ChatState;
use lukan_tools::bg_processes::{self, BgProcessStatus};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgProcessDto {
    pub pid: u32,
    pub command: String,
    pub started_at: String,
    /// null if still running, ISO 8601 timestamp if exited
    pub exited_at: Option<String>,
    /// "running" | "completed" | "killed"
    pub status: String,
}

#[tauri::command]
pub fn list_bg_processes(session_id: Option<String>) -> Vec<BgProcessDto> {
    let procs = match session_id.as_deref() {
        Some(id) if !id.is_empty() => bg_processes::get_bg_processes_for_session(id),
        _ => bg_processes::get_bg_processes(),
    };
    procs
        .into_iter()
        .map(|p| BgProcessDto {
            pid: p.pid,
            command: p.command,
            started_at: p.started_at.to_rfc3339(),
            exited_at: p.exited_at.map(|t| t.to_rfc3339()),
            status: match p.status {
                BgProcessStatus::Running => "running".to_string(),
                BgProcessStatus::Completed => "completed".to_string(),
                BgProcessStatus::Killed => "killed".to_string(),
            },
        })
        .collect()
}

#[tauri::command]
pub fn get_bg_process_log(pid: u32, max_lines: u32) -> Option<String> {
    bg_processes::get_bg_log(pid, max_lines as usize)
}

#[tauri::command]
pub async fn kill_bg_process(pid: u32) -> bool {
    // Mark as killed + send SIGTERM
    let sent = bg_processes::kill_bg_process(pid);
    if sent {
        // Escalate to SIGKILL if SIGTERM doesn't work within 500ms
        bg_processes::kill_process_group_force(pid).await;
    }
    sent
}

/// Send the currently running Bash tool to background (equivalent to Alt+B in TUI)
#[tauri::command]
pub async fn send_to_background(state: tauri::State<'_, ChatState>) -> Result<bool, String> {
    let guard = state.bg_signal_tx.lock().await;
    if let Some(tx) = guard.as_ref() {
        tx.send(()).map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}
