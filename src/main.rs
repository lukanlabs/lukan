use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig};
use lukan_plugins::PluginManager;
use lukan_providers::create_provider;
use lukan_tui::app::App;

mod models;
mod plugin;
mod plugin_config;
mod plugin_exec;
mod sandbox_cmd;
mod setup;
mod whatsapp_compat;

#[derive(Parser)]
#[command(name = "lukan", version, about = "AI agent CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override the LLM provider
    #[arg(long, short)]
    provider: Option<String>,

    /// Override the model
    #[arg(long, short)]
    model: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive chat (default)
    Chat {
        /// Override the LLM provider
        #[arg(long, short)]
        provider: Option<String>,
        /// Override the model
        #[arg(long, short)]
        model: Option<String>,
        /// UI mode: tui (default) or web
        #[arg(long, default_value = "tui")]
        ui: String,
    },
    /// Interactive setup wizard (provider, model, API keys)
    Setup,
    /// Show current configuration and diagnostic info
    Doctor,
    /// Authenticate with OpenAI Codex (OAuth)
    CodexAuth {
        /// Use device code flow instead of browser
        #[arg(long)]
        device: bool,
    },
    /// Authenticate with GitHub Copilot (OAuth Device Flow)
    CopilotAuth,
    /// List and select models for a provider
    Models {
        /// Provider name (anthropic, nebius, fireworks, github-copilot, openai-codex, zai, openai-compatible) or "add"
        provider: Option<String>,
        /// Model entry for "add" subcommand (format: provider:model-id)
        model_entry: Option<String>,
    },
    /// Plugin management commands
    Plugin {
        #[command(subcommand)]
        command: plugin::PluginCommands,
    },
    /// OS-level sandbox management (bwrap)
    Sandbox {
        #[command(subcommand)]
        command: sandbox_cmd::SandboxCommands,
    },
    /// Catch-all for plugin aliases (e.g. `lukan wa ...`)
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    dotenvy::dotenv().ok();

    // Initialize tracing to a log file (not stderr — TUI uses inline viewport,
    // so any stderr output corrupts the display)
    let log_dir = LukanPaths::config_dir();
    std::fs::create_dir_all(&log_dir).ok();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "lukan=info".parse().unwrap());

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_dir.join("lukan.log"))
        .ok();

    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .init();
    } else {
        // Fallback: sink to avoid TUI corruption if log file can't be created
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::sink)
            .init();
    }

    let cli = Cli::parse();

    // Ensure config dirs exist
    LukanPaths::ensure_dirs().await?;

    // Consume the command and dispatch
    match cli.command {
        Some(Commands::Setup) => {
            setup::run_setup().await?;
        }
        Some(Commands::Doctor) => {
            setup::run_doctor().await?;
        }
        Some(Commands::CodexAuth { device }) => {
            setup::run_codex_auth(device).await?;
        }
        Some(Commands::CopilotAuth) => {
            setup::run_copilot_auth().await?;
        }
        Some(Commands::Models {
            provider,
            model_entry,
        }) => {
            models::run_models(provider.as_deref(), model_entry.as_deref()).await?;
        }
        Some(Commands::Plugin { command }) => {
            plugin::handle_plugin_command(command).await?;
        }
        Some(Commands::Sandbox { command }) => {
            sandbox_cmd::handle_sandbox_command(command).await?;
        }
        Some(Commands::External(args)) => {
            dispatch_alias_command(&args).await?;
        }
        Some(Commands::Chat {
            provider,
            model,
            ui,
        }) => {
            let provider_override = provider.or(cli.provider);
            let model_override = model.or(cli.model);
            if ui == "web" {
                run_web(provider_override, model_override).await?;
            } else {
                run_chat(provider_override, model_override).await?;
            }
        }
        None => {
            let provider_override = cli.provider;
            let model_override = cli.model;
            run_chat(provider_override, model_override).await?;
        }
    }

    Ok(())
}

// ── Dynamic alias routing ────────────────────────────────────────────

/// Resolve a plugin alias to its plugin name.
/// Scans all installed plugins and returns the plugin name if the alias matches.
async fn resolve_plugin_alias(alias: &str) -> Result<Option<String>> {
    let manager = PluginManager::new();
    let names = manager.discover().await?;

    for name in names {
        if let Ok(manifest) = PluginManager::load_manifest(&name).await
            && (manifest.plugin.alias.as_deref() == Some(alias) || manifest.plugin.name == alias)
        {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// Dispatch a command that matched via external_subcommand (plugin alias).
///
/// Routing logic:
///   lukan <alias>                        → plugin status <name>
///   lukan <alias> start [-p X] [-m Y]   → plugin start <name> ...
///   lukan <alias> stop                   → plugin stop <name>
///   lukan <alias> restart                → plugin stop + start <name>
///   lukan <alias> logs [-f] [-n 50]     → plugin logs <name> ...
///   lukan <alias> status                 → plugin status <name>
///   lukan <alias> <config_key> ...       → plugin config <name> <key> ...
///   lukan <alias> <custom_command> ...   → plugin exec <name> <cmd> ...
async fn dispatch_alias_command(args: &[String]) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("No command provided");
    }

    let alias = &args[0];
    let plugin_name = resolve_plugin_alias(alias)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Unknown command: '{alias}'"))?;

    let sub_args = &args[1..];

    // No subcommand → status
    if sub_args.is_empty() {
        return plugin::handle_plugin_command(plugin::PluginCommands::Status {
            name: plugin_name,
        })
        .await;
    }

    let subcmd = sub_args[0].as_str();
    let rest = &sub_args[1..];

    match subcmd {
        // ── Lifecycle commands ──
        "start" => {
            let (provider, model) = parse_provider_model_flags(rest);
            plugin::handle_plugin_command(plugin::PluginCommands::Start {
                name: plugin_name,
                provider,
                model,
            })
            .await
        }
        "stop" => {
            plugin::handle_plugin_command(plugin::PluginCommands::Stop {
                name: plugin_name,
            })
            .await
        }
        "restart" => {
            // Stop then start
            plugin::handle_plugin_command(plugin::PluginCommands::Stop {
                name: plugin_name.clone(),
            })
            .await
            .ok(); // Ignore error if not running
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let (provider, model) = parse_provider_model_flags(rest);
            plugin::handle_plugin_command(plugin::PluginCommands::Start {
                name: plugin_name,
                provider,
                model,
            })
            .await
        }
        "status" => {
            plugin::handle_plugin_command(plugin::PluginCommands::Status {
                name: plugin_name,
            })
            .await
        }
        "logs" => {
            let (follow, lines) = parse_logs_flags(rest);
            plugin::handle_plugin_command(plugin::PluginCommands::Logs {
                name: plugin_name,
                follow,
                lines,
            })
            .await
        }

        // ── Could be a config key or a custom command ──
        other => {
            let manifest = PluginManager::load_manifest(&plugin_name).await?;

            // Check if it's a custom command first
            if manifest.commands.contains_key(other) {
                let cmd_args: Vec<String> = rest.to_vec();
                plugin_exec::handle_plugin_exec(&plugin_name, other, &cmd_args).await
            }
            // Check if it's a config key
            else if manifest.config.contains_key(other) {
                let action = rest.first().map(|s| s.as_str());
                let value = rest.get(1).map(|s| s.as_str());
                plugin_config::handle_plugin_config(&plugin_name, Some(other), action, value)
                    .await
            } else {
                // Unknown subcommand
                let mut available = Vec::new();
                available.extend(["start", "stop", "restart", "status", "logs"].iter().map(|s| s.to_string()));
                for key in manifest.config.keys() {
                    available.push(key.clone());
                }
                for key in manifest.commands.keys() {
                    available.push(key.clone());
                }
                available.sort();

                anyhow::bail!(
                    "Unknown subcommand '{other}' for plugin '{plugin_name}'.\n\
                     Available: {}",
                    available.join(", ")
                )
            }
        }
    }
}

/// Parse -p/--provider and -m/--model flags from remaining args.
fn parse_provider_model_flags(args: &[String]) -> (Option<String>, Option<String>) {
    let mut provider = None;
    let mut model = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--provider" => {
                if i + 1 < args.len() {
                    provider = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-m" | "--model" => {
                if i + 1 < args.len() {
                    model = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    (provider, model)
}

/// Parse -f/--follow and -n/--lines flags from remaining args.
fn parse_logs_flags(args: &[String]) -> (bool, String) {
    let mut follow = false;
    let mut lines = "50".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--follow" => {
                follow = true;
                i += 1;
            }
            "-n" | "--lines" => {
                if i + 1 < args.len() {
                    lines = args[i + 1].clone();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    (follow, lines)
}

// ── Existing command handlers ────────────────────────────────────────

async fn run_web(provider_override: Option<String>, model_override: Option<String>) -> Result<()> {
    // Load config
    let mut config = ConfigManager::load().await?;

    // Apply CLI overrides
    if let Some(p) = provider_override {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name. Valid: anthropic, nebius, fireworks, github-copilot, openai-codex, zai, openai-compatible")?;
    }
    if let Some(m) = model_override {
        config.model = Some(m);
    }

    // Load credentials
    let credentials = CredentialsManager::load().await?;

    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    let port = std::env::var("LUKAN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000u16);

    lukan_web::start_web_server(resolved, port).await?;

    Ok(())
}

async fn run_chat(provider_override: Option<String>, model_override: Option<String>) -> Result<()> {
    // Load config
    let mut config = ConfigManager::load().await?;

    // Apply CLI overrides
    if let Some(p) = provider_override {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name. Valid: anthropic, nebius, fireworks, github-copilot, openai-codex, zai, openai-compatible")?;
    }
    if let Some(m) = model_override {
        config.model = Some(m);
    }

    // Load credentials
    let credentials = CredentialsManager::load().await?;

    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    info!(
        "Starting lukan with provider={}, model={}",
        resolved.config.provider,
        resolved.effective_model()
    );

    // Create provider
    let provider = create_provider(&resolved)?;

    // Run TUI
    let app = App::new(provider, resolved);
    app.run().await?;

    Ok(())
}
