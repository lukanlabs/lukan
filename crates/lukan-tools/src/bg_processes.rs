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
pub fn add_bg_process(pid: u32, command: String, log_file: PathBuf) {
    let process = BgProcess {
        pid,
        command,
        started_at: Utc::now(),
        log_file,
    };
    let mut t = tracker().lock().unwrap();
    t.processes.insert(pid, process);
    debug!(pid, "Registered background process");
}

/// Check if a process is still alive using `kill(pid, 0)`
pub fn is_process_alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 just checks if the process exists
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Get list of all tracked background processes (alive ones first, then dead)
pub fn get_bg_processes() -> Vec<(u32, String, DateTime<Utc>, bool)> {
    let t = tracker().lock().unwrap();
    let mut result: Vec<(u32, String, DateTime<Utc>, bool)> = t
        .processes
        .values()
        .map(|p| {
            let alive = is_process_alive(p.pid);
            (p.pid, p.command.clone(), p.started_at, alive)
        })
        .collect();
    // Sort: alive first, then by start time descending
    result.sort_by(|a, b| b.3.cmp(&a.3).then(b.2.cmp(&a.2)));
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

/// Kill a background process by PID
pub fn kill_bg_process(pid: u32) -> bool {
    // SAFETY: sending SIGTERM to the process
    let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if result == 0 {
        debug!(pid, "Sent SIGTERM to background process");
        true
    } else {
        warn!(pid, "Failed to kill background process");
        false
    }
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

/// Clean up all tracked processes: kill alive ones, remove log files
pub fn cleanup_all() {
    let mut t = tracker().lock().unwrap();
    for (pid, process) in t.processes.drain() {
        if is_process_alive(pid) {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        let _ = std::fs::remove_file(&process.log_file);
    }
    debug!("Cleaned up all background processes");
}

/// Get log file path for a PID
pub fn log_file_path(pid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/lukan-bg-{pid}.log"))
}
