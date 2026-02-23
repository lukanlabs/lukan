use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lukan_core::config::LukanPaths;
use serde::Deserialize;
use tracing::info;

/// Default URL to fetch the registry from (R2 CDN).
const DEFAULT_REGISTRY_URL: &str = "https://get.lukan.ai/registry.toml";

// ── Registry TOML types ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RegistryFile {
    #[allow(dead_code)]
    meta: RegistryMeta,
    plugins: HashMap<String, RegistryPlugin>,
}

#[derive(Debug, Deserialize)]
struct RegistryMeta {
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    registry_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RegistryPlugin {
    pub description: String,
    pub version: String,
    pub plugin_type: String,
    pub source: String,
    #[serde(default)]
    pub assets: HashMap<String, PluginAsset>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PluginAsset {
    pub url: String,
    /// Binary name inside the archive (only for native/binary plugins).
    #[serde(default)]
    pub binary: Option<String>,
}

/// Summary for display.
pub struct RemotePluginInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub plugin_type: String,
    pub source: String,
    pub available: bool,
    pub installed: bool,
}

// ── Public API ────────────────────────────────────────────────────────

/// Fetch and parse the plugin registry.
pub async fn fetch_registry() -> Result<HashMap<String, RegistryPlugin>> {
    let url =
        std::env::var("LUKAN_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("Failed to fetch registry from {url}"))?;

    if !resp.status().is_success() {
        bail!("Registry fetch failed: HTTP {} from {url}", resp.status());
    }

    let body = resp.text().await?;
    let registry: RegistryFile = toml::from_str(&body).context("Failed to parse registry.toml")?;

    Ok(registry.plugins)
}

/// List remote plugins with installed status.
pub async fn list_remote() -> Result<Vec<RemotePluginInfo>> {
    let plugins = fetch_registry().await?;
    let platform = current_platform();

    let installed_dir = LukanPaths::plugins_dir();
    let mut infos: Vec<RemotePluginInfo> = Vec::new();

    let mut names: Vec<_> = plugins.keys().cloned().collect();
    names.sort();

    for name in names {
        let p = &plugins[&name];
        // Available if: has "all" asset, or has asset for current platform
        let available = p.assets.contains_key("all") || p.assets.contains_key(&platform);
        let installed = installed_dir.join(&name).join("plugin.toml").exists();

        infos.push(RemotePluginInfo {
            name,
            description: p.description.clone(),
            version: p.version.clone(),
            plugin_type: p.plugin_type.clone(),
            source: p.source.clone(),
            available,
            installed,
        });
    }

    Ok(infos)
}

/// Install a plugin from the remote registry.
///
/// Supports two source types:
/// - "archive": platform-independent (Node.js plugins), uses assets.all
/// - "binary": platform-specific (compiled plugins), uses assets.<platform>
pub async fn install_remote(name: &str, alias_override: Option<&str>) -> Result<String> {
    let plugins = fetch_registry().await?;

    let entry = plugins
        .get(name)
        .with_context(|| format!("Plugin '{name}' not found in registry"))?;

    // Resolve the download asset: try "all" first, then platform-specific
    let platform = current_platform();
    let asset = entry
        .assets
        .get("all")
        .or_else(|| entry.assets.get(&platform))
        .with_context(|| {
            let available: Vec<_> = entry.assets.keys().cloned().collect();
            format!(
                "No download available for plugin '{name}' on platform '{platform}'.\n\
                 Available: {}",
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                }
            )
        })?;

    // Check if already installed
    let dest = LukanPaths::plugin_dir(name);
    if dest.exists() {
        bail!(
            "Plugin '{name}' is already installed at {}\n\
             Use `lukan plugin remove {name}` first to reinstall.",
            dest.display()
        );
    }

    let platform_label = if entry.assets.contains_key("all") {
        "universal"
    } else {
        &platform
    };
    info!(plugin = %name, platform = %platform_label, "Downloading plugin");
    println!(
        "Downloading {name} v{} ({platform_label})...",
        entry.version
    );

    // Download tarball
    let resp = reqwest::get(&asset.url)
        .await
        .with_context(|| format!("Failed to download from {}", asset.url))?;

    if !resp.status().is_success() {
        bail!("Download failed: HTTP {} from {}", resp.status(), asset.url);
    }

    let bytes = resp.bytes().await?;
    println!("Downloaded {} bytes", bytes.len());

    // Extract to temp dir
    let tmp_dir = std::env::temp_dir().join(format!("lukan-plugin-{name}-{}", std::process::id()));
    tokio::fs::create_dir_all(&tmp_dir).await?;

    let tar_path = tmp_dir.join("archive.tar.gz");
    tokio::fs::write(&tar_path, &bytes).await?;

    let status = tokio::process::Command::new("tar")
        .args([
            "xzf",
            tar_path.to_str().unwrap(),
            "-C",
            tmp_dir.to_str().unwrap(),
        ])
        .status()
        .await
        .context("Failed to run tar")?;

    if !status.success() {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        bail!("tar extraction failed");
    }

    // Find plugin.toml in extracted files (may be in a subdirectory)
    let plugin_toml = find_file_recursive(&tmp_dir, "plugin.toml").await?;
    let extract_dir = plugin_toml.parent().unwrap_or(&tmp_dir).to_path_buf();

    // If this is a binary plugin, verify the binary exists and make it executable
    if let Some(ref binary_name) = asset.binary {
        let binary_path = extract_dir.join(binary_name);
        if !binary_path.exists() {
            let alt_binary = tmp_dir.join(binary_name);
            if alt_binary.exists() {
                tokio::fs::copy(&alt_binary, &binary_path).await?;
            } else {
                let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
                bail!(
                    "Binary '{}' not found in archive. Contents: {:?}",
                    binary_name,
                    list_dir_contents(&tmp_dir).await
                );
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary_path, perms)?;
        }
    }

    // Install from extracted directory (reuse existing install logic)
    let extract_str = extract_dir.to_string_lossy().to_string();
    let result = super::PluginManager::install(&extract_str, Some(name), alias_override).await;

    // Cleanup temp dir
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;

    result
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Detect the current platform string matching registry keys.
fn current_platform() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" | "x86" => "x86_64",
        "aarch64" => "aarch64",
        _ => std::env::consts::ARCH,
    };
    format!("{os}-{arch}")
}

/// Recursively find a file by name in a directory.
async fn find_file_recursive(dir: &Path, filename: &str) -> Result<std::path::PathBuf> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_file() && path.file_name().map(|n| n == filename).unwrap_or(false) {
            return Ok(path);
        }
        if path.is_dir()
            && let Ok(found) = Box::pin(find_file_recursive(&path, filename)).await
        {
            return Ok(found);
        }
    }
    bail!("'{}' not found in {}", filename, dir.display())
}

/// List directory contents for error messages.
async fn list_dir_contents(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names
}
