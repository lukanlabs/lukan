use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use lukan_core::config::LukanPaths;
use lukan_core::models::plugin::{HostMessage, PROTOCOL_VERSION, PluginManifest, PluginMessage};
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
    pub alias: Option<String>,
    /// Activity bar contribution from manifest (icon + label for sidebar)
    pub activity_bar: Option<lukan_core::models::plugin::ActivityBarContribution>,
    /// Views declared by the plugin
    pub views: Vec<lukan_core::models::plugin::ViewDeclaration>,
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
        let mut manifest: PluginManifest = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        manifest.inject_security_config();
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

        // Verify runtime dependencies before starting
        verify_runtime_deps(&manifest)?;

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
        let ready = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            while let Some(msg) = plugin_rx.recv().await {
                if let PluginMessage::Ready {
                    version,
                    capabilities,
                } = &msg
                {
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
                if let PluginMessage::Error {
                    message,
                    recoverable,
                } = &msg
                {
                    error!(plugin = %name, "Plugin error during init: {message}");
                    if !recoverable {
                        anyhow::bail!("Plugin '{}' non-recoverable error: {}", name, message);
                    }
                }
            }
            anyhow::bail!("Plugin '{}' closed stdout without sending Ready", name)
        })
        .await
        .map_err(|_| anyhow::anyhow!("Plugin '{}' did not send Ready within 60s", name))??;

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
                        alias: manifest.plugin.alias,
                        activity_bar: manifest.plugin.activity_bar,
                        views: manifest.plugin.views,
                    });
                }
                Err(e) => {
                    infos.push(PluginInfo {
                        name: name.clone(),
                        version: "?".to_string(),
                        description: format!("(error loading manifest: {e})"),
                        plugin_type: "unknown".to_string(),
                        running: false,
                        alias: None,
                        activity_bar: None,
                        views: Vec::new(),
                    });
                }
            }
        }

        Ok(infos)
    }

    /// Reserved command names that cannot be used as plugin aliases.
    pub const RESERVED_COMMANDS: &[&str] = &[
        "chat",
        "setup",
        "doctor",
        "codex-auth",
        "copilot-auth",
        "google-auth",
        "models",
        "plugin",
        "sandbox",
    ];

    /// Install a plugin from a local directory (copies files to plugins dir).
    /// If `alias_override` is provided, it replaces the alias in the installed plugin.toml.
    pub async fn install(
        source: &str,
        name: Option<&str>,
        alias_override: Option<&str>,
    ) -> Result<String> {
        let raw_path = Path::new(source);
        if !raw_path.exists() {
            anyhow::bail!("Source path does not exist: {source}");
        }

        // Prefer dist/ (bundled) over source if it exists
        let dist_path = raw_path.join("dist");
        let source_path = if dist_path.join("plugin.toml").exists() {
            info!("Using bundled dist/ for install");
            dist_path.as_path()
        } else {
            raw_path
        };

        // Validate source has plugin.toml
        let manifest_path = source_path.join("plugin.toml");
        if !manifest_path.exists() {
            anyhow::bail!("No plugin.toml found in {source}");
        }

        // Parse manifest to get the plugin name
        let content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: PluginManifest =
            toml::from_str(&content).context("Failed to parse plugin.toml")?;

        let plugin_name = name.unwrap_or(&manifest.plugin.name);

        // Determine effective alias
        let effective_alias = alias_override.or(manifest.plugin.alias.as_deref());

        // Validate alias against reserved commands and other plugins
        if let Some(alias) = effective_alias {
            if Self::RESERVED_COMMANDS.contains(&alias) {
                anyhow::bail!(
                    "Alias '{alias}' conflicts with a reserved command.\n\
                     Use --alias <other> to choose a different alias."
                );
            }

            // Check for conflicts with other installed plugins
            let manager = PluginManager::new();
            let existing = manager.discover().await?;
            for existing_name in &existing {
                if existing_name == plugin_name {
                    continue; // Skip self (re-install case)
                }
                if let Ok(existing_manifest) = Self::load_manifest(existing_name).await
                    && existing_manifest.plugin.alias.as_deref() == Some(alias)
                {
                    anyhow::bail!(
                        "Alias '{alias}' already used by plugin '{existing_name}'.\n\
                         Use --alias <other> to choose a different alias."
                    );
                }
            }
        }

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

        // If alias_override was provided, update the installed plugin.toml
        if let Some(alias) = alias_override {
            let installed_manifest_path = dest.join("plugin.toml");
            let mut toml_content = tokio::fs::read_to_string(&installed_manifest_path).await?;

            // Simple replacement: if alias already exists, replace it; otherwise add it
            if toml_content.contains("alias =") {
                // Replace existing alias line
                let mut new_content = String::new();
                for line in toml_content.lines() {
                    if line.trim_start().starts_with("alias") && line.contains('=') {
                        new_content.push_str(&format!("alias = \"{alias}\""));
                    } else {
                        new_content.push_str(line);
                    }
                    new_content.push('\n');
                }
                toml_content = new_content;
            } else {
                // Add alias after the [plugin] section header
                toml_content =
                    toml_content.replace("[plugin]\n", &format!("[plugin]\nalias = \"{alias}\"\n"));
            }

            tokio::fs::write(&installed_manifest_path, toml_content).await?;
        }

        // Post-install: run npm/bun install if package.json exists (recursively)
        run_post_install(&dest).await?;

        // Check runtime dependencies
        check_runtime_deps(&manifest);

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

/// Recursively copy a directory, skipping node_modules and build artifacts.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip directories that will be regenerated by post-install
        if name_str == "node_modules" || name_str == "target" || name_str == ".git" {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

/// Run post-install steps: detect package.json files and run npm/bun install.
async fn run_post_install(plugin_dir: &Path) -> Result<()> {
    // Find all directories containing package.json (plugin root + subdirs)
    let mut dirs_with_pkg: Vec<std::path::PathBuf> = Vec::new();
    collect_package_dirs(plugin_dir, &mut dirs_with_pkg).await;

    if dirs_with_pkg.is_empty() {
        return Ok(());
    }

    // Detect package manager: prefer bun > npm
    let pm = if which_exists("bun") {
        "bun"
    } else if which_exists("npm") {
        "npm"
    } else {
        // No package manager available, warn but don't fail
        eprintln!(
            "\x1b[33m  Warning: plugin has Node.js dependencies but neither bun nor npm found.\n  \
             Install them and run `npm install` manually in the plugin directory.\x1b[0m"
        );
        return Ok(());
    };

    for dir in &dirs_with_pkg {
        let relative = dir
            .strip_prefix(plugin_dir)
            .unwrap_or(dir)
            .display()
            .to_string();
        let label = if relative.is_empty() { "." } else { &relative };
        eprintln!("  Installing dependencies ({label})...");

        let status = tokio::process::Command::new(pm)
            .arg("install")
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
            .await;

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "\x1b[33m  Warning: `{pm} install` failed in {label} (exit {})\x1b[0m",
                    s.code().unwrap_or(-1)
                );
            }
            Err(e) => {
                eprintln!("\x1b[33m  Warning: failed to run `{pm} install` in {label}: {e}\x1b[0m");
            }
        }
    }

    Ok(())
}

/// Recursively find directories containing package.json.
async fn collect_package_dirs(dir: &Path, result: &mut Vec<std::path::PathBuf>) {
    if dir.join("package.json").exists() {
        result.push(dir.to_path_buf());
    }
    if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() && name_str != "node_modules" && name_str != ".git" {
                Box::pin(collect_package_dirs(&path, result)).await;
            }
        }
    }
}

/// Check if a command exists in PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Install-time check: warn if runtime dependencies are missing (non-blocking).
fn check_runtime_deps(manifest: &PluginManifest) {
    let Some(ref run) = manifest.run else {
        return;
    };

    let cmd = &run.command;

    // Skip checking for plugin binaries that live inside the plugin dir
    if cmd.starts_with("lukan-") || cmd.starts_with("./") {
        return;
    }

    if !which_exists(cmd) {
        let hint = install_hint(cmd);
        eprintln!(
            "\n\x1b[33m  Warning: this plugin requires '{cmd}' but it was not found in PATH.\x1b[0m"
        );
        if let Some(h) = hint {
            eprintln!("\x1b[33m  Install it: {h}\x1b[0m");
        }
        eprintln!();
    }
}

/// Start-time check: fail if runtime dependencies are missing (blocking).
fn verify_runtime_deps(manifest: &PluginManifest) -> Result<()> {
    let Some(ref run) = manifest.run else {
        return Ok(());
    };

    let cmd = &run.command;

    if cmd.starts_with("lukan-") || cmd.starts_with("./") {
        return Ok(());
    }

    if !which_exists(cmd) {
        let hint = install_hint(cmd);
        let mut msg = format!(
            "Plugin '{}' requires '{cmd}' but it was not found in PATH.",
            manifest.plugin.name
        );
        if let Some(h) = hint {
            msg.push_str(&format!("\nInstall it: {h}"));
        }
        anyhow::bail!(msg);
    }

    Ok(())
}

/// Suggest how to install a missing dependency.
fn install_hint(cmd: &str) -> Option<&'static str> {
    match cmd {
        "node" => Some(
            "https://nodejs.org or `curl -fsSL https://fnm.vercel.app/install | bash && fnm install --lts`",
        ),
        "python3" | "python" => Some("https://python.org or `sudo apt install python3`"),
        "bun" => Some("https://bun.sh or `curl -fsSL https://bun.sh/install | bash`"),
        "deno" => Some("https://deno.land or `curl -fsSL https://deno.land/install.sh | sh`"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PluginManager basics ─────────────────────────────────────────

    #[test]
    fn test_plugin_manager_new_has_no_running() {
        let manager = PluginManager::new();
        assert!(!manager.is_running("anything"));
    }

    #[test]
    fn test_plugin_manager_default() {
        let manager = PluginManager::default();
        assert!(!manager.is_running("test"));
    }

    #[test]
    fn test_is_running_returns_false_for_unknown() {
        let manager = PluginManager::new();
        assert!(!manager.is_running("nonexistent"));
        assert!(!manager.is_running(""));
        assert!(!manager.is_running("whatsapp"));
    }

    // ── RESERVED_COMMANDS ────────────────────────────────────────────

    #[test]
    fn test_reserved_commands_contains_expected() {
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"chat"));
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"setup"));
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"doctor"));
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"plugin"));
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"models"));
        assert!(PluginManager::RESERVED_COMMANDS.contains(&"sandbox"));
    }

    #[test]
    fn test_reserved_commands_does_not_contain_plugin_names() {
        assert!(!PluginManager::RESERVED_COMMANDS.contains(&"whatsapp"));
        assert!(!PluginManager::RESERVED_COMMANDS.contains(&"discord"));
        assert!(!PluginManager::RESERVED_COMMANDS.contains(&"telegram"));
    }

    #[test]
    fn test_reserved_commands_non_empty() {
        assert!(!PluginManager::RESERVED_COMMANDS.is_empty());
    }

    // ── install_hint ─────────────────────────────────────────────────

    #[test]
    fn test_install_hint_node() {
        let hint = install_hint("node");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("nodejs.org"));
    }

    #[test]
    fn test_install_hint_python() {
        assert!(install_hint("python3").is_some());
        assert!(install_hint("python").is_some());
        // Both should give the same hint
        assert_eq!(install_hint("python3"), install_hint("python"));
    }

    #[test]
    fn test_install_hint_bun() {
        let hint = install_hint("bun");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("bun.sh"));
    }

    #[test]
    fn test_install_hint_deno() {
        let hint = install_hint("deno");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("deno.land"));
    }

    #[test]
    fn test_install_hint_unknown_returns_none() {
        assert!(install_hint("ruby").is_none());
        assert!(install_hint("cargo").is_none());
        assert!(install_hint("").is_none());
        assert!(install_hint("some-random-binary").is_none());
    }

    // ── PluginManifest parsing via manager (TOML) ────────────────────

    #[test]
    fn test_manifest_parse_with_alias() {
        let toml_str = r#"
            [plugin]
            name = "discord"
            version = "0.1.0"
            description = "Discord bot"
            alias = "dc"

            [run]
            command = "node"
            args = ["bridge.js"]
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.alias.as_deref(), Some("dc"));
    }

    #[test]
    fn test_manifest_parse_with_commands() {
        let toml_str = r#"
            [plugin]
            name = "test"
            version = "0.1.0"

            [commands.auth]
            description = "Authenticate"
            handler = "auth.js"

            [commands.status]
            description = "Show status"
            handler = "status.js"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.commands.len(), 2);
        assert!(manifest.commands.contains_key("auth"));
        assert_eq!(manifest.commands["auth"].handler, "auth.js");
        assert_eq!(manifest.commands["status"].description, "Show status");
    }

    #[test]
    fn test_manifest_parse_with_security() {
        let toml_str = r#"
            [plugin]
            name = "secure-plugin"
            version = "0.1.0"

            [security]
            default_tools = ["Grep", "Glob", "ReadFiles"]
            include_memory = true
            dir_restrictions = true
            dangerous_tools = ["Bash", "WriteFile"]

            [security.prompts]
            dir_none = "prompt-none.txt"
            dir_allowed = "prompt-allowed.txt"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.security.include_memory);
        assert!(manifest.security.dir_restrictions);
        assert_eq!(manifest.security.default_tools.len(), 3);
        assert_eq!(manifest.security.dangerous_tools, vec!["Bash", "WriteFile"]);
        assert_eq!(
            manifest.security.prompts.dir_none.as_deref(),
            Some("prompt-none.txt")
        );
        assert_eq!(
            manifest.security.prompts.dir_allowed.as_deref(),
            Some("prompt-allowed.txt")
        );
    }

    #[test]
    fn test_manifest_security_defaults() {
        let toml_str = r#"
            [plugin]
            name = "minimal"
            version = "0.1.0"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert!(!manifest.security.include_memory);
        assert!(!manifest.security.dir_restrictions);
        assert!(manifest.security.default_tools.is_empty());
        // When [security] section is absent entirely, Default trait is used (empty vec)
        assert!(manifest.security.dangerous_tools.is_empty());
        assert!(manifest.security.prompts.dir_none.is_none());
        assert!(manifest.security.prompts.dir_allowed.is_none());
    }

    #[test]
    fn test_manifest_parse_with_auth_qr() {
        let toml_str = r#"
            [plugin]
            name = "wa"
            version = "0.1.0"

            [auth]
            type = "qr"
            qr_file = "my-qr.txt"
            status_file = "my-creds.json"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        match manifest.auth.unwrap() {
            lukan_core::models::plugin::AuthDeclaration::Qr {
                qr_file,
                status_file,
            } => {
                assert_eq!(qr_file, "my-qr.txt");
                assert_eq!(status_file, "my-creds.json");
            }
            other => panic!("Expected Qr, got {:?}", other),
        }
    }

    #[test]
    fn test_manifest_parse_with_auth_token() {
        let toml_str = r#"
            [plugin]
            name = "discord"
            version = "0.1.0"

            [auth]
            type = "token"
            check_field = "botToken"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        match manifest.auth.unwrap() {
            lukan_core::models::plugin::AuthDeclaration::Token { check_field } => {
                assert_eq!(check_field, "botToken");
            }
            other => panic!("Expected Token, got {:?}", other),
        }
    }

    #[test]
    fn test_manifest_parse_with_run_env() {
        let toml_str = r#"
            [plugin]
            name = "test"
            version = "0.1.0"

            [run]
            command = "node"
            args = ["index.js"]

            [run.env]
            NODE_ENV = "production"
            DEBUG = "false"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let run = manifest.run.unwrap();
        assert_eq!(run.env.len(), 2);
        assert_eq!(run.env["NODE_ENV"], "production");
        assert_eq!(run.env["DEBUG"], "false");
    }

    #[test]
    fn test_manifest_parse_run_defaults() {
        let toml_str = r#"
            [plugin]
            name = "test"
            version = "0.1.0"

            [run]
            command = "python3"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let run = manifest.run.unwrap();
        assert_eq!(run.command, "python3");
        assert!(run.args.is_empty());
        assert!(run.env.is_empty());
        assert!(run.handler.is_none());
    }

    #[test]
    fn test_manifest_parse_with_config_fields() {
        let toml_str = r#"
            [plugin]
            name = "test"
            version = "0.1.0"

            [config.api_key]
            type = "string"
            description = "API key"
            format = "password"
            group = "Auth"
            order = 0

            [config.channels]
            type = "string[]"
            description = "Channel IDs"
            group = "Access"
            order = 1

            [config.port]
            type = "number"
            description = "Port number"
            default = 8080

            [config.debug]
            type = "bool"
            description = "Enable debug mode"
            default = false
            hidden = true
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.config.len(), 4);

        let api_key = &manifest.config["api_key"];
        assert_eq!(
            api_key.field_type,
            lukan_core::models::plugin::ConfigFieldType::String
        );
        assert_eq!(api_key.format.as_deref(), Some("password"));
        assert_eq!(api_key.group.as_deref(), Some("Auth"));
        assert_eq!(api_key.order, 0);
        assert!(!api_key.hidden);

        let channels = &manifest.config["channels"];
        assert_eq!(
            channels.field_type,
            lukan_core::models::plugin::ConfigFieldType::StringArray
        );

        let port = &manifest.config["port"];
        assert_eq!(
            port.field_type,
            lukan_core::models::plugin::ConfigFieldType::Number
        );
        assert_eq!(port.default, Some(serde_json::json!(8080)));

        let debug = &manifest.config["debug"];
        assert_eq!(
            debug.field_type,
            lukan_core::models::plugin::ConfigFieldType::Bool
        );
        assert!(debug.hidden);
        assert_eq!(debug.default, Some(serde_json::json!(false)));
    }

    #[test]
    fn test_manifest_parse_with_depends_on() {
        let toml_str = r#"
            [plugin]
            name = "test"
            version = "0.1.0"

            [config.backend]
            type = "string"
            description = "Backend type"
            valid_values = ["local", "cloud"]

            [config.server_url]
            type = "string"
            description = "Server URL"

            [config.server_url.depends_on]
            field = "backend"
            values = ["cloud"]
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let server_url = &manifest.config["server_url"];
        let dep = server_url.depends_on.as_ref().unwrap();
        assert_eq!(dep.field, "backend");
        assert_eq!(dep.values, vec!["cloud"]);

        // backend should have no depends_on
        assert!(manifest.config["backend"].depends_on.is_none());
    }

    #[test]
    fn test_manifest_parse_with_contributions() {
        let toml_str = r#"
            [plugin]
            name = "whisper"
            version = "0.1.0"

            [contributions.transcription]
            port_field = "port"
            default_port = 9999
            endpoint = "/transcribe"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let tc = manifest.contributions.transcription.unwrap();
        assert_eq!(tc.port_field, "port");
        assert_eq!(tc.default_port, 9999);
        assert_eq!(tc.endpoint, "/transcribe");
    }

    #[test]
    fn test_manifest_parse_with_views() {
        let toml_str = r#"
            [plugin]
            name = "dashboard"
            version = "0.1.0"

            [plugin.activity_bar]
            icon = "container"
            label = "Dashboard"

            [[plugin.views]]
            id = "overview"
            type = "panel"
            label = "Overview"

            [[plugin.views]]
            id = "logs"
            type = "panel"
            label = "Logs"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        let ab = manifest.plugin.activity_bar.unwrap();
        assert_eq!(ab.icon, "container");
        assert_eq!(ab.label, "Dashboard");
        assert_eq!(manifest.plugin.views.len(), 2);
        assert_eq!(manifest.plugin.views[0].id, "overview");
        assert_eq!(manifest.plugin.views[0].view_type, "panel");
        assert_eq!(manifest.plugin.views[1].id, "logs");
    }

    #[test]
    fn test_manifest_parse_invalid_toml() {
        let toml_str = "this is not valid toml {{{}}}";
        let result = toml::from_str::<PluginManifest>(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_parse_missing_name_fails() {
        let toml_str = r#"
            [plugin]
            version = "0.1.0"
        "#;
        let result = toml::from_str::<PluginManifest>(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_parse_missing_version_fails() {
        let toml_str = r#"
            [plugin]
            name = "test"
        "#;
        let result = toml::from_str::<PluginManifest>(toml_str);
        assert!(result.is_err());
    }
}
