use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::types::{PermissionMode, PermissionsConfig};

/// Per-project configuration stored in .lukan/config.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default)]
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub trusted: bool,
    /// Extra directories the agent is allowed to access beyond the project root.
    /// Paths can be absolute or use `~` for home directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Auto,
            permissions: PermissionsConfig::default(),
            trusted: false,
            allowed_paths: Vec::new(),
        }
    }
}

impl ProjectConfig {
    /// Find and load project config by walking up from the given directory
    pub async fn load(start_dir: &Path) -> Result<Option<(PathBuf, Self)>> {
        let mut dir = start_dir.to_path_buf();

        loop {
            let config_path = dir.join(".lukan").join("config.json");
            if config_path.exists() {
                let content = tokio::fs::read_to_string(&config_path)
                    .await
                    .context("Failed to read .lukan/config.json")?;
                let config: ProjectConfig =
                    serde_json::from_str(&content).context("Failed to parse .lukan/config.json")?;
                return Ok(Some((dir, config)));
            }

            if !dir.pop() {
                break;
            }
        }

        Ok(None)
    }

    /// Mark the project directory as trusted.
    /// Loads or creates .lukan/config.json and sets `trusted: true`.
    pub async fn mark_trusted(project_dir: &Path) -> Result<()> {
        let lukan_dir = project_dir.join(".lukan");
        tokio::fs::create_dir_all(&lukan_dir).await?;

        let config_path = lukan_dir.join("config.json");
        let mut config = if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path)
                .await
                .context("Failed to read .lukan/config.json")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        };

        config.trusted = true;
        let content = serde_json::to_string_pretty(&config)?;
        tokio::fs::write(&config_path, content).await?;
        Ok(())
    }

    /// Build the list of allowed paths for the agent, starting with `cwd`,
    /// then appending any configured `allowed_paths` (expanding `~` to `$HOME`).
    pub fn resolve_allowed_paths(&self, cwd: &Path) -> Vec<PathBuf> {
        let mut allowed = vec![cwd.to_path_buf()];
        for p in &self.allowed_paths {
            let expanded = if p.starts_with('~') {
                let home = std::env::var("HOME").unwrap_or_default();
                PathBuf::from(p.replacen('~', &home, 1))
            } else {
                PathBuf::from(p)
            };
            if !allowed.contains(&expanded) {
                allowed.push(expanded);
            }
        }
        allowed
    }

    /// Initialize a .lukan directory with default config
    pub async fn init(project_dir: &Path) -> Result<PathBuf> {
        let lukan_dir = project_dir.join(".lukan");
        tokio::fs::create_dir_all(&lukan_dir).await?;

        let config = Self::default();
        let config_path = lukan_dir.join("config.json");
        let content = serde_json::to_string_pretty(&config)?;
        tokio::fs::write(&config_path, content).await?;

        Ok(lukan_dir)
    }
}
