use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Subcommand;

use lukan_core::config::{
    ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig, TOOL_GROUPS, WA_DEFAULT_TOOLS,
};
use lukan_plugins::{PluginChannel, PluginManager};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

use crate::plugin_config;
use crate::plugin_exec;
use crate::whatsapp_compat;

// ── Colors ─────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

// ── Base prompt ────────────────────────────────────────────────────────

const BASE_PROMPT: &str = include_str!("../prompts/base.txt");

// ── CLI subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PluginCommands {
    /// List installed plugins
    List,
    /// Install plugins (interactive picker or by name)
    Install {
        /// Plugin name (remote) or source directory path (local). Omit for interactive picker.
        source: Option<String>,
        /// Override plugin name
        #[arg(long)]
        name: Option<String>,
        /// Override CLI alias
        #[arg(long)]
        alias: Option<String>,
    },
    /// List plugins available in the remote registry
    ListRemote,
    /// Remove plugins (interactive picker or by name)
    Remove {
        /// Plugin name. Omit for interactive picker.
        name: Option<String>,
    },
    /// Start a plugin and run the channel loop
    Start {
        /// Plugin name
        name: String,
        /// Override LLM provider
        #[arg(long, short)]
        provider: Option<String>,
        /// Override model
        #[arg(long, short)]
        model: Option<String>,
    },
    /// Stop a running plugin
    Stop {
        /// Plugin name
        name: String,
    },
    /// Show plugin status and config
    Status {
        /// Plugin name
        name: String,
    },
    /// View plugin logs
    Logs {
        /// Plugin name
        name: String,
        /// Follow log output
        #[arg(long, short)]
        follow: bool,
        /// Number of lines to show
        #[arg(long, short = 'n', default_value = "50")]
        lines: String,
    },
    /// View or modify plugin configuration
    Config {
        /// Plugin name
        name: String,
        /// Config key (omit to show all)
        key: Option<String>,
        /// Action (depends on type: set/unset, add/remove/list/clear, on/off)
        action: Option<String>,
        /// Value for the action
        value: Option<String>,
    },
    /// Execute a custom plugin command
    Exec {
        /// Plugin name
        name: String,
        /// Command name (defined in plugin.toml [commands])
        command: String,
        /// Additional arguments
        args: Vec<String>,
    },
}

// ── Dispatch ───────────────────────────────────────────────────────────

pub async fn handle_plugin_command(command: PluginCommands) -> Result<()> {
    match command {
        PluginCommands::List => plugin_list().await,
        PluginCommands::Install {
            source,
            name,
            alias,
        } => {
            if let Some(ref src) = source {
                plugin_install(src, name.as_deref(), alias.as_deref()).await
            } else {
                plugin_install_interactive().await
            }
        }
        PluginCommands::ListRemote => plugin_list_remote().await,
        PluginCommands::Remove { name } => {
            if let Some(ref n) = name {
                plugin_remove(n).await
            } else {
                plugin_remove_interactive().await
            }
        }
        PluginCommands::Start {
            name,
            provider,
            model,
        } => plugin_start(&name, provider, model).await,
        PluginCommands::Stop { name } => plugin_stop(&name).await,
        PluginCommands::Status { name } => plugin_status(&name).await,
        PluginCommands::Logs {
            name,
            follow,
            lines,
        } => plugin_logs(&name, follow, &lines).await,
        PluginCommands::Config {
            name,
            key,
            action,
            value,
        } => {
            plugin_config::handle_plugin_config(
                &name,
                key.as_deref(),
                action.as_deref(),
                value.as_deref(),
            )
            .await
        }
        PluginCommands::Exec {
            name,
            command,
            args,
        } => plugin_exec::handle_plugin_exec(&name, &command, &args).await,
    }
}

// ── Command handlers ───────────────────────────────────────────────────

async fn plugin_list() -> Result<()> {
    let manager = PluginManager::new();
    let plugins = manager.list().await?;

    if plugins.is_empty() {
        println!("{YELLOW}No plugins installed.{RESET}");
        println!("{DIM}Install one with: lukan plugin install <path>{RESET}");
        return Ok(());
    }

    println!("{BOLD}Installed plugins:{RESET}\n");
    for p in &plugins {
        let status = if p.running {
            format!("{GREEN}running{RESET}")
        } else {
            format!("{DIM}stopped{RESET}")
        };
        let alias_str = match &p.alias {
            Some(a) => format!(" {DIM}(alias: {a}){RESET}"),
            None => String::new(),
        };
        println!(
            "  {CYAN}{}{RESET} v{} [{status}]{alias_str}",
            p.name, p.version
        );
        if !p.description.is_empty() {
            println!("    {DIM}{}{RESET}", p.description);
        }
    }

    Ok(())
}

async fn plugin_install_interactive() -> Result<()> {
    println!("{DIM}Fetching plugin registry...{RESET}");
    let plugins = lukan_plugins::registry::list_remote().await?;

    let installable: Vec<_> = plugins
        .iter()
        .filter(|p| p.available && !p.installed)
        .collect();

    if installable.is_empty() {
        println!("{YELLOW}All available plugins are already installed.{RESET}");
        return Ok(());
    }

    let items: Vec<String> = installable
        .iter()
        .map(|p| format!("{} — {}", p.name, p.description))
        .collect();

    let selections = dialoguer::MultiSelect::new()
        .with_prompt("Select plugins to install (Space to toggle, Enter to confirm)")
        .items(&items)
        .interact()?;

    if selections.is_empty() {
        println!("{DIM}No plugins selected.{RESET}");
        return Ok(());
    }

    for idx in selections {
        let p = installable[idx];
        print!("  Installing {CYAN}{}{RESET}... ", p.name);
        match lukan_plugins::registry::install_remote(&p.name, None).await {
            Ok(_) => println!("{GREEN}✓{RESET}"),
            Err(e) => println!("{RED}✗{RESET} {e}"),
        }
    }

    Ok(())
}

async fn plugin_remove_interactive() -> Result<()> {
    let manager = PluginManager::new();
    let plugins = manager.list().await?;

    if plugins.is_empty() {
        println!("{YELLOW}No plugins installed.{RESET}");
        return Ok(());
    }

    let items: Vec<String> = plugins
        .iter()
        .map(|p| {
            let status = if p.running { " (running)" } else { "" };
            format!("{}{status} — {}", p.name, p.description)
        })
        .collect();

    let selections = dialoguer::MultiSelect::new()
        .with_prompt("Select plugins to remove (Space to toggle, Enter to confirm)")
        .items(&items)
        .interact()?;

    if selections.is_empty() {
        println!("{DIM}No plugins selected.{RESET}");
        return Ok(());
    }

    for idx in selections {
        let p = &plugins[idx];
        print!("  Removing {CYAN}{}{RESET}... ", p.name);
        let mut mgr = PluginManager::new();
        match mgr.remove(&p.name).await {
            Ok(_) => println!("{GREEN}✓{RESET}"),
            Err(e) => println!("{RED}✗{RESET} {e}"),
        }
    }

    Ok(())
}

async fn plugin_install(source: &str, name: Option<&str>, alias: Option<&str>) -> Result<()> {
    let source_path = std::path::Path::new(source);

    // If source looks like a path (contains / or . or exists on disk), install locally.
    // Otherwise treat it as a remote plugin name from the registry.
    let is_local = source.contains('/') || source.contains('.') || source_path.exists();

    let installed_name = if is_local {
        PluginManager::install(source, name, alias).await?
    } else {
        println!("{DIM}Fetching plugin registry...{RESET}");
        lukan_plugins::registry::install_remote(source, alias).await?
    };

    println!("{GREEN}✓{RESET} Plugin '{CYAN}{installed_name}{RESET}' installed.");
    println!("{DIM}Start it with: lukan plugin start {installed_name}{RESET}");
    Ok(())
}

async fn plugin_list_remote() -> Result<()> {
    println!("{DIM}Fetching plugin registry...{RESET}");
    let plugins = lukan_plugins::registry::list_remote().await?;

    if plugins.is_empty() {
        println!("{YELLOW}No plugins available in the registry.{RESET}");
        return Ok(());
    }

    println!("{BOLD}Available plugins:{RESET}\n");
    for p in &plugins {
        let status = if p.installed {
            format!(" {GREEN}(installed){RESET}")
        } else if !p.available {
            format!(" {YELLOW}(not available for your platform){RESET}")
        } else {
            String::new()
        };
        let source_tag = match p.source.as_str() {
            "binary" => format!("{DIM}binary{RESET}"),
            "bundled" => format!("{DIM}bundled{RESET}"),
            other => format!("{DIM}{other}{RESET}"),
        };
        println!(
            "  {CYAN}{}{RESET} v{} [{source_tag}]{status}",
            p.name, p.version
        );
        println!("    {DIM}{}{RESET}", p.description);
    }

    println!("\n{DIM}Install with: lukan plugin install <name>{RESET}");
    Ok(())
}

async fn plugin_remove(name: &str) -> Result<()> {
    let mut manager = PluginManager::new();
    manager.remove(name).await?;
    println!("{GREEN}✓{RESET} Plugin '{CYAN}{name}{RESET}' removed.");
    Ok(())
}

async fn plugin_start(
    name: &str,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> Result<()> {
    // If not running as daemon, self-respawn detached and exit immediately
    if std::env::var("LUKAN_DAEMON").as_deref() != Ok("1") {
        return daemon_spawn(name, provider_override, model_override).await;
    }

    // ── Running as daemon ──────────────────────────────────────────────
    plugin_start_foreground(name, provider_override, model_override).await
}

/// Self-respawn the current binary as a detached daemon process.
/// Writes PID file, redirects stdout/stderr to the plugin log, and exits.
async fn daemon_spawn(
    name: &str,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> Result<()> {
    // Check if already running
    let pid_path = LukanPaths::plugin_pid(name);
    if let Ok(pid_str) = tokio::fs::read_to_string(&pid_path).await
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        #[cfg(unix)]
        {
            let alive = unsafe { libc::kill(pid, 0) == 0 };
            if alive {
                println!(
                    "{YELLOW}Plugin '{name}' is already running (PID {pid}).{RESET}\n\
                     {DIM}Use `lukan {name} stop` first, or `lukan {name} restart`.{RESET}"
                );
                return Ok(());
            }
        }
    }

    // Find our own binary path
    let self_exe = std::env::current_exe().context("Failed to find current executable")?;

    // Open log file for stdout/stderr of the daemon
    let log_path = LukanPaths::plugin_log(name);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;
    let log_file2 = log_file.try_clone()?;

    // Build args: `lukan plugin start <name> [--provider X] [--model Y]`
    let mut args = vec!["plugin".to_string(), "start".to_string(), name.to_string()];
    if let Some(ref p) = provider_override {
        args.push("--provider".to_string());
        args.push(p.clone());
    }
    if let Some(ref m) = model_override {
        args.push("--model".to_string());
        args.push(m.clone());
    }

    // Spawn detached in its own process group (setsid) so that
    // `kill(-pid, SIGTERM)` in plugin_stop kills daemon + all children.
    let mut cmd = std::process::Command::new(&self_exe);
    cmd.args(&args)
        .env("LUKAN_DAEMON", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file2));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // setsid — daemon becomes its own process group leader
    }

    let child = cmd.spawn().context("Failed to spawn daemon process")?;

    let pid = child.id();

    // Write PID file
    tokio::fs::write(&pid_path, pid.to_string()).await?;

    // Detach: drop the child handle without waiting
    std::mem::forget(child);

    println!("{GREEN}✓{RESET} Plugin '{CYAN}{name}{RESET}' daemon started (PID {pid})");
    println!("{DIM}Logs: {}{RESET}", log_path.display());
    println!("{DIM}Stop: lukan plugin stop {name}{RESET}");

    Ok(())
}

/// Run the plugin in foreground (called when LUKAN_DAEMON=1).
async fn plugin_start_foreground(
    name: &str,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> Result<()> {
    // Load config + credentials
    let mut config = ConfigManager::load().await?;
    let credentials = CredentialsManager::load().await?;

    // Check for per-plugin overrides in config
    let plugin_overrides = config
        .plugins
        .as_ref()
        .and_then(|p| p.overrides.get(name))
        .cloned();

    // Apply provider override: CLI > plugin config override > global
    if let Some(p) = provider_override.or_else(|| {
        plugin_overrides
            .as_ref()
            .and_then(|o| o.provider.as_ref())
            .map(|p| p.to_string())
    }) {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name")?;
    }

    // Apply model override: CLI > plugin config override > global
    if let Some(m) =
        model_override.or_else(|| plugin_overrides.as_ref().and_then(|o| o.model.clone()))
    {
        config.model = Some(m);
    }

    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    let provider = create_provider(&resolved)?;

    // Build tool registry — for whatsapp, use plugin config tools; otherwise, global overrides
    let mut registry = create_default_registry();
    let is_whatsapp = name == "whatsapp";

    if is_whatsapp {
        // Load WhatsApp plugin config for tool list
        let wa_config =
            whatsapp_compat::load_wa_config_for_plugin(plugin_overrides.as_ref()).await?;
        let tool_names: Vec<String> = wa_config
            .tools
            .clone()
            .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());
        let refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);
    } else if let Some(ref names) = plugin_overrides.as_ref().and_then(|o| o.tools.clone()) {
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);
    }

    // Collect the active tool names for hot-reload defaults
    let active_tool_names: Vec<String> = registry
        .definitions()
        .iter()
        .map(|d| d.name.clone())
        .collect();

    // Max response length
    let max_response_len = plugin_overrides.as_ref().and_then(|o| o.max_response_len);

    // Build system prompt — WhatsApp adds formatting/dir restrictions on top of the shared base
    let tz_name = resolved
        .config
        .timezone
        .clone()
        .unwrap_or_else(|| "UTC".to_string());
    let system_prompt = if is_whatsapp {
        let wa_config =
            whatsapp_compat::load_wa_config_for_plugin(plugin_overrides.as_ref()).await?;
        whatsapp_compat::build_whatsapp_system_prompt(&wa_config, Some(&tz_name)).await
    } else {
        let mut cached = vec![BASE_PROMPT.to_string()];
        // Load prompt.txt from installed plugins that provide tools
        let plugins_dir = LukanPaths::plugins_dir();
        if let Ok(mut entries) = tokio::fs::read_dir(&plugins_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let prompt_path = entry.path().join("prompt.txt");
                if let Ok(plugin_prompt) = tokio::fs::read_to_string(&prompt_path).await {
                    let trimmed = plugin_prompt.trim();
                    if !trimmed.is_empty() {
                        cached.push(trimmed.to_string());
                    }
                }
            }
        }
        let now = chrono::Utc::now();
        let dynamic = format!(
            "Current date: {} ({}). Use this for any time-relative operations.",
            now.format("%Y-%m-%d %H:%M UTC"),
            tz_name
        );
        SystemPrompt::Structured { cached, dynamic }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let agent_config = lukan_agent::AgentConfig {
        provider: Arc::from(provider),
        tools: registry,
        system_prompt,
        cwd,
        provider_name: resolved.config.provider.to_string(),
        model_name: resolved.effective_model(),
        bg_signal: None,
    };

    println!(
        "Starting plugin '{}' ({} with {})",
        name,
        resolved.config.provider,
        resolved.effective_model()
    );

    // Create agent
    let mut agent = lukan_agent::AgentLoop::new(agent_config).await?;

    // Start plugin process
    let mut manager = PluginManager::new();
    let (plugin_rx, host_tx) = manager.start(name).await?;

    println!("Plugin ready. Listening for messages...");

    // Run channel loop (blocks until plugin disconnects or error)
    let log_path = LukanPaths::plugin_log(name);
    let mut channel =
        PluginChannel::new(name, max_response_len, active_tool_names).with_log_file(log_path);
    channel.run(&mut agent, plugin_rx, host_tx).await?;

    // Cleanup
    manager.stop(name).await.ok();

    // Remove PID file on clean exit
    let pid_path = LukanPaths::plugin_pid(name);
    let _ = tokio::fs::remove_file(&pid_path).await;

    Ok(())
}

async fn plugin_stop(name: &str) -> Result<()> {
    // Check PID file
    let pid_path = LukanPaths::plugin_pid(name);
    match tokio::fs::read_to_string(&pid_path).await {
        Ok(pid_str) => {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                #[cfg(unix)]
                {
                    unsafe {
                        if libc::kill(-(pid as i32), libc::SIGTERM) != 0 {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                    }
                }
                let _ = tokio::fs::remove_file(&pid_path).await;
                println!("{GREEN}✓{RESET} Plugin '{CYAN}{name}{RESET}' stopped (PID {pid}).");
            }
        }
        Err(_) => {
            println!("{YELLOW}Plugin '{name}' is not running (no PID file).{RESET}");
        }
    }
    Ok(())
}

async fn plugin_status(name: &str) -> Result<()> {
    let dir = LukanPaths::plugin_dir(name);
    if !dir.exists() {
        println!("{RED}Plugin '{name}' not found.{RESET}");
        return Ok(());
    }

    let manifest = PluginManager::load_manifest(name).await?;

    // Check daemon status via PID
    let pid_path = LukanPaths::plugin_pid(name);
    let (daemon_running, daemon_pid) =
        if let Ok(pid_str) = tokio::fs::read_to_string(&pid_path).await {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                #[cfg(unix)]
                let alive = unsafe { libc::kill(pid, 0) == 0 };
                #[cfg(not(unix))]
                let alive = false;
                (alive, Some(pid))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

    // Load plugin config values
    let config = plugin_config::load_plugin_config(name)
        .await
        .unwrap_or_default();
    let config_obj = config.as_object();

    // Load global config for provider/model info
    let global_config = ConfigManager::load().await.ok();
    let plugin_overrides = global_config
        .as_ref()
        .and_then(|c| c.plugins.as_ref())
        .and_then(|p| p.overrides.get(name))
        .cloned();

    // Header
    println!("\n{BOLD}{CYAN}  {}{RESET}", manifest.plugin.name);
    println!("{DIM}  {}{RESET}\n", manifest.plugin.description);

    // Daemon status
    let daemon_str = if daemon_running {
        let pid = daemon_pid.unwrap_or(0);
        format!("{GREEN}running{RESET} {DIM}(PID {pid}){RESET}")
    } else {
        format!("{DIM}stopped{RESET}")
    };
    println!("  Daemon:      {daemon_str}");

    // Provider/model (plugin override > global)
    if let Some(ref gc) = global_config {
        let provider = plugin_overrides
            .as_ref()
            .and_then(|o| o.provider.as_ref())
            .map(|p| format!("{p}"))
            .unwrap_or_else(|| format!("{} {DIM}(global){RESET}", gc.provider));
        let model = plugin_overrides
            .as_ref()
            .and_then(|o| o.model.as_ref())
            .map(|m| m.to_string())
            .or_else(|| gc.model.clone())
            .unwrap_or_else(|| "(default)".to_string());
        println!("  Provider:    {provider}");
        println!("  Model:       {model}");
    }

    // Show config values driven by manifest schema
    if !manifest.config.is_empty() {
        let mut keys: Vec<&String> = manifest.config.keys().collect();
        keys.sort();

        for key in keys {
            let schema = &manifest.config[key];
            let camel = plugin_config::snake_to_camel(key);
            let current = config_obj.and_then(|obj| obj.get(&camel));

            // Grouped display for "tools" key — discover all tools dynamically
            if key == "tools" {
                let all_tools = lukan_tools::all_tool_names();
                let active_tools: Vec<String> = current
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        WA_DEFAULT_TOOLS
                            .iter()
                            .filter(|t| all_tools.iter().any(|a| a == **t))
                            .map(|s| s.to_string())
                            .collect()
                    });

                println!("  {BOLD}Tools:{RESET}");
                let mut seen = std::collections::HashSet::new();
                for (group_name, group_tools) in TOOL_GROUPS {
                    if !group_tools
                        .iter()
                        .any(|t| all_tools.iter().any(|a| a == *t))
                    {
                        continue;
                    }
                    let tool_strs: Vec<String> = group_tools
                        .iter()
                        .filter(|t| all_tools.iter().any(|a| a == **t))
                        .map(|t| {
                            seen.insert(t.to_string());
                            if active_tools.iter().any(|a| a == *t) {
                                format!("{GREEN}{t}{RESET}")
                            } else {
                                format!("{DIM}{t}{RESET}")
                            }
                        })
                        .collect();
                    println!(
                        "    {:<18} {}",
                        format!("{group_name}:"),
                        tool_strs.join(", ")
                    );
                }
                // Plugin-provided tools not in TOOL_GROUPS
                let ungrouped: Vec<String> = all_tools
                    .iter()
                    .filter(|t| !seen.contains(t.as_str()))
                    .map(|t| {
                        if active_tools.iter().any(|a| a == t) {
                            format!("{GREEN}{t}{RESET}")
                        } else {
                            format!("{DIM}{t}{RESET}")
                        }
                    })
                    .collect();
                if !ungrouped.is_empty() {
                    println!("    {:<18} {}", "Plugin:", ungrouped.join(", "));
                }
                continue;
            }

            let val_str = match current {
                Some(v) => plugin_config::format_value(v),
                None => match &schema.field_type {
                    lukan_core::models::plugin::ConfigFieldType::StringArray => {
                        format!("{DIM}(empty){RESET}")
                    }
                    lukan_core::models::plugin::ConfigFieldType::Bool => format!("{DIM}off{RESET}"),
                    _ => format!("{DIM}(not set){RESET}"),
                },
            };

            // Capitalize first letter of key for display
            let label = {
                let mut chars = key.chars();
                match chars.next() {
                    Some(c) => {
                        let display = key.replace('_', " ");
                        let mut chars2 = display.chars();
                        let first = chars2.next().unwrap_or(c);
                        format!("{}{}", first.to_uppercase(), chars2.collect::<String>())
                    }
                    None => key.clone(),
                }
            };

            // Pad label to align values
            println!("  {label:<14} {val_str}");
        }
    }

    // Custom commands
    if !manifest.commands.is_empty() {
        let cmds: Vec<&String> = manifest.commands.keys().collect();
        let alias = manifest.plugin.alias.as_deref().unwrap_or(name);
        println!(
            "\n{DIM}  Commands: {}{RESET}",
            cmds.iter()
                .map(|c| format!("lukan {alias} {c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    println!();
    Ok(())
}

async fn plugin_logs(name: &str, follow: bool, lines: &str) -> Result<()> {
    let log_file = LukanPaths::plugin_log(name);
    if !log_file.exists() {
        println!("{YELLOW}No log file found for plugin '{name}'.{RESET}");
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
