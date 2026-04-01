use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Information about a background process
#[derive(Serialize, Deserialize)]
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

fn persist_path() -> PathBuf {
    lukan_core::config::LukanPaths::data_dir().join("bg-processes.json")
}

/// Save tracker state to disk (best-effort, non-blocking)
fn persist(tracker: &BgTracker) {
    let path = persist_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(&tracker.processes) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!(error = %e, "Failed to persist bg processes");
            }
        }
        Err(e) => warn!(error = %e, "Failed to serialize bg processes"),
    }
}

/// Load tracker state from disk
fn load_persisted() -> HashMap<u32, BgProcess> {
    let path = persist_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn tracker() -> &'static Mutex<BgTracker> {
    static INSTANCE: OnceLock<Mutex<BgTracker>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let mut processes = load_persisted();
        // Check which persisted processes are still alive and update dead ones
        for p in processes.values_mut() {
            if p.exited_at.is_none() && !is_process_alive(p.pid) {
                p.exited_at = Some(Utc::now());
            }
        }
        debug!(count = processes.len(), "Loaded persisted bg processes");
        Mutex::new(BgTracker { processes })
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
    persist(&t);
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

/// Recursively find all descendant PIDs of a process.
///
/// On Linux, walks `/proc/<pid>/task/<pid>/children`.
/// On macOS (and other platforms), falls back to `pgrep -P <pid>`.
/// Returns descendants in depth-first order (children before grandchildren),
/// which means reversing the result gives a bottom-up order for killing.
fn find_descendants(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        let children = get_direct_children(p);
        for child_pid in children {
            result.push(child_pid);
            stack.push(child_pid);
        }
    }
    result
}

/// Get direct child PIDs of a process.
#[cfg(target_os = "linux")]
fn get_direct_children(pid: u32) -> Vec<u32> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    match std::fs::read_to_string(&path) {
        Ok(children_str) => children_str
            .split_whitespace()
            .filter_map(|t| t.parse::<u32>().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Get direct child PIDs of a process using `pgrep -P`.
#[cfg(not(target_os = "linux"))]
fn get_direct_children(pid: u32) -> Vec<u32> {
    match std::process::Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|t| t.parse::<u32>().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
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
/// Returns (snapshot, changed) where changed indicates if exited_at was set.
fn snapshot_process(p: &mut BgProcess) -> (BgProcessSnapshot, bool) {
    let alive = is_process_alive(p.pid);
    let mut changed = false;
    if !alive && p.exited_at.is_none() {
        p.exited_at = Some(Utc::now());
        changed = true;
    }
    let status = if alive {
        BgProcessStatus::Running
    } else if p.killed {
        BgProcessStatus::Killed
    } else {
        BgProcessStatus::Completed
    };
    (
        BgProcessSnapshot {
            pid: p.pid,
            command: p.command.clone(),
            started_at: p.started_at,
            exited_at: p.exited_at,
            status,
            label: p.label.clone(),
            session_id: p.session_id.clone(),
            tab_id: p.tab_id.clone(),
        },
        changed,
    )
}

/// Merge processes from disk into the in-memory tracker.
/// Only imports processes that are still alive — dead ones on disk but not
/// in memory were likely cleared by the user and should stay removed.
fn merge_from_disk(t: &mut BgTracker) {
    let on_disk = load_persisted();
    let mut added = 0;
    for (pid, process) in on_disk {
        if !t.processes.contains_key(&pid) && is_process_alive(pid) {
            t.processes.insert(pid, process);
            added += 1;
        }
    }
    if added > 0 {
        debug!(added, "Merged live processes from disk into tracker");
    }
}

/// Collect snapshots from tracker, persist if any status changed.
fn collect_snapshots(t: &mut BgTracker, filter: Option<&str>) -> Vec<BgProcessSnapshot> {
    // Merge any processes added by other lukan instances
    merge_from_disk(t);

    let mut any_changed = false;
    let mut result: Vec<BgProcessSnapshot> = t
        .processes
        .values_mut()
        .filter(|p| match filter {
            Some(sid) => p.session_id.as_deref() == Some(sid),
            None => true,
        })
        .map(|p| {
            let (snap, changed) = snapshot_process(p);
            if changed {
                any_changed = true;
            }
            snap
        })
        .collect();
    if any_changed {
        persist(t);
    }
    result.sort_by(|a, b| {
        let a_alive = a.status == BgProcessStatus::Running;
        let b_alive = b.status == BgProcessStatus::Running;
        b_alive.cmp(&a_alive).then(b.started_at.cmp(&a.started_at))
    });
    result
}

/// Get list of all tracked background processes (alive ones first, then dead)
pub fn get_bg_processes() -> Vec<BgProcessSnapshot> {
    let mut t = tracker().lock().unwrap();
    collect_snapshots(&mut t, None)
}

/// Get list of background processes filtered by session
pub fn get_bg_processes_for_session(session_id: &str) -> Vec<BgProcessSnapshot> {
    let mut t = tracker().lock().unwrap();
    collect_snapshots(&mut t, Some(session_id))
}

/// Read the last `max_lines` from a background process log file.
/// Uses a tail-seek approach: reads only the last ~32KB of the file
/// to avoid reading megabytes of output on every poll.
pub fn get_bg_log(pid: u32, max_lines: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let log_path = {
        let t = tracker().lock().unwrap();
        let process = t.processes.get(&pid)?;
        process.log_file.clone()
    };

    let mut file = match std::fs::File::open(&log_path) {
        Ok(f) => f,
        Err(e) => {
            warn!(pid, error = %e, "Failed to read bg process log");
            return None;
        }
    };

    let metadata = file.metadata().ok()?;
    let file_len = metadata.len();

    // For small files, read everything. For large files, seek near the end.
    // 32KB is enough for ~200 lines of typical terminal output.
    const TAIL_BYTES: u64 = 32 * 1024;
    let mut content = String::new();

    if file_len > TAIL_BYTES {
        let _ = file.seek(SeekFrom::End(-(TAIL_BYTES as i64)));
        let _ = file.read_to_string(&mut content);
        // Drop the first partial line (we likely seeked into the middle of one)
        if let Some(pos) = content.find('\n') {
            content = content[pos + 1..].to_string();
        }
    } else {
        let _ = file.read_to_string(&mut content);
    }

    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        Some(content)
    } else {
        let start = lines.len() - max_lines;
        Some(lines[start..].join("\n"))
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
        persist(&t);
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

/// Get combined log output from all bg processes whose label starts with `prefix`.
/// Returns up to `max_lines` of the most recent output across all matching processes.
pub fn get_logs_by_label_prefix(prefix: &str, max_lines: usize) -> String {
    let pids: Vec<u32> = {
        let t = tracker().lock().unwrap();
        t.processes
            .values()
            .filter(|p| p.label.as_deref().is_some_and(|l| l.starts_with(prefix)))
            .map(|p| p.pid)
            .collect()
    };
    let mut combined = String::new();
    for pid in pids {
        if let Some(log) = get_bg_log(pid, max_lines)
            && !log.trim().is_empty()
        {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&log);
        }
    }
    combined
}

/// Kill all background processes whose label starts with `prefix`.
/// Returns the PIDs that were killed.
pub async fn kill_by_label_prefix(prefix: &str) -> Vec<u32> {
    let pids: Vec<u32> = {
        let t = tracker().lock().unwrap();
        t.processes
            .values()
            .filter(|p| {
                p.label.as_deref().is_some_and(|l| l.starts_with(prefix))
                    && p.exited_at.is_none()
                    && !p.killed
            })
            .map(|p| p.pid)
            .collect()
    };
    for &pid in &pids {
        kill_process_group_force(pid).await;
        let mut t = tracker().lock().unwrap();
        if let Some(proc) = t.processes.get_mut(&pid) {
            proc.killed = true;
            proc.exited_at = Some(Utc::now());
        }
        persist(&t);
    }
    pids
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

/// Remove all non-running processes from the tracker (clear history).
/// Returns the number of entries removed.
pub fn clear_completed() -> usize {
    let mut t = tracker().lock().unwrap();
    let before = t.processes.len();
    t.processes.retain(|&pid, p| {
        let alive = is_process_alive(pid);
        if !alive {
            let _ = std::fs::remove_file(&p.log_file);
        }
        alive
    });
    let removed = before - t.processes.len();
    if removed > 0 {
        persist(&t);
    }
    removed
}

/// Remove a tracked process entry (and its log file)
pub fn remove_bg_process(pid: u32) {
    let mut t = tracker().lock().unwrap();
    if let Some(process) = t.processes.remove(&pid) {
        let _ = std::fs::remove_file(&process.log_file);
        persist(&t);
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
    persist(&t);
    debug!("Cleaned up all background processes");
}

/// Get log file path for a PID
pub fn log_file_path(pid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/lukan-bg-{pid}.log"))
}
