use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ResolvedConfig};
use lukan_providers::create_provider;
use lukan_tui::app::App;

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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    dotenvy::dotenv().ok();

    // Initialize tracing (to stderr, not stdout — TUI owns stdout)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lukan=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Ensure config dirs exist
    LukanPaths::ensure_dirs().await?;

    // Determine provider/model overrides
    let (provider_override, model_override) = match &cli.command {
        Some(Commands::Chat { provider, model }) => (
            provider.clone().or(cli.provider.clone()),
            model.clone().or(cli.model.clone()),
        ),
        None => (cli.provider.clone(), cli.model.clone()),
    };

    // Load config
    let mut config = ConfigManager::load().await?;

    // Apply CLI overrides
    if let Some(p) = provider_override {
        config.provider = serde_json::from_value(serde_json::Value::String(p))
            .context("Invalid provider name")?;
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
