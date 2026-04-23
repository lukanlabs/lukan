use anyhow::Result;
use chrono::Utc;
use lukan_core::config::LukanPaths;
use lukan_core::models::sessions::{ChatSession, SessionSummary};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::subagent_worktrees::canonical_project_root;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionWorktreeState {
    pub path: String,
    pub slug: String,
    pub original_root: String,
}

/// Manages session persistence to ~/.config/lukan/sessions/{id}.json
pub struct SessionManager;

impl SessionManager {
    fn session_worktree_path(id: &str) -> std::path::PathBuf {
        LukanPaths::sessions_dir().join(format!("{id}.worktree.json"))
    }

    pub async fn create(provider: &str, model: &str) -> Result<ChatSession> {
        let id = generate_session_id();
        let mut session = ChatSession::new(id);
        session.provider = Some(provider.to_string());
        session.model = Some(model.to_string());
        if let Some(cwd) = session.cwd.clone() {
            session.project_root = Some(
                canonical_project_root(std::path::Path::new(&cwd))
                    .to_string_lossy()
                    .to_string(),
            );
        }
        debug!(id = %session.id, "Created new session");
        Ok(session)
    }

    /// Load a session from disk. Returns None if the file doesn't exist.
    pub async fn load(id: &str) -> Result<Option<ChatSession>> {
        let path = LukanPaths::session_file(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let session: ChatSession = serde_json::from_str(&data)?;
        debug!(id, messages = session.messages.len(), "Loaded session");
        Ok(Some(session))
    }

    /// Save a session to disk (bumps updated_at)
    pub async fn save(session: &mut ChatSession) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        session.updated_at = Utc::now();
        let path = LukanPaths::session_file(&session.id);
        let data = serde_json::to_string_pretty(session)?;
        tokio::fs::write(&path, data).await?;
        debug!(id = %session.id, path = %path.display(), "Saved session");
        Ok(())
    }

    /// Save session worktree state associated with a session id.
    pub async fn save_worktree_state(id: &str, state: &SessionWorktreeState) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        let path = Self::session_worktree_path(id);
        let data = serde_json::to_string_pretty(state)?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    /// Load session worktree state if present.
    pub async fn load_worktree_state(id: &str) -> Result<Option<SessionWorktreeState>> {
        let path = Self::session_worktree_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(path).await?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    /// List all sessions sorted by updated_at descending
    pub async fn list() -> Result<Vec<SessionSummary>> {
        let dir = LukanPaths::sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&dir).await?;
        let mut sessions = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match tokio::fs::read_to_string(&path).await {
                    Ok(data) => match serde_json::from_str::<ChatSession>(&data) {
                        Ok(session) => sessions.push(session.summary()),
                        Err(e) => {
                            debug!(path = %path.display(), error = %e, "Skipping invalid session file");
                        }
                    },
                    Err(e) => {
                        debug!(path = %path.display(), error = %e, "Failed to read session file");
                    }
                }
            }
        }

        // Sort by updated_at descending (most recent first)
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(sessions)
    }

    /// List sessions filtered by working directory.
    /// Only returns sessions that were created in the given cwd.
    /// Sessions without a cwd (old format) are excluded.
    pub async fn list_for_cwd(cwd: &str) -> Result<Vec<SessionSummary>> {
        let all = Self::list().await?;
        Ok(all
            .into_iter()
            .filter(|s| s.project_root.as_deref() == Some(cwd) || s.cwd.as_deref() == Some(cwd))
            .collect())
    }

    /// Delete a session file. Returns true if it existed.
    pub async fn delete(id: &str) -> Result<bool> {
        let path = LukanPaths::session_file(id);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
            debug!(id, "Deleted session");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete all session files. Returns the number deleted.
    pub async fn delete_all() -> Result<u32> {
        let dir = LukanPaths::sessions_dir();
        if !dir.exists() {
            return Ok(0);
        }
        let mut entries = tokio::fs::read_dir(&dir).await?;
        let mut count = 0u32;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                tokio::fs::remove_file(&path).await?;
                count += 1;
            }
        }
        debug!(count, "Deleted all sessions");
        Ok(count)
    }
}

/// Generate a random 6-char hex string (matches Node's randomBytes(3).toString("hex"))
fn generate_session_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_id() {
        let id = generate_session_id();
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xab, 0xcd, 0xef]), "abcdef");
        assert_eq!(hex_encode(&[0x00, 0x01, 0x0a]), "00010a");
    }

    #[tokio::test]
    async fn test_create_session() {
        let session = SessionManager::create("anthropic", "claude-sonnet-4-20250514")
            .await
            .unwrap();
        assert_eq!(session.id.len(), 6);
        assert_eq!(session.provider.as_deref(), Some("anthropic"));
        assert_eq!(session.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert!(session.messages.is_empty());
        assert!(session.project_root.is_some());
    }

    #[tokio::test]
    async fn test_session_worktree_state_roundtrip() {
        let state = SessionWorktreeState {
            path: "/tmp/worktree".to_string(),
            slug: "feature-x".to_string(),
            original_root: "/tmp/repo".to_string(),
        };
        SessionManager::save_worktree_state("abc123", &state)
            .await
            .unwrap();
        let loaded = SessionManager::load_worktree_state("abc123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.path, state.path);
        assert_eq!(loaded.slug, state.slug);
        assert_eq!(loaded.original_root, state.original_root);
    }
}
