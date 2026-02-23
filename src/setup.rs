use anyhow::Result;
use std::io::{self, Write};

use lukan_core::config::{
    AppConfig, ConfigManager, Credentials, CredentialsManager, LukanPaths, ProviderName,
};
use lukan_providers::{codex_auth, copilot_auth};

// ── Colors ─────────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

// ── Setup wizard ───────────────────────────────────────────────────────────

pub async fn run_setup() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan setup{RESET}");
    println!("{DIM}  Interactive configuration wizard{RESET}\n");

    let config = ConfigManager::load().await?;
    let creds = CredentialsManager::load().await?;

    let config = setup_provider(config)?;
    let config = setup_model(config)?;
    let config = if config.provider == ProviderName::OpenaiCompatible {
        setup_openai_compatible_url(config)?
    } else {
        config
    };
    let creds = setup_credentials(&config.provider, creds)?;

    // Save
    ConfigManager::save(&config).await?;
    CredentialsManager::save(&creds).await?;

    println!(
        "\n{GREEN}✓{RESET} Configuration saved to {DIM}{}{RESET}",
        LukanPaths::config_file().display()
    );
    println!(
        "{GREEN}✓{RESET} Credentials saved to {DIM}{}{RESET}",
        LukanPaths::credentials_file().display()
    );

    // Quick validation
    println!();
    let api_key = CredentialsManager::get_api_key(&creds, &config.provider);
    if api_key.is_some() {
        println!(
            "{GREEN}✓{RESET} API key configured for {BOLD}{}{RESET}",
            config.provider
        );
    } else if config.provider == ProviderName::OpenaiCompatible {
        println!(
            "{GREEN}✓{RESET} No API key for {BOLD}{}{RESET} {DIM}(optional){RESET}",
            config.provider
        );
    } else {
        let env_var = env_var_for_provider(&config.provider);
        println!(
            "{YELLOW}⚠{RESET} No API key for {BOLD}{}{RESET}. Set via {CYAN}{env_var}{RESET} or re-run {CYAN}lukan setup{RESET}",
            config.provider
        );
    }

    let model = config
        .model
        .as_deref()
        .unwrap_or(config.provider.default_model());
    println!(
        "{GREEN}✓{RESET} Active: {BOLD}{}{RESET} / {BOLD}{}{RESET}",
        config.provider, model
    );
    println!();

    Ok(())
}

fn setup_provider(mut config: AppConfig) -> Result<AppConfig> {
    println!("{BOLD}1. Provider{RESET}");
    println!();

    let providers = [
        ("anthropic", "Anthropic (Claude)"),
        ("nebius", "Nebius (DeepSeek, MiniMax, GLM)"),
        ("fireworks", "Fireworks (open-source models)"),
        ("github-copilot", "GitHub Copilot"),
        ("openai-codex", "OpenAI Codex"),
        ("zai", "z.ai (GLM models)"),
        ("openai-compatible", "OpenAI-compatible endpoint"),
    ];

    let current_str = config.provider.to_string();
    for (i, (id, desc)) in providers.iter().enumerate() {
        let is_current = *id == current_str;
        let marker = if is_current {
            format!("{GREEN}●{RESET}")
        } else {
            format!("{DIM}○{RESET}")
        };
        println!("  {marker} {BOLD}{}{RESET}  {DIM}{desc}{RESET}", i + 1);
    }

    println!();
    let input = prompt(&format!(
        "Select provider [1-7] {DIM}(current: {current_str}){RESET}: "
    ))?;

    if !input.is_empty() {
        let idx: usize = input.trim().parse().unwrap_or(0);
        if idx >= 1 && idx <= providers.len() {
            let (id, _) = providers[idx - 1];
            config.provider = serde_json::from_value(serde_json::Value::String(id.to_string()))?;
            println!("  {GREEN}✓{RESET} Provider set to {BOLD}{id}{RESET}");
        } else {
            println!("  {YELLOW}⚠{RESET} Invalid selection, keeping {BOLD}{current_str}{RESET}");
        }
    } else {
        println!("  {DIM}Keeping {current_str}{RESET}");
    }

    println!();
    Ok(config)
}

fn setup_model(mut config: AppConfig) -> Result<AppConfig> {
    println!("{BOLD}2. Model{RESET}");
    println!();

    let default_model = config.provider.default_model();
    let current = config.model.as_deref().unwrap_or(default_model);

    println!(
        "  Default for {}: {BOLD}{default_model}{RESET}",
        config.provider
    );
    if current != default_model {
        println!("  Current override: {BOLD}{current}{RESET}");
    }

    println!();
    let input = prompt(&format!(
        "Model name {DIM}(Enter to keep '{current}'){RESET}: "
    ))?;

    if !input.is_empty() {
        let model = input.trim().to_string();
        config.model = Some(model.clone());
        println!("  {GREEN}✓{RESET} Model set to {BOLD}{model}{RESET}");
    } else {
        println!("  {DIM}Keeping {current}{RESET}");
    }

    println!();
    Ok(config)
}

fn setup_openai_compatible_url(mut config: AppConfig) -> Result<AppConfig> {
    println!("{BOLD}Base URL{RESET}");
    println!();

    let current = config.openai_compatible_base_url.as_deref().unwrap_or("");
    let hint = if current.is_empty() {
        "e.g. http://localhost:11434/v1"
    } else {
        current
    };

    let input = prompt(&format!(
        "  OpenAI-compatible base URL {DIM}({hint}){RESET}: "
    ))?;

    if !input.is_empty() {
        let normalized = lukan_providers::openai_compat::normalize_base_url(&input);
        config.openai_compatible_base_url = Some(normalized.clone());
        println!("    {GREEN}✓{RESET} Base URL set to {BOLD}{normalized}{RESET}");
    } else if !current.is_empty() {
        println!("    {DIM}Keeping {current}{RESET}");
    }

    println!();
    Ok(config)
}

fn setup_credentials(provider: &ProviderName, mut creds: Credentials) -> Result<Credentials> {
    println!("{BOLD}3. API Key{RESET}");
    println!();

    // Only ask for the API key of the selected provider
    match provider {
        ProviderName::Anthropic => {
            creds.anthropic_api_key = prompt_credential(
                "Anthropic API key",
                "ANTHROPIC_API_KEY",
                creds.anthropic_api_key.as_deref(),
            )?
            .or(creds.anthropic_api_key);
        }
        ProviderName::Nebius => {
            creds.nebius_api_key = prompt_credential(
                "Nebius API key",
                "NEBIUS_API_KEY",
                creds.nebius_api_key.as_deref(),
            )?
            .or(creds.nebius_api_key);
        }
        ProviderName::Fireworks => {
            creds.fireworks_api_key = prompt_credential(
                "Fireworks API key",
                "FIREWORKS_API_KEY",
                creds.fireworks_api_key.as_deref(),
            )?
            .or(creds.fireworks_api_key);
        }
        ProviderName::GithubCopilot => {
            creds.copilot_client_id = prompt_credential(
                "Copilot OAuth Client ID",
                "GITHUB_COPILOT_CLIENT_ID",
                creds.copilot_client_id.as_deref(),
            )?
            .or(creds.copilot_client_id);

            creds.github_token = prompt_credential(
                "GitHub token (manual fallback)",
                "GITHUB_TOKEN",
                creds.github_token.as_deref(),
            )?
            .or(creds.github_token);

            println!(
                "\n  {DIM}Tip: Run {CYAN}lukan copilot-auth{RESET}{DIM} for automatic OAuth authentication{RESET}"
            );
        }
        ProviderName::OpenaiCodex => {
            creds.codex_access_token = prompt_credential(
                "Codex access token",
                "CODEX_ACCESS_TOKEN",
                creds.codex_access_token.as_deref(),
            )?
            .or(creds.codex_access_token);
        }
        ProviderName::Zai => {
            creds.zai_api_key =
                prompt_credential("z.ai API key", "ZAI_API_KEY", creds.zai_api_key.as_deref())?
                    .or(creds.zai_api_key);
        }
        ProviderName::OpenaiCompatible => {
            creds.openai_compatible_api_key = prompt_credential(
                "OpenAI-compatible API key",
                "OPENAI_COMPATIBLE_API_KEY",
                creds.openai_compatible_api_key.as_deref(),
            )?
            .or(creds.openai_compatible_api_key);
        }
    }

    // Optionally configure search API
    println!();
    let input = prompt(&format!("Configure search API keys? {DIM}(y/N){RESET}: "))?;

    if input.trim().eq_ignore_ascii_case("y") {
        creds.brave_api_key = prompt_credential(
            "Brave Search API key",
            "BRAVE_API_KEY",
            creds.brave_api_key.as_deref(),
        )?
        .or(creds.brave_api_key);

        creds.tavily_api_key = prompt_credential(
            "Tavily API key",
            "TAVILY_API_KEY",
            creds.tavily_api_key.as_deref(),
        )?
        .or(creds.tavily_api_key);
    }

    Ok(creds)
}

fn prompt_credential(label: &str, env_var: &str, current: Option<&str>) -> Result<Option<String>> {
    let status = match current {
        Some(k) if !k.is_empty() => {
            let masked = mask_key(k);
            format!("{GREEN}configured{RESET} {DIM}({masked}){RESET}")
        }
        _ => format!("{DIM}not set{RESET}"),
    };

    let input = prompt(&format!("  {label} [{status}] {DIM}({env_var}){RESET}: "))?;

    if input.is_empty() {
        Ok(None)
    } else {
        let trimmed = input.trim().to_string();
        println!("    {GREEN}✓{RESET} Updated");
        Ok(Some(trimmed))
    }
}

// ── Codex Auth ────────────────────────────────────────────────────────────

pub async fn run_codex_auth(device: bool) -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan codex-auth{RESET}");
    println!("{DIM}  OpenAI Codex authentication{RESET}\n");

    let client = reqwest::Client::new();

    let tokens = if device {
        println!("{DIM}Using device code flow...{RESET}");
        codex_auth::auth_device_flow(&client).await?
    } else {
        println!("{DIM}Using browser flow...{RESET}");
        codex_auth::auth_browser_flow(&client).await?
    };

    // Save tokens to credentials
    let mut creds = CredentialsManager::load().await?;
    creds.codex_access_token = Some(tokens.access_token.clone());
    creds.codex_refresh_token = Some(tokens.refresh_token);
    creds.codex_token_expiry = Some(tokens.expires_at);
    CredentialsManager::save(&creds).await?;

    // Also set provider to openai-codex
    let mut config = ConfigManager::load().await?;
    config.provider = ProviderName::OpenaiCodex;
    ConfigManager::save(&config).await?;

    println!("{GREEN}✓{RESET} Codex authentication successful!");
    println!(
        "{GREEN}✓{RESET} Credentials saved to {DIM}{}{RESET}",
        LukanPaths::credentials_file().display()
    );
    println!("{GREEN}✓{RESET} Provider set to {BOLD}openai-codex{RESET}");

    // Show account ID if extractable
    if let Some(acct_id) = codex_auth::extract_account_id(&tokens.access_token) {
        println!("{GREEN}✓{RESET} Account ID: {DIM}{acct_id}{RESET}");
    }

    println!("\n{DIM}Run 'lukan chat' to start chatting with Codex.{RESET}\n");

    Ok(())
}

// ── Copilot Auth ──────────────────────────────────────────────────────────

pub async fn run_copilot_auth() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan copilot-auth{RESET}");
    println!("{DIM}  GitHub Copilot authentication (OAuth Device Flow){RESET}\n");

    let creds = CredentialsManager::load().await?;

    let client_id = creds
        .copilot_client_id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("GITHUB_COPILOT_CLIENT_ID").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "GitHub Copilot Client ID not configured.\n\
                 Set it via: lukan setup  or  export GITHUB_COPILOT_CLIENT_ID=...\n\
                 Create an OAuth App at https://github.com/settings/developers"
            )
        })?;

    let client = reqwest::Client::new();
    let token = copilot_auth::auth_copilot_device_flow(&client, &client_id).await?;

    // Save token
    let mut creds = CredentialsManager::load().await?;
    creds.copilot_token = Some(token);
    CredentialsManager::save(&creds).await?;

    // Set provider to github-copilot
    let mut config = ConfigManager::load().await?;
    config.provider = ProviderName::GithubCopilot;
    ConfigManager::save(&config).await?;

    println!("{GREEN}✓{RESET} Copilot authentication successful!");
    println!(
        "{GREEN}✓{RESET} Credentials saved to {DIM}{}{RESET}",
        LukanPaths::credentials_file().display()
    );
    println!("{GREEN}✓{RESET} Provider set to {BOLD}github-copilot{RESET}");
    println!("\n{DIM}Run 'lukan chat' to start chatting.{RESET}\n");

    Ok(())
}

// ── Doctor command ─────────────────────────────────────────────────────────

pub async fn run_doctor() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan doctor{RESET}");
    println!("{DIM}  Configuration diagnostic{RESET}\n");

    let config = ConfigManager::load().await?;
    let creds = CredentialsManager::load().await?;

    // Installation
    println!("{BOLD}Installation{RESET}");
    println!(
        "  Config dir:     {}",
        format_path_status(&LukanPaths::config_dir())
    );
    println!(
        "  Config file:    {}",
        format_path_status(&LukanPaths::config_file())
    );
    println!(
        "  Credentials:    {}",
        format_path_status(&LukanPaths::credentials_file())
    );
    println!(
        "  Sessions dir:   {}",
        format_path_status(&LukanPaths::sessions_dir())
    );
    println!();

    // Active provider
    let model = config
        .model
        .as_deref()
        .unwrap_or(config.provider.default_model());
    println!("{BOLD}Active Provider{RESET}");
    println!("  Provider:  {BOLD}{}{RESET}", config.provider);
    println!("  Model:     {BOLD}{model}{RESET}");
    println!("  MaxTokens: {}", config.max_tokens);
    if config.provider == ProviderName::OpenaiCompatible
        && let Some(ref url) = config.openai_compatible_base_url
    {
        println!("  Base URL:  {BOLD}{url}{RESET}");
    }
    if let Some(ref tz) = config.timezone {
        println!("  Timezone:  {tz}");
    }
    println!();

    // Credentials status
    println!("{BOLD}API Keys{RESET}");
    print_key_status(
        "Anthropic",
        creds.anthropic_api_key.as_deref(),
        "ANTHROPIC_API_KEY",
    );
    print_key_status("Nebius", creds.nebius_api_key.as_deref(), "NEBIUS_API_KEY");
    print_key_status(
        "Fireworks",
        creds.fireworks_api_key.as_deref(),
        "FIREWORKS_API_KEY",
    );
    print_key_status(
        "Copilot",
        creds.copilot_token.as_deref(),
        "lukan copilot-auth",
    );
    print_key_status(
        "GitHub token",
        creds.github_token.as_deref(),
        "GITHUB_TOKEN",
    );
    print_key_status(
        "Codex",
        creds.codex_access_token.as_deref(),
        "lukan codex-auth",
    );
    print_key_status("z.ai", creds.zai_api_key.as_deref(), "ZAI_API_KEY");
    print_key_status(
        "OpenAI-compat",
        creds.openai_compatible_api_key.as_deref(),
        "OPENAI_COMPATIBLE_API_KEY",
    );
    print_key_status(
        "Brave Search",
        creds.brave_api_key.as_deref(),
        "BRAVE_API_KEY",
    );
    print_key_status("Tavily", creds.tavily_api_key.as_deref(), "TAVILY_API_KEY");
    println!();

    // Active provider check
    let active_key = CredentialsManager::get_api_key(&creds, &config.provider);
    println!("{BOLD}Health Check{RESET}");
    if active_key.is_some() {
        println!(
            "  {GREEN}✓{RESET} API key present for active provider ({BOLD}{}{RESET})",
            config.provider
        );
    } else if config.provider == ProviderName::OpenaiCompatible {
        // API key is optional for openai-compatible (Ollama, vLLM, LM Studio, etc.)
        println!(
            "  {GREEN}✓{RESET} No API key for {BOLD}{}{RESET} {DIM}(optional for local endpoints){RESET}",
            config.provider
        );
    } else {
        let env_var = env_var_for_provider(&config.provider);
        println!(
            "  {RED}✗{RESET} No API key for active provider ({BOLD}{}{RESET})",
            config.provider
        );
        println!("    Set via: {CYAN}lukan setup{RESET} or {CYAN}export {env_var}=...{RESET}");
    }

    // Models configured
    if let Some(ref models) = config.models {
        println!("  Models configured: {}", models.len());
    } else {
        println!("  {DIM}No custom models configured (using provider defaults){RESET}");
    }
    println!();

    // ── Sandbox (bwrap) ──
    println!("{BOLD}Sandbox{RESET}");
    let bwrap_available = lukan_tools::sandbox::is_bwrap_available();
    if bwrap_available {
        println!("  {GREEN}✓{RESET} OS sandbox (bwrap): {GREEN}available{RESET}");
    } else {
        println!("  {YELLOW}!{RESET} OS sandbox (bwrap): {YELLOW}not available{RESET}");
        let diagnosis = lukan_tools::sandbox::diagnose_bwrap();
        println!("  {DIM}Diagnosis: {diagnosis}{RESET}");
    }
    let has_profile = lukan_tools::sandbox::has_apparmor_profile();
    if has_profile {
        println!("  {GREEN}✓{RESET} AppArmor profile:   {GREEN}installed{RESET}");
    } else {
        println!("  {DIM}✗ AppArmor profile:   not installed{RESET}");
    }
    println!();

    // ── Plugins ──
    println!("{BOLD}Plugins{RESET}");
    let plugin_mgr = lukan_plugins::PluginManager::new();
    let installed = plugin_mgr.discover().await.unwrap_or_default();
    if installed.is_empty() {
        println!("  {DIM}No plugins installed{RESET}");
    } else {
        for name in &installed {
            if let Ok(manifest) = lukan_plugins::PluginManager::load_manifest(name).await {
                let alias = manifest
                    .plugin
                    .alias
                    .as_ref()
                    .map(|a| format!(" {DIM}(alias: {a}){RESET}"))
                    .unwrap_or_default();
                let ptype = &manifest.plugin.plugin_type;
                println!("  {GREEN}✓{RESET} {BOLD}{name}{RESET} [{ptype}]{alias}");
            } else {
                println!("  {YELLOW}!{RESET} {name} {DIM}(manifest error){RESET}");
            }
        }
    }
    println!();

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string())
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    format!("{prefix}...{suffix}")
}

fn format_path_status(path: &std::path::Path) -> String {
    if path.exists() {
        format!("{GREEN}✓{RESET} {}", path.display())
    } else {
        format!("{DIM}✗ {} (not created yet){RESET}", path.display())
    }
}

fn print_key_status(name: &str, key: Option<&str>, env_var: &str) {
    let width = 15;
    match key {
        Some(k) if !k.is_empty() => {
            let masked = mask_key(k);
            println!("  {GREEN}✓{RESET} {name:<width$} {DIM}{masked}{RESET}");
        }
        _ => {
            println!("  {DIM}✗ {name:<width$} not set ({env_var}){RESET}");
        }
    }
}

fn env_var_for_provider(provider: &ProviderName) -> &'static str {
    match provider {
        ProviderName::Anthropic => "ANTHROPIC_API_KEY",
        ProviderName::Nebius => "NEBIUS_API_KEY",
        ProviderName::Fireworks => "FIREWORKS_API_KEY",
        ProviderName::GithubCopilot => "GITHUB_TOKEN",
        ProviderName::OpenaiCodex => "CODEX_ACCESS_TOKEN",
        ProviderName::Zai => "ZAI_API_KEY",
        ProviderName::OpenaiCompatible => "OPENAI_COMPATIBLE_API_KEY",
    }
}
