use std::io::SeekFrom;

use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tracing::error;

use lukan_core::config::LukanPaths;

use super::WorkerNotification;

/// Watches the worker notifications JSONL file for new entries.
/// Tracks the byte offset to only return new notifications on each poll.
pub struct NotificationWatcher {
    offset: u64,
}

impl Default for NotificationWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationWatcher {
    /// Create a new watcher, starting from the current end of the file
    /// so only future notifications are returned.
    pub fn new() -> Self {
        let path = LukanPaths::worker_notifications_file();
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self { offset }
    }

    /// Poll for new notifications since the last call.
    /// Returns an empty vec if nothing new.
    pub async fn poll(&mut self) -> Vec<WorkerNotification> {
        let path = LukanPaths::worker_notifications_file();
        let current_len = match tokio::fs::metadata(&path).await {
            Ok(m) => m.len(),
            Err(_) => return vec![],
        };

        if current_len <= self.offset {
            // File truncated or unchanged
            if current_len < self.offset {
                self.offset = current_len;
            }
            return vec![];
        }

        let mut notifications = Vec::new();

        match tokio::fs::File::open(&path).await {
            Ok(mut file) => {
                if let Err(e) = file.seek(SeekFrom::Start(self.offset)).await {
                    error!(error = %e, "Failed to seek notification file");
                    return vec![];
                }

                let reader = BufReader::new(file);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(notif) = serde_json::from_str::<WorkerNotification>(&line) {
                        notifications.push(notif);
                    }
                }

                self.offset = current_len;
            }
            Err(e) => {
                error!(error = %e, "Failed to open notification file");
            }
        }

        notifications
    }
}
