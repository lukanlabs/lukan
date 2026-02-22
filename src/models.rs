use anyhow::Result;
use std::io::{self, Write};

use lukan_core::config::{ConfigManager, CredentialsManager};

// ── Colors ─────────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";

// ── Static model lists (for providers without catalog APIs) ────────────────

struct StaticModel {
    id: &'static str,
    name: &'static str,
}

const GITHUB_COPILOT_MODELS: &[StaticModel] = &[
    StaticModel { id: "claude-opus-4.6", name: "Claude Opus 4.6" },
    StaticModel { id: "claude-opus-4.5", name: "Claude Opus 4.5" },
    StaticModel { id: "claude-opus-4.1", name: "Claude Opus 4.1" },
    StaticModel { id: "claude-sonnet-4.5", name: "Claude Sonnet 4.5" },
    StaticModel { id: "claude-sonnet-4", name: "Claude Sonnet 4" },
    StaticModel { id: "claude-haiku-4.5", name: "Claude Haiku 4.5" },
    StaticModel { id: "gpt-5.2", name: "GPT-5.2" },
    StaticModel { id: "gpt-5.1", name: "GPT-5.1" },
    StaticModel { id: "gpt-5", name: "GPT-5" },
    StaticModel { id: "gpt-5-mini", name: "GPT-5 mini" },
    StaticModel { id: "gpt-4.1", name: "GPT-4.1" },
    StaticModel { id: "gpt-4o", name: "GPT-4o" },
    StaticModel { id: "gpt-4-turbo", name: "GPT-4 Turbo" },
    StaticModel { id: "gpt-4", name: "GPT-4" },
    StaticModel { id: "gemini-3-pro", name: "Gemini 3 Pro" },
    StaticModel { id: "gemini-3-flash", name: "Gemini 3 Flash" },
    StaticModel { id: "gemini-2.5-pro", name: "Gemini 2.5 Pro" },
    StaticModel { id: "grok-code-fast-1", name: "Grok Code Fast 1" },
    StaticModel { id: "qwen2.5", name: "Qwen 2.5" },
    StaticModel { id: "raptor-mini", name: "Raptor mini" },
];

const OPENAI_CODEX_MODELS: &[StaticModel] = &[
    StaticModel { id: "gpt-5.3-codex", name: "GPT-5.3 Codex" },
    StaticModel { id: "gpt-5.3-codex-spark", name: "GPT-5.3 Codex Spark" },
    StaticModel { id: "gpt-5.2-codex", name: "GPT-5.2 Codex" },
    StaticModel { id: "gpt-5.1-codex-max", name: "GPT-5.1 Codex Max" },
    StaticModel { id: "gpt-5.1-codex", name: "GPT-5.1 Codex" },
    StaticModel { id: "gpt-5.1-codex-mini", name: "GPT-5.1 Codex Mini" },
    StaticModel { id: "gpt-5.2", name: "GPT-5.2" },
    StaticModel { id: "gpt-5.1", name: "GPT-5.1" },
    StaticModel { id: "gpt-5-codex", name: "GPT-5 Codex" },
    StaticModel { id: "gpt-5-codex-mini", name: "GPT-5 Codex Mini" },
    StaticModel { id: "gpt-5", name: "GPT-5" },
];

const ZAI_MODELS: &[StaticModel] = &[
    StaticModel { id: "glm-5", name: "GLM-5" },
    StaticModel { id: "glm-4.7", name: "GLM-4.7" },
    StaticModel { id: "glm-4.6", name: "GLM-4.6" },
    StaticModel { id: "glm-4.5", name: "GLM-4.5" },
    StaticModel { id: "glm-4.5v", name: "GLM-4.5V (vision)" },
    StaticModel { id: "glm-4.1v", name: "GLM-4.1V (vision)" },
    StaticModel { id: "glm-4", name: "GLM-4" },
];

// ── Main entry point ───────────────────────────────────────────────────────

pub async fn run_models(provider: Option<&str>, model_entry: Option<&str>) -> Result<()> {
    let Some(provider) = provider else {
        print_usage();
        return Ok(());
    };

    // Handle manual add
    if provider == "add" {
        return handle_add(model_entry).await;
    }

    // Provider-specific listing
    match provider {
        "anthropic" | "claude" => select_anthropic().await,
        "nebius" => select_nebius().await,
        "fireworks" => select_fireworks().await,
        "github-copilot" | "copilot" => select_static("github-copilot", GITHUB_COPILOT_MODELS, true).await,
        "openai-codex" | "codex" => select_static("openai-codex", OPENAI_CODEX_MODELS, true).await,
        "zai" | "z.ai" => select_static("zai", ZAI_MODELS, false).await,
        "openai-compatible" | "oai-compatible" => select_openai_compatible().await,
        other => {
            eprintln!("{RED}Error:{RESET} Provider \"{other}\" does not have a catalog API.");
            println!("\nAdd models manually:");
            println!("  lukan models add {other}:<model-id>");
            Ok(())
        }
    }
}

fn print_usage() {
    println!("{BOLD}Usage:{RESET}");
    println!("  lukan models <provider>           Interactive model selector");
    println!("  lukan models add <provider:model> Add model manually");
    println!();
    println!("{BOLD}Providers with catalog API:{RESET}");
    println!("  {CYAN}anthropic{RESET}         Anthropic Claude models (requires API key)");
    println!("  {CYAN}fireworks{RESET}         Fireworks AI models (requires API key)");
    println!("  {CYAN}nebius{RESET}            Nebius AI models (requires API key)");
    println!("  {CYAN}github-copilot{RESET}    GitHub Copilot (Premium models)");
    println!("  {CYAN}openai-codex{RESET}      OpenAI Codex (ChatGPT subscription)");
    println!("  {CYAN}openai-compatible{RESET}  Generic OpenAI-compatible endpoint");
    println!("  {CYAN}zai{RESET}               z.ai (GLM models)");
    println!();
    println!("{BOLD}Examples:{RESET}");
    println!("  lukan models anthropic");
    println!("  lukan models github-copilot");
    println!("  lukan models add nebius:deepseek-ai/DeepSeek-R1");
}

// ── Manual add ─────────────────────────────────────────────────────────────

async fn handle_add(model_entry: Option<&str>) -> Result<()> {
    let Some(entry) = model_entry else {
        eprintln!("{RED}Error:{RESET} Format: lukan models add <provider>:<model-id>");
        println!("\nExamples:");
        println!("  lukan models add nebius:deepseek-ai/DeepSeek-R1");
        println!("  lukan models add anthropic:claude-sonnet-4-5-20250929");
        return Ok(());
    };

    if !entry.contains(':') {
        eprintln!("{RED}Error:{RESET} Format: lukan models add <provider>:<model-id>");
        return Ok(());
    }

    ConfigManager::add_model(entry).await?;
    println!("{GREEN}✓{RESET} Model added: {BOLD}{entry}{RESET}");
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Anthropic (API fetch) ──────────────────────────────────────────────────

async fn select_anthropic() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.anthropic_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!("{RED}Error:{RESET} Anthropic API key not found. Set ANTHROPIC_API_KEY or run: lukan setup");
        return Ok(());
    }

    println!("{DIM}Fetching models from Anthropic API...{RESET}");
    let models = lukan_providers::anthropic::fetch_anthropic_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    // Display numbered list
    println!();
    for (i, m) in models.iter().enumerate() {
        println!(
            "  {DIM}{:>3}.{RESET} {:<30} {DIM}{}{RESET}",
            i + 1,
            m.display_name,
            m.id
        );
    }

    // Prompt selection
    let selected = prompt_selection(models.len())?;

    if selected.is_empty() {
        println!("No models selected.");
        return Ok(());
    }

    for &idx in &selected {
        let m = &models[idx];
        let entry = format!("anthropic:{}", m.id);
        ConfigManager::add_model(&entry).await?;
        ConfigManager::add_vision_model(&m.id).await?; // All Anthropic models support vision
    }

    println!(
        "\n{GREEN}✓{RESET} Added {} model(s) {MAGENTA}(all vision-capable){RESET}:",
        selected.len()
    );
    for &idx in &selected {
        println!("  - anthropic:{}", models[idx].id);
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Nebius (API fetch) ─────────────────────────────────────────────────────

async fn select_nebius() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.nebius_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!("{RED}Error:{RESET} Nebius API key not found. Set NEBIUS_API_KEY or run: lukan setup");
        return Ok(());
    }

    println!("{DIM}Fetching models from Nebius API...{RESET}");
    let models = lukan_providers::nebius::fetch_nebius_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    println!();
    for (i, m) in models.iter().enumerate() {
        let vision = if m.supports_image_input { " [vision]" } else { "" };
        println!(
            "  {DIM}{:>3}.{RESET} {}{MAGENTA}{vision}{RESET}",
            i + 1,
            m.id,
        );
    }

    let selected = prompt_selection(models.len())?;

    if selected.is_empty() {
        println!("No models selected.");
        return Ok(());
    }

    let mut vision_count = 0;
    for &idx in &selected {
        let m = &models[idx];
        let entry = format!("nebius:{}", m.id);
        ConfigManager::add_model(&entry).await?;
        if m.supports_image_input {
            ConfigManager::add_vision_model(&m.id).await?;
            vision_count += 1;
        }
    }

    println!("\n{GREEN}✓{RESET} Added {} model(s):", selected.len());
    for &idx in &selected {
        let m = &models[idx];
        let badge = if m.supports_image_input {
            format!(" {MAGENTA}(vision){RESET}")
        } else {
            String::new()
        };
        println!("  - nebius:{}{badge}", m.id);
    }
    if vision_count > 0 {
        println!(
            "\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s)."
        );
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Fireworks (API fetch) ──────────────────────────────────────────────────

async fn select_fireworks() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.fireworks_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!("{RED}Error:{RESET} Fireworks API key not found. Set FIREWORKS_API_KEY or run: lukan setup");
        return Ok(());
    }

    println!("{DIM}Fetching models from Fireworks API...{RESET}");
    let models = lukan_providers::fireworks::fetch_fireworks_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    println!();
    for (i, m) in models.iter().enumerate() {
        let vision = if m.supports_image_input { " [vision]" } else { "" };
        println!(
            "  {DIM}{:>3}.{RESET} {:<35} {DIM}{}{RESET}{MAGENTA}{vision}{RESET}",
            i + 1,
            m.display_name,
            m.id,
        );
    }

    let selected = prompt_selection(models.len())?;

    if selected.is_empty() {
        println!("No models selected.");
        return Ok(());
    }

    let mut vision_count = 0;
    for &idx in &selected {
        let m = &models[idx];
        let entry = format!("fireworks:{}", m.id);
        ConfigManager::add_model(&entry).await?;
        if m.supports_image_input {
            ConfigManager::add_vision_model(&m.id).await?;
            vision_count += 1;
        }
    }

    println!("\n{GREEN}✓{RESET} Added {} model(s):", selected.len());
    for &idx in &selected {
        let m = &models[idx];
        let badge = if m.supports_image_input {
            format!(" {MAGENTA}(vision){RESET}")
        } else {
            String::new()
        };
        println!("  - fireworks:{}{badge}", m.id);
    }
    if vision_count > 0 {
        println!(
            "\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s)."
        );
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── OpenAI-compatible (API fetch) ──────────────────────────────────────────

async fn select_openai_compatible() -> Result<()> {
    let config = ConfigManager::load().await?;
    let base_url = config
        .openai_compatible_base_url
        .as_deref()
        .unwrap_or("")
        .trim();

    if base_url.is_empty() {
        eprintln!("{RED}Error:{RESET} openaiCompatibleBaseURL is not set.");
        println!("\nRun:");
        println!("  lukan config set openaiCompatibleBaseURL http://localhost:8080/v1");
        return Ok(());
    }

    let creds = CredentialsManager::load().await?;
    let api_key = creds.openai_compatible_api_key.as_deref().unwrap_or("");

    println!("{DIM}Fetching models from {base_url}...{RESET}");

    let client = reqwest::Client::new();
    let normalized = base_url.trim_end_matches('/');
    let url = format!("{normalized}/models");

    let mut req = client.get(&url).header("accept", "application/json");
    if !api_key.is_empty() {
        req = req.header("authorization", format!("Bearer {api_key}"));
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("API error: {status} {body}");
    }

    let data: serde_json::Value = resp.json().await?;
    let model_ids: Vec<String> = data["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if model_ids.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    println!();
    for (i, id) in model_ids.iter().enumerate() {
        println!("  {DIM}{:>3}.{RESET} {id}", i + 1);
    }

    let selected = prompt_selection(model_ids.len())?;

    if selected.is_empty() {
        println!("No models selected.");
        return Ok(());
    }

    for &idx in &selected {
        let entry = format!("openai-compatible:{}", model_ids[idx]);
        ConfigManager::add_model(&entry).await?;
    }

    println!("\n{GREEN}✓{RESET} Added {} model(s):", selected.len());
    for &idx in &selected {
        println!("  - openai-compatible:{}", model_ids[idx]);
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Static model list (Copilot, Codex, z.ai) ──────────────────────────────

async fn select_static(
    provider_name: &str,
    models: &[StaticModel],
    all_vision: bool,
) -> Result<()> {
    println!();
    for (i, m) in models.iter().enumerate() {
        println!(
            "  {DIM}{:>3}.{RESET} {:<25} {DIM}{}{RESET}",
            i + 1,
            m.name,
            m.id,
        );
    }

    let selected = prompt_selection(models.len())?;

    if selected.is_empty() {
        println!("No models selected.");
        return Ok(());
    }

    for &idx in &selected {
        let m = &models[idx];
        let entry = format!("{provider_name}:{}", m.id);
        ConfigManager::add_model(&entry).await?;
        if all_vision || m.name.contains("vision") || m.name.contains("Vision") {
            ConfigManager::add_vision_model(m.id).await?;
        }
    }

    let vision_label = if all_vision {
        format!(" {MAGENTA}(all vision-capable){RESET}")
    } else {
        String::new()
    };
    println!(
        "\n{GREEN}✓{RESET} Added {} model(s){vision_label}:",
        selected.len()
    );
    for &idx in &selected {
        println!("  - {provider_name}:{}", models[idx].id);
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Selection prompt ───────────────────────────────────────────────────────

/// Prompt the user to enter model numbers (comma/space separated, or ranges).
/// Returns 0-indexed indices of selected models.
fn prompt_selection(total: usize) -> Result<Vec<usize>> {
    println!();
    print!(
        "{BOLD}Select models{RESET} {DIM}(e.g. 1,3,5 or 1-5 or 'all'){RESET}: "
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        return Ok(vec![]);
    }

    if input == "all" {
        return Ok((0..total).collect());
    }

    let mut indices = Vec::new();

    for part in input.split([',', ' ']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start.trim().parse().unwrap_or(0);
            let end: usize = end.trim().parse().unwrap_or(0);
            if start >= 1 && end >= start && end <= total {
                for i in start..=end {
                    indices.push(i - 1);
                }
            }
        } else if let Ok(n) = part.parse::<usize>()
            && n >= 1
            && n <= total
        {
            indices.push(n - 1);
        }
    }

    indices.sort();
    indices.dedup();
    Ok(indices)
}
