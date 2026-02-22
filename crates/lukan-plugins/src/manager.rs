use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use lukan_core::config::LukanPaths;
use lukan_core::models::plugin::{HostMessage, PluginManifest, PluginMessage, PROTOCOL_VERSION};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::process::PluginProcess;

/// Runtime state for a started plugin.
struct RunningPlugin {
    process: PluginProcess,
    host_tx: mpsc::Sender<HostMessage>,
}

/// Manages plugin discovery, lifecycle, and communication.
pub struct PluginManager {
    running: HashMap<String, RunningPlugin>,
}

/// Info about a discovered/installed plugin.
#[derive(Debug)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: String,
    pub running: bool,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
        }
    }

    /// Discover all installed plugins by scanning the plugins directory.
    pub async fn discover(&self) -> Result<Vec<String>> {
        let plugins_dir = LukanPaths::plugins_dir();
        if !plugins_dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        let mut entries = tokio::fs::read_dir(&plugins_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir()
                && path.join("plugin.toml").exists()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                names.push(name.to_string());
            }
        }

        names.sort();
        Ok(names)
    }

    /// Load and parse a plugin manifest from plugin.toml.
    pub async fn load_manifest(name: &str) -> Result<PluginManifest> {
        let manifest_path = LukanPaths::plugin_manifest(name);
        let content = tokio::fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        Ok(manifest)
    }

    /// Start a plugin: spawn process, send Init, wait for Ready.
    /// Returns channels for PluginChannel to use.
    pub async fn start(
        &mut self,
        name: &str,
    ) -> Result<(mpsc::Receiver<PluginMessage>, mpsc::Sender<HostMessage>)> {
        if self.running.contains_key(name) {
            anyhow::bail!("Plugin '{}' is already running", name);
        }

        let manifest = Self::load_manifest(name).await?;

        // Load plugin-specific config.json (if exists)
        let config_path = LukanPaths::plugin_config(name);
        let config: serde_json::Value = if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path).await?;
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
        } else {
            serde_json::Value::Object(Default::default())
        };

        // Spawn process
        let mut process = PluginProcess::new(name.to_string(), manifest);
        process.spawn().await?;

        // Set up I/O loop
        let (mut plugin_rx, host_tx) = process.run_io_loop()?;

        // Send Init
        let init_msg = HostMessage::Init {
            name: name.to_string(),
            config,
            protocol_version: PROTOCOL_VERSION,
        };
        host_tx
            .send(init_msg)
            .await
            .context("Failed to send Init to plugin")?;

        // Wait for Ready (with timeout)
        let ready = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Some(msg) = plugin_rx.recv().await {
                if let PluginMessage::Ready { version, capabilities } = &msg {
                    info!(
                        plugin = %name,
                        version = %version,
                        ?capabilities,
                        "Plugin ready"
                    );
                    return Ok(msg);
                }
                // Forward non-Ready messages (like Log) but keep waiting
                if let PluginMessage::Log { level: _, message } = &msg {
                    info!(plugin = %name, "Plugin (pre-ready): {message}");
                }
                if let PluginMessage::Error { message, recoverable } = &msg {
                    error!(plugin = %name, "Plugin error during init: {message}");
                    if !recoverable {
                        anyhow::bail!("Plugin '{}' non-recoverable error: {}", name, message);
                    }
                }
            }
            anyhow::bail!("Plugin '{}' closed stdout without sending Ready", name)
        })
        .await
        .map_err(|_| anyhow::anyhow!("Plugin '{}' did not send Ready within 10s", name))??;

        // Verify Ready
        if let PluginMessage::Ready { .. } = ready {
            // Good
        }

        self.running.insert(
            name.to_string(),
            RunningPlugin {
                process,
                host_tx: host_tx.clone(),
            },
        );

        Ok((plugin_rx, host_tx))
    }

    /// Stop a running plugin gracefully.
    pub async fn stop(&mut self, name: &str) -> Result<()> {
        if let Some(mut rp) = self.running.remove(name) {
            // Send shutdown through the host_tx channel
            let _ = rp.host_tx.send(HostMessage::Shutdown).await;
            rp.process.shutdown().await?;
            info!(plugin = %name, "Plugin stopped");
        } else {
            anyhow::bail!("Plugin '{}' is not running", name);
        }
        Ok(())
    }

    /// List installed plugins with their status.
    pub async fn list(&self) -> Result<Vec<PluginInfo>> {
        let names = self.discover().await?;
        let mut infos = Vec::new();

        for name in names {
            match Self::load_manifest(&name).await {
                Ok(manifest) => {
                    infos.push(PluginInfo {
                        name: name.clone(),
                        version: manifest.plugin.version,
                        description: manifest.plugin.description,
                        plugin_type: manifest.plugin.plugin_type,
                        running: self.running.contains_key(&name),
                    });
                }
                Err(e) => {
                    infos.push(PluginInfo {
                        name: name.clone(),
                        version: "?".to_string(),
                        description: format!("(error loading manifest: {e})"),
                        plugin_type: "unknown".to_string(),
                        running: false,
                    });
                }
            }
        }

        Ok(infos)
    }

    /// Install a plugin from a local directory (copies files to plugins dir).
    pub async fn install(source: &str, name: Option<&str>) -> Result<String> {
        let source_path = Path::new(source);
        if !source_path.exists() {
            anyhow::bail!("Source path does not exist: {source}");
        }

        // Validate source has plugin.toml
        let manifest_path = source_path.join("plugin.toml");
        if !manifest_path.exists() {
            anyhow::bail!("No plugin.toml found in {source}");
        }

        // Parse manifest to get the plugin name
        let content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: PluginManifest = toml::from_str(&content)
            .context("Failed to parse plugin.toml")?;

        let plugin_name = name.unwrap_or(&manifest.plugin.name);
        let dest = LukanPaths::plugin_dir(plugin_name);

        if dest.exists() {
            anyhow::bail!(
                "Plugin '{}' already installed at {}",
                plugin_name,
                dest.display()
            );
        }

        // Copy directory recursively
        copy_dir_recursive(source_path, &dest).await?;

        info!(plugin = %plugin_name, "Plugin installed");
        Ok(plugin_name.to_string())
    }

    /// Remove an installed plugin. Stops it if running.
    pub async fn remove(&mut self, name: &str) -> Result<()> {
        // Stop if running
        if self.running.contains_key(name) {
            self.stop(name).await?;
        }

        let dir = LukanPaths::plugin_dir(name);
        if !dir.exists() {
            anyhow::bail!("Plugin '{}' not found at {}", name, dir.display());
        }

        tokio::fs::remove_dir_all(&dir).await?;
        info!(plugin = %name, "Plugin removed");
        Ok(())
    }

    /// Check if a plugin is currently running.
    pub fn is_running(&self, name: &str) -> bool {
        self.running.contains_key(name)
    }
}

/// Recursively copy a directory.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}
