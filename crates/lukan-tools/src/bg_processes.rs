use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use tracing::{debug, warn};

/// Information about a background process
pub struct BgProcess {
    pub pid: u32,
    pub command: String,
    pub started_at: DateTime<Utc>,
    pub log_file: PathBuf,
    pub session_id: Option<String>,
    /// Frontend tab ID (for matching with UI tab labels)
    pub tab_id: Option<String>,
    /// Human-readable label for the agent/tab that spawned this process
    pub label: Option<String>,
    /// When the process was detected as no longer alive
    pub exited_at: Option<DateTime<Utc>>,
    /// Whether the process was explicitly killed via kill_bg_process
    pub killed: bool,
}

/// Global tracker for background processes
struct BgTracker {
    /// Map from PID to process info (kept even after process dies, for log retrieval)
    processes: HashMap<u32, BgProcess>,
}

fn tracker() -> &'static Mutex<BgTracker> {
    static INSTANCE: OnceLock<Mutex<BgTracker>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        Mutex::new(BgTracker {
            processes: HashMap::new(),
        })
    })
}

/// Register a new background process
pub fn add_bg_process(
    pid: u32,
    command: String,
    log_file: PathBuf,
    session_id: Option<String>,
    label: Option<String>,
    tab_id: Option<String>,
) {
    let process = BgProcess {
        pid,
        command,
        started_at: Utc::now(),
        log_file,
        session_id,
        tab_id,
        label,
        exited_at: None,
        killed: false,
    };
    let mut t = tracker().lock().unwrap();
    t.processes.insert(pid, process);
    debug!(pid, "Registered background process");
}

/// Process status as seen by the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgProcessStatus {
    Running,
    Completed,
    Killed,
}

/// Snapshot of a background process for the UI
pub struct BgProcessSnapshot {
    pub pid: u32,
    pub command: String,
    pub started_at: DateTime<Utc>,
    pub exited_at: Option<DateTime<Utc>>,
    pub status: BgProcessStatus,
    pub label: Option<String>,
    pub session_id: Option<String>,
    pub tab_id: Option<String>,
}

/// Check if a process is still alive using `kill(pid, 0)`
pub fn is_process_alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 just checks if the process exists
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Recursively find all descendant PIDs of a process by walking `/proc`.
/// Returns descendants in depth-first order (children before grandchildren),
/// which means reversing the result gives a bottom-up order for killing.
fn find_descendants(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        // Read /proc/<p>/task/<p>/children to get direct children
        let path = format!("/proc/{p}/task/{p}/children");
        if let Ok(children_str) = std::fs::read_to_string(&path) {
            for token in children_str.split_whitespace() {
                if let Ok(child_pid) = token.parse::<u32>() {
                    result.push(child_pid);
                    stack.push(child_pid);
                }
            }
        }
    }
    result
}

/// Send a signal to a process and all its descendants (found via /proc tree walk).
/// This is more robust than process-group kill because it catches children that
/// created their own process groups (e.g. uvicorn --reload, python subprocess).
fn kill_tree(pid: u32, signal: i32) {
    let descendants = find_descendants(pid);
    // Kill descendants first (bottom-up: reverse since find_descendants is DFS)
    for &child in descendants.iter().rev() {
        unsafe {
            libc::kill(child as i32, signal);
        }
    }
    // Kill the root process
    unsafe {
        libc::kill(pid as i32, signal);
    }
    // Also try the process group for good measure
    let pgid = -(pid as i32);
    unsafe {
        libc::kill(pgid, signal);
    }
}

/// Build a snapshot from a tracked process, updating exited_at on first death detection.
fn snapshot_process(p: &mut BgProcess) -> BgProcessSnapshot {
    let alive = is_process_alive(p.pid);
    if !alive && p.exited_at.is_none() {
        p.exited_at = Some(Utc::now());
    }
    let status = if alive {
        BgProcessStatus::Running
    } else if p.killed {
        BgProcessStatus::Killed
    } else {
        BgProcessStatus::Completed
    };
    BgProcessSnapshot {
        pid: p.pid,
        command: p.command.clone(),
        started_at: p.started_at,
        exited_at: p.exited_at,
        status,
        label: p.label.clone(),
        session_id: p.session_id.clone(),
        tab_id: p.tab_id.clone(),
    }
}

/// Get list of all tracked background processes (alive ones first, then dead)
pub fn get_bg_processes() -> Vec<BgProcessSnapshot> {
    let mut t = tracker().lock().unwrap();
    let mut result: Vec<BgProcessSnapshot> =
        t.processes.values_mut().map(snapshot_process).collect();
    result.sort_by(|a, b| {
        let a_alive = a.status == BgProcessStatus::Running;
        let b_alive = b.status == BgProcessStatus::Running;
        b_alive.cmp(&a_alive).then(b.started_at.cmp(&a.started_at))
    });
    result
}

/// Get list of background processes filtered by session
pub fn get_bg_processes_for_session(session_id: &str) -> Vec<BgProcessSnapshot> {
    let mut t = tracker().lock().unwrap();
    let mut result: Vec<BgProcessSnapshot> = t
        .processes
        .values_mut()
        .filter(|p| p.session_id.as_deref() == Some(session_id))
        .map(snapshot_process)
        .collect();
    result.sort_by(|a, b| {
        let a_alive = a.status == BgProcessStatus::Running;
        let b_alive = b.status == BgProcessStatus::Running;
        b_alive.cmp(&a_alive).then(b.started_at.cmp(&a.started_at))
    });
    result
}

/// Read the last `max_lines` from a background process log file
pub fn get_bg_log(pid: u32, max_lines: usize) -> Option<String> {
    let log_path = {
        let t = tracker().lock().unwrap();
        let process = t.processes.get(&pid)?;
        process.log_file.clone()
    };

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() <= max_lines {
                Some(content)
            } else {
                let start = lines.len() - max_lines;
                Some(lines[start..].join("\n"))
            }
        }
        Err(e) => {
            warn!(pid, error = %e, "Failed to read bg process log");
            None
        }
    }
}

/// Kill a background process and all its descendants by walking the process
/// tree via `/proc`. This catches children in different process groups (e.g.
/// uvicorn --reload spawns a subprocess that may have its own PGID).
pub fn kill_bg_process(pid: u32) -> bool {
    // Mark as killed in the tracker before sending signal
    {
        let mut t = tracker().lock().unwrap();
        if let Some(p) = t.processes.get_mut(&pid) {
            p.killed = true;
        }
    }

    let alive = is_process_alive(pid);
    if !alive {
        debug!(pid, "Process already dead");
        return false;
    }

    kill_tree(pid, libc::SIGTERM);
    debug!(pid, "Sent SIGTERM to process tree");
    true
}

/// Forcefully kill a process tree: SIGTERM → wait 500ms → SIGKILL.
/// Walks `/proc` to find all descendants so nothing escapes, even if
/// children created their own process groups.
pub async fn kill_process_group_force(pid: u32) {
    // SIGTERM the entire tree
    kill_tree(pid, libc::SIGTERM);
    debug!(pid, "Sent SIGTERM to process tree (force kill)");

    // Wait 500ms for graceful shutdown
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // SIGKILL the entire tree — re-walk because SIGTERM may have spawned
    // cleanup children or some processes may have forked since the first walk
    kill_tree(pid, libc::SIGKILL);
    debug!(pid, "Sent SIGKILL to process tree");
}

/// Wait for a background process to finish, polling every `poll_interval_ms`.
/// Returns the log content when done, or None on timeout.
pub async fn wait_bg_process(pid: u32, timeout_ms: u64) -> Option<String> {
    let poll_interval = std::time::Duration::from_millis(2000);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    loop {
        if !is_process_alive(pid) {
            // Process finished — return its full log
            return get_bg_log(pid, usize::MAX);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Remove a tracked process entry (and its log file)
pub fn remove_bg_process(pid: u32) {
    let mut t = tracker().lock().unwrap();
    if let Some(process) = t.processes.remove(&pid) {
        let _ = std::fs::remove_file(&process.log_file);
    }
}

/// Clean up all tracked processes: kill alive ones (entire tree), remove log files
pub fn cleanup_all() {
    let mut t = tracker().lock().unwrap();
    for (pid, process) in t.processes.drain() {
        if is_process_alive(pid) {
            kill_tree(pid, libc::SIGTERM);
        }
        let _ = std::fs::remove_file(&process.log_file);
    }
    debug!("Cleaned up all background processes");
}

/// Get log file path for a PID
pub fn log_file_path(pid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/lukan-bg-{pid}.log"))
}
