use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Subcommand;

use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig, WA_DEFAULT_TOOLS};
use lukan_plugins::{PluginChannel, PluginManager};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

use crate::whatsapp;

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
    /// Install a plugin from a local directory
    Install {
        /// Source directory containing plugin.toml
        source: String,
        /// Override plugin name
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove an installed plugin
    Remove {
        /// Plugin name
        name: String,
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
}

// ── Dispatch ───────────────────────────────────────────────────────────

pub async fn handle_plugin_command(command: PluginCommands) -> Result<()> {
    match command {
        PluginCommands::List => plugin_list().await,
        PluginCommands::Install { source, name } => plugin_install(&source, name.as_deref()).await,
        PluginCommands::Remove { name } => plugin_remove(&name).await,
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
        println!(
            "  {CYAN}{}{RESET} v{} [{status}]",
            p.name, p.version
        );
        if !p.description.is_empty() {
            println!("    {DIM}{}{RESET}", p.description);
        }
    }

    Ok(())
}

async fn plugin_install(source: &str, name: Option<&str>) -> Result<()> {
    let installed_name = PluginManager::install(source, name).await?;
    println!("{GREEN}✓{RESET} Plugin '{CYAN}{installed_name}{RESET}' installed.");
    println!("{DIM}Start it with: lukan plugin start {installed_name}{RESET}");
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
    if let Some(p) = provider_override
        .or_else(|| plugin_overrides.as_ref().and_then(|o| o.provider.as_ref()).map(|p| p.to_string()))
    {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name")?;
    }

    // Apply model override: CLI > plugin config override > global
    if let Some(m) = model_override
        .or_else(|| plugin_overrides.as_ref().and_then(|o| o.model.clone()))
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
        let wa_config = whatsapp::load_wa_config_for_plugin(plugin_overrides.as_ref()).await?;
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

    // Max response length
    let max_response_len = plugin_overrides.as_ref().and_then(|o| o.max_response_len);

    // Build system prompt — WhatsApp gets its specialized prompt with dir restrictions
    let system_prompt = if is_whatsapp {
        let wa_config = whatsapp::load_wa_config_for_plugin(plugin_overrides.as_ref()).await?;
        whatsapp::build_whatsapp_system_prompt(&wa_config)
    } else {
        SystemPrompt::Text(BASE_PROMPT.to_string())
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
        "{GREEN}✓{RESET} Starting plugin '{CYAN}{name}{RESET}' ({} with {})",
        resolved.config.provider,
        resolved.effective_model()
    );

    // Create agent
    let mut agent = lukan_agent::AgentLoop::new(agent_config).await?;

    // Start plugin process
    let mut manager = PluginManager::new();
    let (plugin_rx, host_tx) = manager.start(name).await?;

    println!("{GREEN}✓{RESET} Plugin ready. Listening for messages...");

    // Run channel loop
    let channel = PluginChannel::new(name, max_response_len);
    channel.run(&mut agent, plugin_rx, host_tx).await?;

    // Cleanup
    manager.stop(name).await.ok();

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

    // Check running status via PID
    let pid_path = LukanPaths::plugin_pid(name);
    let running = if let Ok(pid_str) = tokio::fs::read_to_string(&pid_path).await {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            #[cfg(unix)]
            {
                unsafe { libc::kill(pid, 0) == 0 }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    let status = if running {
        format!("{GREEN}running{RESET}")
    } else {
        format!("{DIM}stopped{RESET}")
    };

    println!("{BOLD}Plugin: {CYAN}{}{RESET}", manifest.plugin.name);
    println!("  Version:     {}", manifest.plugin.version);
    println!("  Type:        {}", manifest.plugin.plugin_type);
    println!("  Description: {}", manifest.plugin.description);
    println!("  Status:      {status}");
    println!("  Command:     {} {}", manifest.run.command, manifest.run.args.join(" "));
    println!("  Directory:   {}", dir.display());

    // Show config if present
    let config_path = LukanPaths::plugin_config(name);
    if config_path.exists() {
        println!("  Config:      {}", config_path.display());
    }

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
