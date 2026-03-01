use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use lukan_core::config::{
    ConfigManager, CredentialsManager, LukanPaths, ProviderName, ResolvedConfig,
};
use lukan_plugins::PluginManager;
use lukan_providers::create_provider;
use lukan_tui::app::App;

mod daemon;
mod models;
mod plugin;
mod plugin_config;
mod plugin_exec;
mod sandbox_cmd;
mod setup;
mod update;
mod worker;

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

    /// Continue the most recent chat session
    #[arg(long, short = 'c')]
    r#continue: bool,
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
        /// Continue the most recent chat session
        #[arg(long, short = 'c')]
        r#continue: bool,
        /// UI mode: tui (default) or web
        #[arg(long, default_value = "tui")]
        ui: String,
        /// Open the desktop settings app (Tauri)
        #[arg(long)]
        desktop: bool,
        /// Enable browser tools: auto (default), chrome, edge, chromium
        #[arg(long, value_name = "BROWSER", default_missing_value = "auto", num_args = 0..=1)]
        browser: Option<String>,
        /// Connect to an existing Chrome DevTools Protocol endpoint
        #[arg(long, value_name = "URL")]
        browser_cdp: Option<String>,
        /// Allow browser navigation to private/internal IPs
        #[arg(long)]
        browser_allow_internal: bool,
        /// Chrome profile: temp (default), persistent, or a custom path
        #[arg(long, value_name = "MODE", default_value = "temp")]
        browser_profile: String,
        /// Run Chrome in visible (headed) mode
        #[arg(long)]
        browser_visible: bool,
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
    /// Self-update to the latest version
    Update,
    /// Manage scheduled workers
    Worker {
        #[command(subcommand)]
        command: worker::WorkerCommands,
    },
    /// Worker daemon (background scheduler)
    Daemon {
        #[command(subcommand)]
        command: daemon::DaemonCommands,
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
        Some(Commands::Update) => {
            update::run_update().await?;
        }
        Some(Commands::Sandbox { command }) => {
            sandbox_cmd::handle_sandbox_command(command).await?;
        }
        Some(Commands::Worker { command }) => {
            worker::handle_worker_command(command).await?;
        }
        Some(Commands::Daemon { command }) => {
            daemon::handle_daemon_command(command).await?;
        }
        Some(Commands::External(args)) => {
            dispatch_alias_command(&args).await?;
        }
        Some(Commands::Chat {
            provider,
            model,
            r#continue: continue_session,
            ui,
            desktop,
            browser,
            browser_cdp,
            browser_allow_internal,
            browser_profile,
            browser_visible,
        }) => {
            if desktop {
                run_desktop().await?;
            } else {
                let provider_override = provider.or(cli.provider);
                let model_override = model.or(cli.model);
                let do_continue = continue_session || cli.r#continue;
                if ui == "web" {
                    run_web(provider_override, model_override).await?;
                } else {
                    let browser_opts =
                        if browser.is_some() || browser_cdp.is_some() || browser_visible {
                            Some(BrowserOpts {
                                cdp_url: browser_cdp,
                                allow_internal: browser_allow_internal,
                                profile: browser_profile,
                                visible: browser_visible,
                                browser_name: browser.unwrap_or_else(|| "auto".to_string()),
                            })
                        } else {
                            None
                        };
                    run_chat(provider_override, model_override, browser_opts, do_continue).await?;
                }
            }
        }
        None => {
            let provider_override = cli.provider;
            let model_override = cli.model;
            run_chat(provider_override, model_override, None, cli.r#continue).await?;
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
        return plugin::handle_plugin_command(plugin::PluginCommands::Status { name: plugin_name })
            .await;
    }

    let subcmd = sub_args[0].as_str();
    let rest = &sub_args[1..];

    // ── Help ──
    if subcmd == "--help" || subcmd == "-h" || subcmd == "help" {
        let manifest = PluginManager::load_manifest(&plugin_name).await?;

        println!("\x1b[1mUsage:\x1b[0m lukan {alias} <command>\n");
        println!("\x1b[1mLifecycle:\x1b[0m");
        println!("  start     Start the plugin daemon");
        println!("  stop      Stop the plugin daemon");
        println!("  restart   Restart the plugin daemon");
        println!("  status    Show plugin status");
        println!("  logs      Show plugin logs (--follow, --lines N)");

        if !manifest.commands.is_empty() {
            println!("\n\x1b[1mCommands:\x1b[0m");
            let mut cmds: Vec<_> = manifest.commands.iter().collect();
            cmds.sort_by_key(|(k, _)| *k);
            for (name, def) in cmds {
                let desc = &def.description;
                println!("  {name:<18} {desc}");
            }
        }

        if !manifest.config.is_empty() {
            println!("\n\x1b[1mConfig:\x1b[0m");
            let mut keys: Vec<_> = manifest.config.iter().collect();
            keys.sort_by_key(|(k, _)| *k);
            for (key, schema) in keys {
                let type_label = match schema.field_type {
                    lukan_core::models::plugin::ConfigFieldType::String => "string",
                    lukan_core::models::plugin::ConfigFieldType::StringArray => "string[]",
                    lukan_core::models::plugin::ConfigFieldType::Number => "number",
                    lukan_core::models::plugin::ConfigFieldType::Bool => "bool",
                };
                let desc = if schema.description.is_empty() {
                    String::new()
                } else {
                    format!("  {}", schema.description)
                };
                println!("  {key:<18} \x1b[2m({type_label}){desc}\x1b[0m");
            }
        }

        return Ok(());
    }

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
            plugin::handle_plugin_command(plugin::PluginCommands::Stop { name: plugin_name }).await
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
            plugin::handle_plugin_command(plugin::PluginCommands::Status { name: plugin_name })
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
                plugin_config::handle_plugin_config(&plugin_name, Some(other), action, value).await
            } else {
                // Unknown subcommand
                let mut available = Vec::new();
                available.extend(
                    ["start", "stop", "restart", "status", "logs"]
                        .iter()
                        .map(|s| s.to_string()),
                );
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
    // Ensure worker daemon is running
    daemon::ensure_daemon_running();

    // Load config
    let mut config = ConfigManager::load().await?;

    // Apply CLI overrides
    if let Some(p) = provider_override {
        let new_provider: ProviderName = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name. Valid: anthropic, nebius, fireworks, github-copilot, openai-codex, zai, ollama-cloud, openai-compatible")?;
        // When switching provider via CLI without --model, reset to the new provider's default
        if config.provider != new_provider && model_override.is_none() {
            config.model = None;
        }
        config.provider = new_provider;
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

async fn run_desktop() -> Result<()> {
    // Ensure worker daemon is running
    daemon::ensure_daemon_running();

    // Desktop requires a graphical display (X11 or Wayland)
    let has_display = std::env::var("DISPLAY").is_ok_and(|v| !v.is_empty())
        || std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty());

    if !has_display {
        anyhow::bail!(
            "No graphical display detected (DISPLAY/WAYLAND_DISPLAY not set).\n\
             --desktop requires a desktop environment with X11 or Wayland.\n\
             Use 'lukan chat' for TUI or 'lukan chat --ui web' for browser."
        );
    }

    // Find the lukan-desktop binary next to this executable, or in PATH
    let self_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let desktop_bin = self_exe
        .parent()
        .map(|p| p.join("lukan-desktop"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("lukan-desktop"));

    if !desktop_bin.exists() && which_cmd_exists("lukan-desktop").is_none() {
        anyhow::bail!(
            "lukan-desktop binary not found.\n\
             Build it with: cargo build -p lukan-desktop"
        );
    }

    info!("Launching lukan-desktop from {}", desktop_bin.display());

    let output = tokio::process::Command::new(&desktop_bin)
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .with_context(|| format!("Failed to run {}", desktop_bin.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code();

        // Check stderr for common issues
        let stderr_lower = stderr.to_lowercase();
        if stderr_lower.contains("libwebkit2gtk")
            || stderr_lower.contains("shared object file")
            || stderr_lower.contains("shared library")
            || code == Some(127)
        {
            eprintln!("Error: lukan-desktop is missing required system libraries.");
            eprintln!();
            eprintln!("Install the dependencies:");
            eprintln!("  Ubuntu/Debian: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0");
            eprintln!("  Fedora:        sudo dnf install webkit2gtk4.1 gtk3");
            eprintln!("  Arch:          sudo pacman -S webkit2gtk-4.1 gtk3");
            eprintln!();
            eprintln!("Or use the browser UI instead: lukan chat --ui web");
            std::process::exit(1);
        }

        if stderr_lower.contains("gtk") || stderr_lower.contains("display") {
            // GTK/display init failure — our panic hook already printed a message
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
            std::process::exit(1);
        }

        // code=None means killed by signal
        if let Some(c) = code {
            if c == 1 || c == 101 {
                // Our panic hook (1) or raw panic (101) already printed to stderr
                if !stderr.is_empty() {
                    eprint!("{stderr}");
                }
                std::process::exit(c);
            }
            anyhow::bail!("lukan-desktop exited with code {c}\n{stderr}");
        }

        // Killed by signal (SIGSEGV, SIGABRT, etc.)
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = output.status.signal() {
                let sig_name = match sig {
                    11 => "SIGSEGV (segmentation fault)",
                    6 => "SIGABRT (aborted)",
                    9 => "SIGKILL",
                    _ => "",
                };
                eprintln!(
                    "Error: lukan-desktop crashed (signal {sig}{}).",
                    if sig_name.is_empty() {
                        String::new()
                    } else {
                        format!(" — {sig_name}")
                    }
                );
                if !stderr.is_empty() {
                    eprintln!("{stderr}");
                }
                eprintln!();
                eprintln!("This usually means a missing or incompatible system library.");
                eprintln!("Try: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0");
                eprintln!("Or use: lukan chat --ui web");
                std::process::exit(1);
            }
        }

        anyhow::bail!("lukan-desktop failed\n{stderr}");
    }

    Ok(())
}

/// Check if a command exists in PATH, returning its path.
fn which_cmd_exists(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(cmd);
            if full.is_file() { Some(full) } else { None }
        })
    })
}

/// Browser CLI options.
struct BrowserOpts {
    cdp_url: Option<String>,
    allow_internal: bool,
    /// Profile mode string from CLI: "temp", "persistent", or a custom path.
    profile: String,
    visible: bool,
    /// Browser name: "auto", "chrome", "edge", "chromium".
    browser_name: String,
}

impl BrowserOpts {
    fn profile_mode(&self) -> lukan_browser::ProfileMode {
        match self.profile.as_str() {
            "temp" => lukan_browser::ProfileMode::Temp,
            "persistent" => lukan_browser::ProfileMode::Persistent,
            path => lukan_browser::ProfileMode::Custom(std::path::PathBuf::from(path)),
        }
    }
}

async fn run_chat(
    provider_override: Option<String>,
    model_override: Option<String>,
    browser_opts: Option<BrowserOpts>,
    continue_session: bool,
) -> Result<()> {
    // Ensure worker daemon is running
    daemon::ensure_daemon_running();

    // Load config
    let mut config = ConfigManager::load().await?;

    // Apply CLI overrides
    if let Some(p) = provider_override {
        let new_provider: ProviderName = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name. Valid: anthropic, nebius, fireworks, github-copilot, openai-codex, zai, ollama-cloud, openai-compatible")?;
        // When switching provider via CLI without --model, reset to the new provider's default
        if config.provider != new_provider && model_override.is_none() {
            config.model = None;
        }
        config.provider = new_provider;
    }
    if let Some(m) = model_override {
        config.model = Some(m);
    }

    // Initialize browser if requested
    if let Some(opts) = &browser_opts {
        let cdp_url = opts
            .cdp_url
            .clone()
            .or_else(|| config.browser_cdp_url.clone());

        lukan_browser::BrowserManager::init(lukan_browser::BrowserConfig {
            cdp_url,
            allow_internal: opts.allow_internal,
            profile: opts.profile_mode(),
            visible: opts.visible,
            download_dir: None,
            browser_name: opts.browser_name.clone(),
        });
    }

    // Load credentials
    let credentials = CredentialsManager::load().await?;

    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    info!(
        "Starting lukan with provider={}, model={}, browser={}",
        resolved.config.provider,
        resolved.effective_model().as_deref().unwrap_or("(none)"),
        browser_opts.is_some()
    );

    // Create provider (falls back to NullProvider when no model is selected,
    // so the TUI still launches and the user can pick a model via /model)
    let provider =
        create_provider(&resolved).unwrap_or_else(|_| Box::new(lukan_providers::NullProvider));

    // Run TUI
    let mut app = App::new(provider, resolved);
    if browser_opts.is_some() {
        app.enable_browser_tools();
    }
    if continue_session {
        app.set_continue_session();
    }
    app.run().await?;

    // Cleanup browser on exit
    if let Some(manager) = lukan_browser::BrowserManager::get() {
        manager.disconnect().await;
    }

    Ok(())
}
