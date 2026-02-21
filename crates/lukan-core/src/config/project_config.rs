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
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Auto,
            permissions: PermissionsConfig::default(),
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
