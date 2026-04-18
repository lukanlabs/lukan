use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lukan_tools::ToolContext;
use tokio::sync::Mutex;

pub fn make_tool_context(cwd: &Path) -> ToolContext {
    ToolContext {
        progress_tx: None,
        event_tx: None,
        tool_call_id: None,
        read_files: Arc::new(Mutex::new(HashMap::new())),
        cwd: cwd.to_path_buf(),
        bg_signal: None,
        sandbox: None,
        allowed_paths: None,
        cancel: None,
        session_id: None,
        extra_env: HashMap::new(),
        agent_label: None,
        tab_id: None,
        blocked_env_vars: Vec::new(),
    }
}

pub fn make_restricted_tool_context(cwd: &Path, allowed_paths: Vec<PathBuf>) -> ToolContext {
    let mut ctx = make_tool_context(cwd);
    ctx.allowed_paths = Some(allowed_paths);
    ctx
}

pub async fn mark_file_as_read(ctx: &ToolContext, path: &Path) {
    let mtime = tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok());
    ctx.read_files.lock().await.insert(path.to_path_buf(), mtime);
}
