use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use lukan_core::config::{
    ConfigManager, LukanPaths, PluginOverrides, PluginsConfig, ProviderName, ResolvedConfig,
    WA_ALL_TOOLS, WA_DEFAULT_TOOLS, WhatsAppConfig,
};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

// ── Colors ─────────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

// ── WhatsApp prompts (embedded at compile time) ────────────────────────────

const WA_FORMAT_PROMPT: &str = include_str!("../prompts/whatsapp-format.txt");
const WA_DIR_NONE_PROMPT: &str = include_str!("../prompts/whatsapp-dir-none.txt");
const WA_DIR_ALLOWED_PROMPT: &str = include_str!("../prompts/whatsapp-dir-allowed.txt");
const BASE_PROMPT: &str = include_str!("../prompts/base.txt");

// ── WhatsApp Plugin Config (lives in ~/.config/lukan/plugins/whatsapp/config.json) ──

/// Plugin-specific configuration for WhatsApp.
/// Lives in ~/.config/lukan/plugins/whatsapp/config.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WhatsAppPluginConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_dirs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_dir_restrictions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_advance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_chat: Option<String>,
}

const WA_PLUGIN_NAME: &str = "whatsapp";

/// Load the WhatsApp plugin's config.json
async fn load_wa_plugin_config() -> Result<WhatsAppPluginConfig> {
    let config_path = LukanPaths::plugin_config(WA_PLUGIN_NAME);
    if !config_path.exists() {
        return Ok(WhatsAppPluginConfig::default());
    }
    let content = tokio::fs::read_to_string(&config_path)
        .await
        .context("Failed to read WhatsApp plugin config")?;
    let config: WhatsAppPluginConfig =
        serde_json::from_str(&content).context("Failed to parse WhatsApp plugin config")?;
    Ok(config)
}

/// Save the WhatsApp plugin's config.json
async fn save_wa_plugin_config(config: &WhatsAppPluginConfig) -> Result<()> {
    let plugin_dir = LukanPaths::plugin_dir(WA_PLUGIN_NAME);
    tokio::fs::create_dir_all(&plugin_dir).await?;
    let config_path = LukanPaths::plugin_config(WA_PLUGIN_NAME);
    let content = serde_json::to_string_pretty(config)?;
    tokio::fs::write(&config_path, content)
        .await
        .context("Failed to write WhatsApp plugin config")?;
    Ok(())
}

/// Check that the WhatsApp plugin is installed (has plugin.toml).
/// Returns Ok if installed, Err with user-friendly message if not.
fn ensure_wa_plugin_installed() -> Result<()> {
    let manifest_path = LukanPaths::plugin_manifest(WA_PLUGIN_NAME);
    if !manifest_path.exists() {
        anyhow::bail!(
            "WhatsApp plugin not installed.\n\
             Install it with: lukan plugin install <path-to-whatsapp-plugin>"
        );
    }
    Ok(())
}

/// Convert WhatsAppPluginConfig to the legacy WhatsAppConfig (for system prompt builder etc.)
fn plugin_config_to_wa_config(
    pc: &WhatsAppPluginConfig,
    overrides: Option<&PluginOverrides>,
) -> WhatsAppConfig {
    WhatsAppConfig {
        enabled: pc.enabled,
        bridge_url: pc.bridge_url.clone(),
        whitelist: pc.whitelist.clone(),
        allowed_groups: pc.allowed_groups.clone(),
        prefix: pc.prefix.clone(),
        tools: pc.tools.clone(),
        allowed_dirs: pc.allowed_dirs.clone(),
        skip_dir_restrictions: pc.skip_dir_restrictions,
        provider: overrides.and_then(|o| o.provider.clone()),
        model: overrides.and_then(|o| o.model.clone()),
        reminder_advance: pc.reminder_advance,
        reminder_chat: pc.reminder_chat.clone(),
    }
}

/// Load WhatsApp plugin config and build a WhatsAppConfig suitable for system prompt building.
/// Called by plugin.rs when starting the "whatsapp" plugin.
pub async fn load_wa_config_for_plugin(overrides: Option<&PluginOverrides>) -> Result<WhatsAppConfig> {
    let pc = load_wa_plugin_config().await?;
    Ok(plugin_config_to_wa_config(&pc, overrides))
}

// ── WaCommands enum ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum WaCommands {
    /// Enable WhatsApp channel
    On,
    /// Disable WhatsApp channel
    Off,
    /// Add a phone number to the whitelist
    Allow {
        /// Phone number (digits only)
        number: String,
    },
    /// Remove a phone number from the whitelist
    Deny {
        /// Phone number to remove
        number: String,
    },
    /// Manage allowed groups (add/remove/list)
    Group {
        /// Action: add, remove, or list
        action: String,
        /// Group ID (e.g. 123456789-987654321@g.us)
        group_id: Option<String>,
    },
    /// Get or set the command prefix
    Prefix {
        /// Prefix value (omit to show current, use "none" to remove)
        value: Option<String>,
    },
    /// Show current WhatsApp configuration
    Status,
    /// Manage allowed tools (list/add/remove/reset)
    Tools {
        /// Action: list, add, remove, reset
        action: Option<String>,
        /// Tool name
        tool_name: Option<String>,
    },
    /// Manage allowed directories for file access
    Dir {
        /// Action: list, add, remove, clear, off, on
        action: Option<String>,
        /// Directory path
        dir_path: Option<String>,
    },
    /// List available WhatsApp groups from the connector
    Groups,
    /// Select the WhatsApp channel model interactively
    Model,
    /// Authenticate WhatsApp by scanning QR code
    Auth,
    /// Delete WhatsApp session (requires QR scan on next start)
    Logout,
    /// Start WhatsApp plugin as a background process
    Start {
        /// Override provider
        #[arg(long, short)]
        provider: Option<String>,
        /// Override model
        #[arg(long, short)]
        model: Option<String>,
    },
    /// Stop the WhatsApp plugin
    Stop,
    /// Restart the WhatsApp plugin
    Restart,
    /// View WhatsApp plugin logs
    Logs {
        /// Follow log output
        #[arg(long, short)]
        follow: bool,
        /// Number of lines to show
        #[arg(long, short = 'n', default_value = "50")]
        lines: String,
    },
    /// Get or set reminder notification advance (minutes)
    ReminderAdvance {
        /// Minutes before reminder
        minutes: Option<String>,
    },
    /// Get or set chat ID for proactive reminder notifications
    ReminderChat {
        /// Chat ID
        chat_id: Option<String>,
    },
}

// ── run_whatsapp: interactive channel ─────────────────────────────────────

pub async fn run_whatsapp(
    provider_override: Option<String>,
    model_override: Option<String>,
    no_connector: bool,
    resolved: &ResolvedConfig,
) -> Result<()> {
    // Load from plugin config (with fallback to legacy global config for backward compat)
    let pc = if LukanPaths::plugin_manifest(WA_PLUGIN_NAME).exists() {
        load_wa_plugin_config().await?
    } else {
        // Backward compat: use legacy global config
        let wa = resolved.config.whatsapp.clone().unwrap_or_default();
        WhatsAppPluginConfig {
            enabled: wa.enabled,
            bridge_url: wa.bridge_url,
            whitelist: wa.whitelist,
            allowed_groups: wa.allowed_groups,
            prefix: wa.prefix,
            tools: wa.tools,
            allowed_dirs: wa.allowed_dirs,
            skip_dir_restrictions: wa.skip_dir_restrictions,
            reminder_advance: wa.reminder_advance,
            reminder_chat: wa.reminder_chat,
        }
    };

    // Check auth
    let auth_dir = LukanPaths::whatsapp_auth_dir();
    let creds_file = auth_dir.join("creds.json");
    if !creds_file.exists() {
        println!("{RED}WhatsApp not authenticated.{RESET}");
        println!("Authenticate first:\n");
        println!("  lukan wa auth\n");
        return Ok(());
    }

    let connector_url = pc
        .bridge_url
        .clone()
        .unwrap_or_else(|| "ws://localhost:3001".to_string());

    // Auto-start connector if needed
    if !no_connector && !check_connector_running(&connector_url).await {
        println!("{DIM}Starting whatsapp-connector...{RESET}");
        start_connector_process().await?;
        // Give it time to start
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    // Read overrides from global config
    let wa_overrides = resolved
        .config
        .plugins
        .as_ref()
        .and_then(|p| p.overrides.get(WA_PLUGIN_NAME))
        .cloned()
        .unwrap_or_default();

    // Determine provider/model (CLI > plugin overrides > global)
    let mut config = resolved.config.clone();
    if let Some(p) = provider_override.or(wa_overrides.provider.as_ref().map(|p| p.to_string())) {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name")?;
    }
    if let Some(m) = model_override.or(wa_overrides.model.clone()) {
        config.model = Some(m);
    }

    let wa_resolved = ResolvedConfig {
        config,
        credentials: resolved.credentials.clone(),
    };

    let provider = create_provider(&wa_resolved)?;

    // Build filtered tool registry
    let tool_names: Vec<String> = pc
        .tools
        .clone()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());
    let tool_refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
    let mut registry = create_default_registry();
    registry.retain(&tool_refs);

    // Build system prompt (convert to legacy WhatsAppConfig for the prompt builder)
    let wa_config = plugin_config_to_wa_config(&pc, Some(&wa_overrides));
    let system_prompt = build_whatsapp_system_prompt(&wa_config);

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let agent_config = lukan_agent::AgentConfig {
        provider: Arc::from(provider),
        tools: registry,
        system_prompt,
        cwd,
        provider_name: wa_resolved.config.provider.to_string(),
        model_name: wa_resolved.effective_model(),
        bg_signal: None,
    };

    println!(
        "{GREEN}✓{RESET} WhatsApp channel started ({} with {})",
        wa_resolved.config.provider,
        wa_resolved.effective_model()
    );

    lukan_agent::whatsapp_channel::start_whatsapp_channel(agent_config, &wa_config).await?;

    Ok(())
}

// ── handle_wa_command: dispatch all wa subcommands ────────────────────────

pub async fn handle_wa_command(command: WaCommands) -> Result<()> {
    match command {
        WaCommands::On => wa_on().await,
        WaCommands::Off => wa_off().await,
        WaCommands::Allow { number } => wa_allow(&number).await,
        WaCommands::Deny { number } => wa_deny(&number).await,
        WaCommands::Group { action, group_id } => wa_group(&action, group_id.as_deref()).await,
        WaCommands::Prefix { value } => wa_prefix(value.as_deref()).await,
        WaCommands::Status => wa_status().await,
        WaCommands::Tools { action, tool_name } => {
            wa_tools(action.as_deref(), tool_name.as_deref()).await
        }
        WaCommands::Dir { action, dir_path } => {
            wa_dir(action.as_deref(), dir_path.as_deref()).await
        }
        WaCommands::Groups => wa_groups().await,
        WaCommands::Model => wa_model().await,
        WaCommands::Auth => wa_auth().await,
        WaCommands::Logout => wa_logout().await,
        WaCommands::Start { provider, model } => wa_start(provider, model).await,
        WaCommands::Stop => wa_stop().await,
        WaCommands::Restart => wa_restart().await,
        WaCommands::Logs { follow, lines } => wa_logs(follow, &lines).await,
        WaCommands::ReminderAdvance { minutes } => wa_reminder_advance(minutes.as_deref()).await,
        WaCommands::ReminderChat { chat_id } => wa_reminder_chat(chat_id.as_deref()).await,
    }
}

// ── Individual command handlers ──────────────────────────────────────────

async fn wa_on() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    pc.enabled = Some(true);
    save_wa_plugin_config(&pc).await?;
    println!("{GREEN}✓{RESET} WhatsApp channel enabled.");
    Ok(())
}

async fn wa_off() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    pc.enabled = Some(false);
    save_wa_plugin_config(&pc).await?;

    // Stop plugin if running
    if kill_plugin().await {
        println!("{GREEN}✓{RESET} Plugin stopped.");
    }

    println!("{GREEN}✓{RESET} WhatsApp channel disabled. Configuration preserved.");
    Ok(())
}

async fn wa_allow(number: &str) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    let mut whitelist = pc.whitelist.take().unwrap_or_default();

    let clean: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
    if whitelist.contains(&clean) {
        println!("{YELLOW}{clean} already in whitelist.{RESET}");
    } else {
        whitelist.push(clean.clone());
        pc.whitelist = Some(whitelist);
        save_wa_plugin_config(&pc).await?;
        println!("{GREEN}✓{RESET} Added {clean} to whitelist.");
    }
    Ok(())
}

async fn wa_deny(number: &str) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    let mut whitelist = pc.whitelist.take().unwrap_or_default();

    let clean: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Some(idx) = whitelist.iter().position(|w| w == &clean) {
        whitelist.remove(idx);
        pc.whitelist = Some(whitelist);
        save_wa_plugin_config(&pc).await?;
        println!("{GREEN}✓{RESET} Removed {clean} from whitelist.");
    } else {
        println!("{YELLOW}{clean} not in whitelist.{RESET}");
    }
    Ok(())
}

async fn wa_group(action: &str, group_id: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    let mut groups = pc.allowed_groups.take().unwrap_or_default();

    match action {
        "list" => {
            if groups.is_empty() {
                println!("{YELLOW}No allowed groups configured.{RESET}");
            } else {
                println!("{BOLD}Allowed groups:{RESET}");
                for (i, g) in groups.iter().enumerate() {
                    println!("  {}) {CYAN}{g}{RESET}", i + 1);
                }
            }
        }
        "add" => {
            let gid = group_id.ok_or_else(|| {
                anyhow::anyhow!("Usage: wa group add <groupId@g.us>")
            })?;
            if groups.contains(&gid.to_string()) {
                println!("{YELLOW}{gid} already allowed.{RESET}");
            } else {
                groups.push(gid.to_string());
                pc.allowed_groups = Some(groups);
                save_wa_plugin_config(&pc).await?;
                println!("{GREEN}✓{RESET} Added group {gid}.");
            }
        }
        "remove" => {
            let gid = group_id.ok_or_else(|| {
                anyhow::anyhow!("Usage: wa group remove <groupId@g.us>")
            })?;
            if let Some(idx) = groups.iter().position(|g| g == gid) {
                groups.remove(idx);
                pc.allowed_groups = Some(groups);
                save_wa_plugin_config(&pc).await?;
                println!("{GREEN}✓{RESET} Removed group {gid}.");
            } else {
                println!("{YELLOW}{gid} not in allowed groups.{RESET}");
            }
        }
        _ => {
            println!("{RED}Unknown action. Use: add, remove, or list{RESET}");
        }
    }
    Ok(())
}

async fn wa_prefix(value: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;

    match value {
        None => {
            let pfx = pc.prefix.as_deref().unwrap_or("(none)");
            println!("Prefix: {pfx}");
        }
        Some("none") | Some("") => {
            pc.prefix = None;
            save_wa_plugin_config(&pc).await?;
            println!(
                "{GREEN}✓{RESET} Prefix removed. All messages from whitelisted users will be processed."
            );
        }
        Some(v) => {
            pc.prefix = Some(v.to_string());
            save_wa_plugin_config(&pc).await?;
            println!("{GREEN}✓{RESET} Prefix set to \"{v}\".");
        }
    }
    Ok(())
}

async fn wa_status() -> Result<()> {
    // Check if plugin is installed
    let installed = LukanPaths::plugin_manifest(WA_PLUGIN_NAME).exists();
    if !installed {
        println!("{RED}WhatsApp plugin not installed.{RESET}");
        println!("{DIM}Install it with: lukan plugin install <path-to-whatsapp-plugin>{RESET}");
        return Ok(());
    }

    let pc = load_wa_plugin_config().await?;
    let config = ConfigManager::load().await?;

    let whitelist = pc.whitelist.clone().unwrap_or_default();
    let groups = pc.allowed_groups.clone().unwrap_or_default();
    let tools: Vec<String> = pc
        .tools
        .clone()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());

    // Plugin process status (check plugin PID file)
    let plugin_status = match read_pid_and_check(&LukanPaths::plugin_pid(WA_PLUGIN_NAME)).await {
        Some(pid) => format!("{GREEN}running{RESET} (PID {pid})"),
        None => format!("{RED}stopped{RESET}"),
    };

    // Auth status
    let auth_dir = LukanPaths::whatsapp_auth_dir();
    let authed = auth_dir.join("creds.json").exists();

    // Connector status
    let connector_url = pc.bridge_url.as_deref().unwrap_or("ws://localhost:3001");
    let connector_up = check_connector_running(connector_url).await;

    let enabled = pc.enabled.unwrap_or(true);
    println!("{BOLD}WhatsApp Status:{RESET}");
    println!(
        "  Channel:     {}",
        if enabled {
            format!("{GREEN}enabled{RESET}")
        } else {
            format!("{RED}disabled{RESET}")
        }
    );
    println!("  Plugin:      {plugin_status}");
    println!(
        "  Connector:   {} ({connector_url})",
        if connector_up {
            format!("{GREEN}connected{RESET}")
        } else {
            format!("{RED}not running{RESET}")
        }
    );
    println!(
        "  Auth:        {}",
        if authed {
            format!("{GREEN}authenticated{RESET}")
        } else {
            format!("{YELLOW}not authenticated{RESET}")
        }
    );

    // Read provider/model from global plugin overrides
    let wa_overrides = config
        .plugins
        .as_ref()
        .and_then(|p| p.overrides.get(WA_PLUGIN_NAME))
        .cloned()
        .unwrap_or_default();

    let wa_provider = wa_overrides
        .provider
        .as_ref()
        .unwrap_or(&config.provider);
    let wa_model = wa_overrides.model.as_deref().unwrap_or(
        config
            .model
            .as_deref()
            .unwrap_or(config.provider.default_model()),
    );
    let is_channel_specific = wa_overrides.provider.is_some() || wa_overrides.model.is_some();
    let suffix = if is_channel_specific {
        ""
    } else {
        &format!(" {DIM}(global){RESET}")
    };
    println!("  Provider:    {wa_provider}{suffix}");
    println!("  Model:       {wa_model}{suffix}");
    println!(
        "  Prefix:      {}",
        pc.prefix.as_deref().unwrap_or("(none)")
    );
    println!(
        "  Whitelist:   {}",
        if whitelist.is_empty() {
            "(empty)".to_string()
        } else {
            whitelist.join(", ")
        }
    );
    println!(
        "  Groups:      {}",
        if groups.is_empty() {
            "(empty)".to_string()
        } else {
            groups.join(", ")
        }
    );
    println!("  Tools:       {}", tools.join(", "));

    let has_dangerous = tools
        .iter()
        .any(|t| t == "Bash" || t == "WriteFile" || t == "EditFile");
    let dirs = pc.allowed_dirs.clone().unwrap_or_default();
    if pc.skip_dir_restrictions.unwrap_or(false) {
        println!("  Allowed dirs: {YELLOW}skipped (unrestricted){RESET}");
    } else if has_dangerous {
        if dirs.is_empty() {
            println!("  Allowed dirs: {RED}none (all file access blocked){RESET}");
        } else {
            println!("  Allowed dirs: {}", dirs.join(", "));
        }
    } else {
        println!("  Allowed dirs: {DIM}n/a (no dangerous tools){RESET}");
    }
    println!(
        "  Reminders:   {}min advance → {}",
        pc.reminder_advance.unwrap_or(15),
        pc.reminder_chat.as_deref().unwrap_or("(auto)")
    );

    Ok(())
}

async fn wa_tools(action: Option<&str>, tool_name: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    let mut current: Vec<String> = pc
        .tools
        .take()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());

    match action.unwrap_or("list") {
        "list" => {
            println!("{BOLD}WhatsApp agent tools:{RESET}");
            for t in WA_ALL_TOOLS {
                let enabled = current.iter().any(|c| c == t);
                let mark = if enabled {
                    format!("{GREEN} ✓{RESET}")
                } else {
                    format!("{RED} ✗{RESET}")
                };
                let is_default = if WA_DEFAULT_TOOLS.contains(t) {
                    format!(" {DIM}(default){RESET}")
                } else {
                    String::new()
                };
                println!("{mark} {t}{is_default}");
            }
            println!("\n{DIM}Usage: wa tools add|remove <name> | wa tools reset{RESET}");
        }
        "reset" => {
            pc.tools = None;
            save_wa_plugin_config(&pc).await?;
            let defaults: Vec<&str> = WA_DEFAULT_TOOLS.to_vec();
            println!("{GREEN}✓{RESET} Reset to defaults: {}", defaults.join(", "));
        }
        "add" => {
            let name = tool_name
                .ok_or_else(|| anyhow::anyhow!("Usage: wa tools add <name>"))?;
            if !WA_ALL_TOOLS.contains(&name) {
                println!("{RED}Unknown tool: {name}{RESET}");
                println!(
                    "{DIM}Available: {}{RESET}",
                    WA_ALL_TOOLS.join(", ")
                );
                return Ok(());
            }
            if current.iter().any(|c| c == name) {
                println!("{YELLOW}{name} already enabled.{RESET}");
            } else {
                current.push(name.to_string());
                pc.tools = Some(current.clone());
                save_wa_plugin_config(&pc).await?;
                let joined: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
                println!("{GREEN}✓{RESET} Enabled {name}. Active tools: {}", joined.join(", "));
            }
        }
        "remove" => {
            let name = tool_name
                .ok_or_else(|| anyhow::anyhow!("Usage: wa tools remove <name>"))?;
            if let Some(idx) = current.iter().position(|c| c == name) {
                current.remove(idx);
                pc.tools = Some(current.clone());
                save_wa_plugin_config(&pc).await?;
                let joined: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
                println!(
                    "{GREEN}✓{RESET} Disabled {name}. Active tools: {}",
                    joined.join(", ")
                );
            } else {
                println!("{YELLOW}{name} not enabled.{RESET}");
            }
        }
        other => {
            println!("{RED}Unknown action: {other}. Use: list, add, remove, reset{RESET}");
        }
    }
    Ok(())
}

async fn wa_dir(action: Option<&str>, dir_path: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;
    let mut current = pc.allowed_dirs.take().unwrap_or_default();

    match action.unwrap_or("list") {
        "list" => {
            if pc.skip_dir_restrictions.unwrap_or(false) {
                println!("{YELLOW}Directory restrictions are OFF (unrestricted file access).{RESET}");
                println!("{DIM}Use: wa dir on — to re-enable restrictions{RESET}");
            } else if current.is_empty() {
                println!(
                    "{RED}No allowed directories configured. All file access is blocked.{RESET}"
                );
                println!("{DIM}Use: wa dir add <path> — to allow a directory{RESET}");
                println!("{DIM}Use: wa dir off — to disable restrictions entirely{RESET}");
            } else {
                println!("{BOLD}Allowed directories:{RESET}");
                for d in &current {
                    println!("  {CYAN}{d}{RESET}");
                }
            }
            println!("\n{DIM}Usage: wa dir add|remove <path> | wa dir clear | wa dir off|on{RESET}");
        }
        "off" => {
            pc.skip_dir_restrictions = Some(true);
            pc.allowed_dirs = if current.is_empty() {
                None
            } else {
                Some(current)
            };
            save_wa_plugin_config(&pc).await?;
            println!(
                "{GREEN}✓{RESET} Directory restrictions disabled. Full file access granted."
            );
        }
        "on" => {
            pc.skip_dir_restrictions = None;
            pc.allowed_dirs = if current.is_empty() {
                None
            } else {
                Some(current.clone())
            };
            save_wa_plugin_config(&pc).await?;
            if !current.is_empty() {
                println!(
                    "{GREEN}✓{RESET} Directory restrictions enabled. Allowed: {}",
                    current.join(", ")
                );
            } else {
                println!("{GREEN}✓{RESET} Directory restrictions enabled. No directories allowed — add some with: wa dir add <path>");
            }
        }
        "clear" => {
            pc.allowed_dirs = None;
            save_wa_plugin_config(&pc).await?;
            println!(
                "{GREEN}✓{RESET} Allowed directories cleared. All file access is now blocked."
            );
            println!("{DIM}Use: wa dir add <path> — to allow a directory{RESET}");
            println!("{DIM}Use: wa dir off — to disable restrictions entirely{RESET}");
        }
        "add" => {
            let dir = dir_path
                .ok_or_else(|| anyhow::anyhow!("Usage: wa dir add <path>"))?;
            let resolved = std::fs::canonicalize(dir)
                .unwrap_or_else(|_| std::path::PathBuf::from(dir));
            let resolved_str = resolved.to_string_lossy().to_string();

            if !resolved.is_dir() {
                println!("{RED}Not a valid directory: {resolved_str}{RESET}");
                return Ok(());
            }
            if current.contains(&resolved_str) {
                println!("{YELLOW}{resolved_str} already in allowed dirs.{RESET}");
            } else {
                current.push(resolved_str.clone());
                pc.allowed_dirs = Some(current.clone());
                save_wa_plugin_config(&pc).await?;
                println!(
                    "{GREEN}✓{RESET} Added {resolved_str}. Allowed dirs: {}",
                    current.join(", ")
                );
            }
        }
        "remove" => {
            let dir = dir_path
                .ok_or_else(|| anyhow::anyhow!("Usage: wa dir remove <path>"))?;
            let resolved = std::fs::canonicalize(dir)
                .unwrap_or_else(|_| std::path::PathBuf::from(dir));
            let resolved_str = resolved.to_string_lossy().to_string();

            if let Some(idx) = current.iter().position(|d| d == &resolved_str) {
                current.remove(idx);
                pc.allowed_dirs = if current.is_empty() {
                    None
                } else {
                    Some(current.clone())
                };
                save_wa_plugin_config(&pc).await?;
                if !current.is_empty() {
                    println!(
                        "{GREEN}✓{RESET} Removed {resolved_str}. Allowed dirs: {}",
                        current.join(", ")
                    );
                } else {
                    println!("{GREEN}✓{RESET} Removed {resolved_str}. No directories allowed — all file access is blocked.");
                }
            } else {
                println!("{YELLOW}{resolved_str} not in allowed dirs.{RESET}");
            }
        }
        other => {
            println!("{RED}Unknown action: {other}. Use: list, add, remove, clear, off, on{RESET}");
        }
    }
    Ok(())
}

async fn wa_groups() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let pc = load_wa_plugin_config().await?;
    let connector_url = pc
        .bridge_url
        .as_deref()
        .unwrap_or("ws://localhost:3001");
    let allowed = pc.allowed_groups.clone().unwrap_or_default();

    println!("Connecting to connector at {connector_url}...");

    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let ws_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        connect_async(connector_url),
    )
    .await;

    let (ws_stream, _) = match ws_result {
        Ok(Ok(ws)) => ws,
        _ => {
            println!("{RED}Could not connect to connector. Is it running?{RESET}");
            return Ok(());
        }
    };

    let (mut writer, mut reader) = ws_stream.split();

    // Request groups list
    let cmd = serde_json::json!({"type": "list_groups"});
    writer
        .send(WsMessage::Text(cmd.to_string().into()))
        .await?;

    // Wait for response
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(msg) = reader.next().await {
            if let Ok(WsMessage::Text(text)) = msg
                && let Ok(data) = serde_json::from_str::<serde_json::Value>(&text)
                && data.get("type").and_then(|t| t.as_str()) == Some("groups")
            {
                return Some(data);
            }
        }
        None
    })
    .await;

    match timeout {
        Ok(Some(data)) => {
            let groups = data["groups"].as_array();
            match groups {
                Some(groups) if groups.is_empty() => {
                    println!("{YELLOW}No groups found.{RESET}");
                }
                Some(groups) => {
                    println!("{BOLD}{} groups available:{RESET}\n", groups.len());
                    for g in groups {
                        let id = g["id"].as_str().unwrap_or("");
                        let subject = g["subject"].as_str().unwrap_or("");
                        let participants = g["participants"].as_u64().unwrap_or(0);
                        let mark = if allowed.iter().any(|a| a == id) {
                            format!("{GREEN} ✓{RESET}")
                        } else {
                            "  ".to_string()
                        };
                        println!("{mark} {CYAN}{id}{RESET}");
                        println!("    {subject} ({participants} members)\n");
                    }
                    println!("{DIM}Use: lukan wa group add <id>{RESET}");
                }
                None => {
                    println!("{YELLOW}No groups found.{RESET}");
                }
            }
        }
        _ => {
            println!("{RED}Timeout waiting for groups list from connector.{RESET}");
        }
    }

    Ok(())
}

async fn wa_model() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let config = ConfigManager::load().await?;

    // Read current overrides from global config
    let wa_overrides = config
        .plugins
        .as_ref()
        .and_then(|p| p.overrides.get(WA_PLUGIN_NAME))
        .cloned()
        .unwrap_or_default();

    let current_entry = match (&wa_overrides.provider, &wa_overrides.model) {
        (Some(p), Some(m)) => Some(format!("{p}:{m}")),
        _ => None,
    };

    let global_default = format!(
        "{}:{}",
        config.provider,
        config.model.as_deref().unwrap_or(config.provider.default_model())
    );

    // Get available models from config
    let models = config
        .models
        .clone()
        .unwrap_or_else(|| vec![global_default.clone()]);

    // Build choices
    let mut choices: Vec<String> = models.clone();
    choices.push(format!("__global__ (Use global default: {global_default})"));

    println!("{BOLD}Select model for WhatsApp:{RESET}");
    for (i, choice) in choices.iter().enumerate() {
        let current = match &current_entry {
            Some(entry) if *entry == choices[i] => " ← current",
            None if choices[i].starts_with("__global__") => " ← current",
            _ => "",
        };
        println!("  {}) {choice}{current}", i + 1);
    }

    print!("\nChoice (1-{}): ", choices.len());
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let idx: usize = input.trim().parse().unwrap_or(0);

    if idx == 0 || idx > choices.len() {
        println!("{RED}Invalid choice.{RESET}");
        return Ok(());
    }

    let mut config = config;

    if idx == choices.len() {
        // Global default selected — remove whatsapp overrides for provider/model
        let plugins = config.plugins.get_or_insert_with(PluginsConfig::default);
        if let Some(ovr) = plugins.overrides.get_mut(WA_PLUGIN_NAME) {
            ovr.provider = None;
            ovr.model = None;
        }
        ConfigManager::save(&config).await?;
        println!(
            "{GREEN}✓{RESET} WhatsApp model reset to global default ({CYAN}{global_default}{RESET})"
        );
    } else {
        let entry = &models[idx - 1];
        if let Some(colon) = entry.find(':') {
            let provider_str = &entry[..colon];
            let model = &entry[colon + 1..];
            let provider: ProviderName =
                serde_json::from_value(serde_json::Value::String(provider_str.to_string()))
                    .context("Invalid provider in model entry")?;
            let plugins = config.plugins.get_or_insert_with(PluginsConfig::default);
            let ovr = plugins
                .overrides
                .entry(WA_PLUGIN_NAME.to_string())
                .or_default();
            ovr.provider = Some(provider);
            ovr.model = Some(model.to_string());
            ConfigManager::save(&config).await?;
            println!("{GREEN}✓{RESET} WhatsApp model set to {CYAN}{entry}{RESET}");
        } else {
            println!("{RED}Invalid model format (expected provider:model){RESET}");
        }
    }

    // Signal running plugin to hot-reload
    if let Some(pid) = read_pid_file(&LukanPaths::plugin_pid(WA_PLUGIN_NAME)).await {
        #[cfg(unix)]
        {
            unsafe { libc::kill(pid as i32, libc::SIGUSR1) };
            println!("{GREEN}✓{RESET} Plugin notified — model switched live (PID {pid})");
        }
    } else {
        println!("{DIM}No plugin running. Changes will apply on next start.{RESET}");
    }

    Ok(())
}

async fn wa_auth() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let auth_dir = LukanPaths::whatsapp_auth_dir();
    let creds_file = auth_dir.join("creds.json");

    if creds_file.exists() {
        println!("{GREEN}✓{RESET} WhatsApp already authenticated.");
        println!("{DIM}To re-authenticate, run: lukan wa logout{RESET}");
        return Ok(());
    }

    println!("Starting connector for QR authentication...\n");

    let (cmd, args, cwd) = get_connector_command();
    let mut child = std::process::Command::new(&cmd)
        .args(&args)
        .current_dir(&cwd)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("Failed to start whatsapp-connector")?;

    // Poll for auth file
    let start = std::time::Instant::now();
    let max_wait = std::time::Duration::from_secs(120);
    let mut authenticated = false;

    while start.elapsed() < max_wait {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if creds_file.exists() {
            authenticated = true;
            break;
        }
    }

    let _ = child.kill();

    if authenticated {
        println!("\n{GREEN}✓{RESET} WhatsApp authenticated successfully!");
        println!("\nStart the daemon with:");
        println!("  lukan wa start");
    } else {
        println!("\n{RED}✗{RESET} Authentication timed out (2 minutes).");
        println!("Try again with: lukan wa auth");
    }

    Ok(())
}

async fn wa_logout() -> Result<()> {
    // Stop plugin if running
    if kill_plugin().await {
        println!("{DIM}Stopped plugin.{RESET}");
    }

    let auth_dir = LukanPaths::whatsapp_auth_dir();
    match tokio::fs::remove_dir_all(&auth_dir).await {
        Ok(_) => {
            println!("{GREEN}✓{RESET} WhatsApp session deleted.");
            println!("{DIM}Run: lukan wa auth{RESET}");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("{YELLOW}No WhatsApp session found.{RESET}");
        }
        Err(e) => {
            println!("{RED}Failed to delete session: {e}{RESET}");
        }
    }

    Ok(())
}

async fn wa_start(provider: Option<String>, model: Option<String>) -> Result<()> {
    ensure_wa_plugin_installed()?;

    // Check auth
    let auth_dir = LukanPaths::whatsapp_auth_dir();
    if !auth_dir.join("creds.json").exists() {
        println!("{RED}WhatsApp not authenticated.{RESET}");
        println!("Authenticate first:\n");
        println!("  lukan wa auth\n");
        return Ok(());
    }

    // Check if already running
    if let Some(pid) = read_pid_and_check(&LukanPaths::plugin_pid(WA_PLUGIN_NAME)).await {
        println!("{YELLOW}WhatsApp plugin already running (PID {pid}).{RESET}");
        println!("{DIM}Use: lukan wa stop{RESET}");
        return Ok(());
    }

    // Delegate to plugin start — spawns lukan as a daemon running "plugin start whatsapp"
    let exe = std::env::current_exe().context("Failed to get current exe")?;
    let log_file = LukanPaths::plugin_log(WA_PLUGIN_NAME);
    let pid_file = LukanPaths::plugin_pid(WA_PLUGIN_NAME);

    let plugin_dir = LukanPaths::plugin_dir(WA_PLUGIN_NAME);
    tokio::fs::create_dir_all(&plugin_dir).await?;

    let mut args = vec![
        "plugin".to_string(),
        "start".to_string(),
        WA_PLUGIN_NAME.to_string(),
    ];
    if let Some(p) = provider {
        args.push("-p".to_string());
        args.push(p);
    }
    if let Some(m) = model {
        args.push("-m".to_string());
        args.push(m);
    }

    let log_fd = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .context("Failed to open log file")?;
    let log_fd2 = log_fd.try_clone()?;

    let child = std::process::Command::new(exe)
        .args(&args)
        .stdout(log_fd)
        .stderr(log_fd2)
        .stdin(std::process::Stdio::null())
        .spawn()
        .context("Failed to start WhatsApp plugin daemon")?;

    let pid = child.id();
    tokio::fs::write(&pid_file, pid.to_string()).await?;

    println!("{GREEN}✓{RESET} WhatsApp plugin started (PID {pid})");
    println!("{DIM}Logs: {}{RESET}", log_file.display());
    println!("{DIM}Stop: lukan wa stop{RESET}");

    Ok(())
}

async fn wa_stop() -> Result<()> {
    ensure_wa_plugin_installed()?;
    let pid_file = LukanPaths::plugin_pid(WA_PLUGIN_NAME);

    match read_pid_file(&pid_file).await {
        Some(pid) => {
            kill_process(pid);
            let _ = tokio::fs::remove_file(&pid_file).await;
            println!("{GREEN}✓{RESET} WhatsApp plugin stopped (PID {pid}).");
        }
        None => {
            println!("{YELLOW}No plugin running (no PID file).{RESET}");
        }
    }

    Ok(())
}

async fn wa_restart() -> Result<()> {
    ensure_wa_plugin_installed()?;
    // Stop existing
    let pid_file = LukanPaths::plugin_pid(WA_PLUGIN_NAME);
    if let Some(pid) = read_pid_file(&pid_file).await {
        kill_process(pid);
        let _ = tokio::fs::remove_file(&pid_file).await;
        println!("{DIM}Stopped plugin (PID {pid}){RESET}");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Start again using wa_start with no overrides
    wa_start(None, None).await
}

async fn wa_logs(follow: bool, lines: &str) -> Result<()> {
    let log_file = LukanPaths::plugin_log(WA_PLUGIN_NAME);
    if !log_file.exists() {
        println!("{YELLOW}No log file found for WhatsApp plugin.{RESET}");
        return Ok(());
    }

    let mut cmd_args = vec!["-n".to_string(), lines.to_string()];
    if follow {
        cmd_args.push("-f".to_string());
    }
    cmd_args.push(log_file.to_string_lossy().to_string());

    let status = std::process::Command::new("tail")
        .args(&cmd_args)
        .status()
        .context("Failed to run tail")?;

    if !status.success() {
        println!("{RED}tail exited with error{RESET}");
    }

    Ok(())
}

async fn wa_reminder_advance(minutes: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;

    match minutes {
        None => {
            println!(
                "Reminder advance: {} minutes",
                pc.reminder_advance.unwrap_or(15)
            );
        }
        Some(m) => {
            let n: u32 = m
                .parse()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid value. Use a positive number of minutes.")
                })?;
            pc.reminder_advance = Some(n);
            save_wa_plugin_config(&pc).await?;
            println!("{GREEN}✓{RESET} Reminder advance set to {n} minutes.");
        }
    }
    Ok(())
}

async fn wa_reminder_chat(chat_id: Option<&str>) -> Result<()> {
    ensure_wa_plugin_installed()?;
    let mut pc = load_wa_plugin_config().await?;

    match chat_id {
        None => {
            println!(
                "Reminder chat: {}",
                pc.reminder_chat
                    .as_deref()
                    .unwrap_or("(auto — first allowed group or whitelist)")
            );
        }
        Some(id) => {
            pc.reminder_chat = Some(id.to_string());
            save_wa_plugin_config(&pc).await?;
            println!("{GREEN}✓{RESET} Reminder notifications will be sent to {id}");
        }
    }
    Ok(())
}

// ── System prompt builder ───────────────────────────────────────────────

pub fn build_whatsapp_system_prompt(wa_config: &WhatsAppConfig) -> SystemPrompt {
    let mut prompt = String::new();
    prompt.push_str(BASE_PROMPT);
    prompt.push_str("\n\n");
    prompt.push_str(WA_FORMAT_PROMPT);

    // Directory restrictions
    let tools: Vec<String> = wa_config
        .tools
        .clone()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());

    let has_dangerous = tools
        .iter()
        .any(|t| t == "Bash" || t == "WriteFile" || t == "EditFile");

    if has_dangerous && !wa_config.skip_dir_restrictions.unwrap_or(false) {
        let dirs = wa_config.allowed_dirs.clone().unwrap_or_default();
        if dirs.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(WA_DIR_NONE_PROMPT);
        } else {
            prompt.push_str("\n\n");
            let dir_list = dirs
                .iter()
                .map(|d| format!("- `{d}`"))
                .collect::<Vec<_>>()
                .join("\n");
            prompt.push_str(&WA_DIR_ALLOWED_PROMPT.replace("{{ALLOWED_DIRS}}", &dir_list));
        }
    }

    SystemPrompt::Text(prompt)
}

// ── Connector/daemon helpers ────────────────────────────────────────────

/// Get the command to start the whatsapp-connector.
fn get_connector_command() -> (String, Vec<String>, std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().unwrap_or(Path::new("."));

    // Check locations — plugin dir first, then relative to binary, then CWD
    let plugin_dir = LukanPaths::plugin_dir(WA_PLUGIN_NAME);
    let connector_locations = [
        // Inside the plugin directory (e.g. connector bundled with plugin)
        plugin_dir.join("whatsapp-connector"),
        // Sibling of the plugin directory
        LukanPaths::plugins_dir().join("whatsapp-connector"),
        // Relative to the lukan binary
        exe_dir.join("../whatsapp-connector"),
        exe_dir.join("../../whatsapp-connector"),
        // CWD
        std::path::PathBuf::from("whatsapp-connector"),
    ];

    for loc in &connector_locations {
        let index = loc.join("index.js");
        if index.exists() {
            return (
                "node".to_string(),
                vec![index.to_string_lossy().to_string()],
                loc.clone(),
            );
        }
    }

    // Fallback: assume it's in the current working directory
    let cwd = std::env::current_dir().unwrap_or_default();
    let fallback = cwd.join("whatsapp-connector");
    (
        "node".to_string(),
        vec![fallback.join("index.js").to_string_lossy().to_string()],
        fallback,
    )
}

/// Check if the connector is running by attempting a quick WebSocket connection.
async fn check_connector_running(url: &str) -> bool {
    use tokio_tungstenite::connect_async;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        connect_async(url),
    )
    .await;

    matches!(result, Ok(Ok(_)))
}

/// Start the connector as a background daemon process.
async fn start_connector_process() -> Result<()> {
    let (cmd, args, cwd) = get_connector_command();

    let child = std::process::Command::new(&cmd)
        .args(&args)
        .current_dir(&cwd)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("Failed to start whatsapp-connector")?;

    // Save PID
    let pid = child.id();
    let pid_file = LukanPaths::whatsapp_connector_pid_file();
    tokio::fs::write(&pid_file, pid.to_string()).await?;

    Ok(())
}

/// Kill the WhatsApp plugin process using its PID file.
async fn kill_plugin() -> bool {
    let pid_file = LukanPaths::plugin_pid(WA_PLUGIN_NAME);
    if let Some(pid) = read_pid_file(&pid_file).await {
        kill_process(pid);
        let _ = tokio::fs::remove_file(&pid_file).await;
        true
    } else {
        false
    }
}

/// Read a PID from a file.
async fn read_pid_file(path: &std::path::Path) -> Option<u32> {
    tokio::fs::read_to_string(path)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Read a PID file and check if the process is still alive.
async fn read_pid_and_check(path: &std::path::Path) -> Option<u32> {
    let pid = read_pid_file(path).await?;
    if process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Check if a process is alive.
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Kill a process by PID.
fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        // Try process group first, then individual process
        unsafe {
            if libc::kill(-(pid as i32), libc::SIGTERM) != 0 {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}
