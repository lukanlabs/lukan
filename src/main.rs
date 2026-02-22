use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig};
use lukan_providers::create_provider;
use lukan_tui::app::App;

mod models;
mod plugin;
mod sandbox_cmd;
mod setup;
mod whatsapp;

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
    /// Authenticate with Google (OAuth2 PKCE) for Workspace tools
    GoogleAuth,
    /// List and select models for a provider
    Models {
        /// Provider name (anthropic, nebius, fireworks, github-copilot, openai-codex, zai, openai-compatible) or "add"
        provider: Option<String>,
        /// Model entry for "add" subcommand (format: provider:model-id)
        model_entry: Option<String>,
    },
    /// Start WhatsApp channel
    Whatsapp {
        /// Override the LLM provider
        #[arg(long, short)]
        provider: Option<String>,
        /// Override the model
        #[arg(long, short)]
        model: Option<String>,
        /// Don't auto-start the connector
        #[arg(long)]
        no_connector: bool,
    },
    /// WhatsApp plugin management
    Wa {
        #[command(subcommand)]
        command: whatsapp::WaCommands,
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
            return Ok(());
        }
        Some(Commands::Doctor) => {
            setup::run_doctor().await?;
            return Ok(());
        }
        Some(Commands::CodexAuth { device }) => {
            setup::run_codex_auth(device).await?;
            return Ok(());
        }
        Some(Commands::CopilotAuth) => {
            setup::run_copilot_auth().await?;
            return Ok(());
        }
        Some(Commands::GoogleAuth) => {
            run_google_auth().await?;
            return Ok(());
        }
        Some(Commands::Models {
            provider,
            model_entry,
        }) => {
            models::run_models(provider.as_deref(), model_entry.as_deref()).await?;
            return Ok(());
        }
        Some(Commands::Wa { command }) => {
            whatsapp::handle_wa_command(command).await?;
            return Ok(());
        }
        Some(Commands::Plugin { command }) => {
            plugin::handle_plugin_command(command).await?;
            return Ok(());
        }
        Some(Commands::Sandbox { command }) => {
            sandbox_cmd::handle_sandbox_command(command).await?;
            return Ok(());
        }
        Some(Commands::Whatsapp {
            provider,
            model,
            no_connector,
        }) => {
            // Load config + credentials for WhatsApp
            let config = ConfigManager::load().await?;
            let credentials = CredentialsManager::load().await?;
            let resolved = ResolvedConfig {
                config,
                credentials,
            };
            whatsapp::run_whatsapp(provider, model, no_connector, &resolved).await?;
            return Ok(());
        }
        Some(Commands::Chat {
            provider,
            model,
            ui,
        }) => {
            let provider_override = provider.or(cli.provider);
            let model_override = model.or(cli.model);
            if ui == "web" {
                return run_web(provider_override, model_override).await;
            }
            return run_chat(provider_override, model_override).await;
        }
        None => {
            let provider_override = cli.provider;
            let model_override = cli.model;
            return run_chat(provider_override, model_override).await;
        }
    }
}

async fn run_google_auth() -> Result<()> {
    use lukan_core::config::CredentialsManager;

    println!("\n\x1b[1m\x1b[36m  lukan google-auth\x1b[0m");
    println!("\x1b[2m  Google Workspace authentication (OAuth2 PKCE)\x1b[0m\n");

    // Load existing credentials to get client_id / client_secret
    let creds = CredentialsManager::load().await?;

    let client_id = creds
        .google_client_id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("GOOGLE_CLIENT_ID").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Google Client ID not configured.\n\
                 Set it via: lukan setup  or  export GOOGLE_CLIENT_ID=...\n\
                 Or set 'googleClientId' in ~/.config/lukan/credentials.json\n\
                 Create credentials at https://console.cloud.google.com/apis/credentials"
            )
        })?;

    let client_secret = creds
        .google_client_secret
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("GOOGLE_CLIENT_SECRET").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Google Client Secret not configured.\n\
                 Set it via: lukan setup  or  export GOOGLE_CLIENT_SECRET=...\n\
                 Or set 'googleClientSecret' in ~/.config/lukan/credentials.json"
            )
        })?;

    let tokens =
        lukan_tools::google_auth::authenticate_google(&client_id, &client_secret).await?;

    // Save tokens
    let mut creds = CredentialsManager::load().await?;
    creds.google_client_id = Some(client_id);
    creds.google_client_secret = Some(client_secret);
    creds.google_access_token = Some(tokens.access_token);
    creds.google_refresh_token = Some(tokens.refresh_token);
    creds.google_token_expiry = Some(tokens.expires_at);
    CredentialsManager::save(&creds).await?;

    println!("\x1b[32m✓\x1b[0m Google authentication successful!");
    println!(
        "\x1b[32m✓\x1b[0m Credentials saved to \x1b[2m{}\x1b[0m",
        lukan_core::config::LukanPaths::credentials_file().display()
    );
    println!("\n\x1b[2mGoogle Workspace tools (Sheets, Calendar, Docs, Drive) are now available.\x1b[0m\n");

    Ok(())
}

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
