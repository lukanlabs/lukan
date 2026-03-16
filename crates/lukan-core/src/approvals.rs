use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::config::LukanPaths;

/// An approval request persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub pipeline_id: String,
    pub run_id: String,
    pub step_id: String,
    /// Human-readable context (rendered message)
    pub context: String,
    /// "pending" | "approved" | "rejected" | "timed_out"
    pub status: String,
    /// Who/what resolved the approval (e.g. "ui", "api", "whatsapp:+1234")
    pub resolved_by: Option<String>,
    /// Optional comment from the approver
    pub comment: Option<String>,
    pub created_at: String,
    pub timeout_at: String,
    pub resolved_at: Option<String>,
    /// Plugin that was notified (if any)
    pub notify_plugin: Option<String>,
    /// Channel that was notified (if any)
    pub notify_channel: Option<String>,
}

/// Manages approval persistence to disk.
///
/// Approvals: ~/.config/lukan/approvals/{id}.json
pub struct ApprovalManager;

impl ApprovalManager {
    /// Create a new approval request and persist it
    pub async fn create(req: ApprovalRequest) -> Result<ApprovalRequest> {
        let dir = LukanPaths::approvals_dir();
        tokio::fs::create_dir_all(&dir).await?;
        let path = LukanPaths::approval_file(&req.id);
        let data = serde_json::to_string_pretty(&req)?;
        tokio::fs::write(&path, data).await?;
        debug!(id = %req.id, pipeline = %req.pipeline_id, "Created approval request");
        Ok(req)
    }

    /// Get an approval request by ID
    pub async fn get(id: &str) -> Result<Option<ApprovalRequest>> {
        let path = LukanPaths::approval_file(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let req: ApprovalRequest = serde_json::from_str(&data)?;
        Ok(Some(req))
    }

    /// Resolve an approval (approve or reject). Atomic: write temp file then rename.
    pub async fn resolve(
        id: &str,
        approved: bool,
        resolved_by: &str,
        comment: Option<String>,
    ) -> Result<Option<ApprovalRequest>> {
        let path = LukanPaths::approval_file(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let mut req: ApprovalRequest = serde_json::from_str(&data)?;

        if req.status != "pending" {
            return Ok(Some(req)); // already resolved
        }

        req.status = if approved {
            "approved".to_string()
        } else {
            "rejected".to_string()
        };
        req.resolved_by = Some(resolved_by.to_string());
        req.comment = comment;
        req.resolved_at = Some(chrono::Utc::now().to_rfc3339());

        // Atomic write: write to temp file, then rename
        let tmp_path = path.with_extension("tmp");
        let new_data = serde_json::to_string_pretty(&req)?;
        tokio::fs::write(&tmp_path, &new_data).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        debug!(
            id = %req.id,
            status = %req.status,
            resolved_by,
            "Resolved approval request"
        );
        Ok(Some(req))
    }

    /// List all pending approval requests
    pub async fn list_pending() -> Result<Vec<ApprovalRequest>> {
        let dir = LukanPaths::approvals_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&dir).await?;
        let mut pending = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(data) = tokio::fs::read_to_string(&path).await
                && let Ok(req) = serde_json::from_str::<ApprovalRequest>(&data)
                && req.status == "pending"
            {
                pending.push(req);
            }
        }

        // Sort by created_at descending
        pending.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(pending)
    }

    /// Find pending approvals for a specific plugin and channel
    pub async fn find_pending_for_plugin(
        plugin: &str,
        channel: &str,
    ) -> Result<Vec<ApprovalRequest>> {
        let pending = Self::list_pending().await?;
        Ok(pending
            .into_iter()
            .filter(|r| {
                r.notify_plugin.as_deref() == Some(plugin)
                    && r.notify_channel.as_deref() == Some(channel)
            })
            .collect())
    }
}
