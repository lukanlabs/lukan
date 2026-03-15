use lukan_core::config::LukanPaths;

use super::scheduler::PipelineNotification;

/// Watches the pipeline notification JSONL file for new entries.
/// Used by the web server to forward notifications to WebSocket clients.
pub struct PipelineNotificationWatcher {
    offset: u64,
}

impl PipelineNotificationWatcher {
    pub fn new() -> Self {
        // Start at the current end of the file so we only see new entries
        let offset = std::fs::metadata(LukanPaths::pipeline_notifications_file())
            .map(|m| m.len())
            .unwrap_or(0);
        Self { offset }
    }

    /// Read any new notification lines since the last poll
    pub async fn poll(&mut self) -> Vec<PipelineNotification> {
        let path = LukanPaths::pipeline_notifications_file();
        let Ok(data) = tokio::fs::read(&path).await else {
            return vec![];
        };

        if (data.len() as u64) <= self.offset {
            return vec![];
        }

        let new_data = &data[self.offset as usize..];
        self.offset = data.len() as u64;

        let text = String::from_utf8_lossy(new_data);
        text.lines()
            .filter_map(|line| serde_json::from_str::<PipelineNotification>(line).ok())
            .collect()
    }
}
