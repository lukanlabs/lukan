use anyhow::Result;
use console::Style;
use dialoguer::MultiSelect;
use dialoguer::theme::ColorfulTheme;
use regex::Regex;

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
    StaticModel {
        id: "claude-opus-4.6",
        name: "Claude Opus 4.6",
    },
    StaticModel {
        id: "claude-opus-4.5",
        name: "Claude Opus 4.5",
    },
    StaticModel {
        id: "claude-opus-4.1",
        name: "Claude Opus 4.1",
    },
    StaticModel {
        id: "claude-sonnet-4.5",
        name: "Claude Sonnet 4.5",
    },
    StaticModel {
        id: "claude-sonnet-4",
        name: "Claude Sonnet 4",
    },
    StaticModel {
        id: "claude-haiku-4.5",
        name: "Claude Haiku 4.5",
    },
    StaticModel {
        id: "gpt-5.2",
        name: "GPT-5.2",
    },
    StaticModel {
        id: "gpt-5.1",
        name: "GPT-5.1",
    },
    StaticModel {
        id: "gpt-5",
        name: "GPT-5",
    },
    StaticModel {
        id: "gpt-5-mini",
        name: "GPT-5 mini",
    },
    StaticModel {
        id: "gpt-4.1",
        name: "GPT-4.1",
    },
    StaticModel {
        id: "gpt-4o",
        name: "GPT-4o",
    },
    StaticModel {
        id: "gpt-4-turbo",
        name: "GPT-4 Turbo",
    },
    StaticModel {
        id: "gpt-4",
        name: "GPT-4",
    },
    StaticModel {
        id: "gemini-3-pro",
        name: "Gemini 3 Pro",
    },
    StaticModel {
        id: "gemini-3-flash",
        name: "Gemini 3 Flash",
    },
    StaticModel {
        id: "gemini-2.5-pro",
        name: "Gemini 2.5 Pro",
    },
    StaticModel {
        id: "grok-code-fast-1",
        name: "Grok Code Fast 1",
    },
    StaticModel {
        id: "qwen2.5",
        name: "Qwen 2.5",
    },
    StaticModel {
        id: "raptor-mini",
        name: "Raptor mini",
    },
];

const OPENAI_CODEX_MODELS: &[StaticModel] = &[
    StaticModel {
        id: "gpt-5.3-codex",
        name: "GPT-5.3 Codex",
    },
    StaticModel {
        id: "gpt-5.3-codex-spark",
        name: "GPT-5.3 Codex Spark",
    },
    StaticModel {
        id: "gpt-5.2-codex",
        name: "GPT-5.2 Codex",
    },
    StaticModel {
        id: "gpt-5.1-codex-max",
        name: "GPT-5.1 Codex Max",
    },
    StaticModel {
        id: "gpt-5.1-codex",
        name: "GPT-5.1 Codex",
    },
    StaticModel {
        id: "gpt-5.1-codex-mini",
        name: "GPT-5.1 Codex Mini",
    },
    StaticModel {
        id: "gpt-5.2",
        name: "GPT-5.2",
    },
    StaticModel {
        id: "gpt-5.1",
        name: "GPT-5.1",
    },
    StaticModel {
        id: "gpt-5-codex",
        name: "GPT-5 Codex",
    },
    StaticModel {
        id: "gpt-5-codex-mini",
        name: "GPT-5 Codex Mini",
    },
    StaticModel {
        id: "gpt-5",
        name: "GPT-5",
    },
];

const ZAI_MODELS: &[StaticModel] = &[
    StaticModel {
        id: "glm-5",
        name: "GLM-5",
    },
    StaticModel {
        id: "glm-4.7",
        name: "GLM-4.7",
    },
    StaticModel {
        id: "glm-4.6",
        name: "GLM-4.6",
    },
    StaticModel {
        id: "glm-4.5",
        name: "GLM-4.5",
    },
    StaticModel {
        id: "glm-4.5v",
        name: "GLM-4.5V (vision)",
    },
    StaticModel {
        id: "glm-4.1v",
        name: "GLM-4.1V (vision)",
    },
    StaticModel {
        id: "glm-4",
        name: "GLM-4",
    },
];

// ── Theme ──────────────────────────────────────────────────────────────────

fn picker_theme() -> ColorfulTheme {
    ColorfulTheme {
        active_item_style: Style::new().cyan().bold(),
        active_item_prefix: console::style("❯ ".to_string()).cyan().bold(),
        inactive_item_prefix: console::style("  ".to_string()),
        checked_item_prefix: console::style("◉ ".to_string()).green(),
        unchecked_item_prefix: console::style("◯ ".to_string()).dim(),
        prompt_prefix: console::style("? ".to_string()).cyan().bold(),
        ..ColorfulTheme::default()
    }
}

/// Get currently configured model IDs for a provider prefix.
async fn get_existing_model_ids(provider_prefix: &str) -> Vec<String> {
    let prefix = format!("{provider_prefix}:");
    ConfigManager::load()
        .await
        .ok()
        .and_then(|c| c.models)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.strip_prefix(&prefix).map(|s| s.to_string()))
        .collect()
}

/// Compute which items should be pre-checked based on existing config.
fn defaults_for(model_ids: &[String], existing: &[String]) -> Vec<bool> {
    model_ids.iter().map(|id| existing.contains(id)).collect()
}

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
        "github-copilot" | "copilot" => {
            select_static("github-copilot", GITHUB_COPILOT_MODELS, true).await
        }
        "openai-codex" | "codex" => select_static("openai-codex", OPENAI_CODEX_MODELS, true).await,
        "zai" | "z.ai" => select_static("zai", ZAI_MODELS, false).await,
        "ollama-cloud" | "ollama" => select_ollama_cloud().await,
        "openai-compatible" | "oai-compatible" => select_openai_compatible().await,
        "lukan-cloud" | "lukan" => select_lukan_cloud().await,
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
    println!("  {CYAN}ollama-cloud{RESET}      Ollama Cloud (requires API key)");
    println!("  {CYAN}openai-compatible{RESET}  Generic OpenAI-compatible endpoint");
    println!("  {CYAN}zai{RESET}               z.ai (GLM models)");
    println!("  {CYAN}lukan-cloud{RESET}       Lukan Cloud (requires API key)");
    println!();
    println!("{BOLD}Examples:{RESET}");
    println!("  lukan models anthropic");
    println!("  lukan models github-copilot");
    println!("  lukan models lukan");
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
        eprintln!(
            "{RED}Error:{RESET} Anthropic API key not found. Set ANTHROPIC_API_KEY or run: lukan setup"
        );
        return Ok(());
    }

    println!("{DIM}Fetching models from Anthropic API...{RESET}");
    let models = lukan_providers::anthropic::fetch_anthropic_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let existing = get_existing_model_ids("anthropic").await;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let items: Vec<String> = models
        .iter()
        .map(|m| format!("{:<30} {}", m.display_name, m.id))
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("anthropic:{}", models[i].id))
        .collect();
    let vision_ids: Vec<String> = selected.iter().map(|&i| models[i].id.clone()).collect();

    ConfigManager::set_provider_models("anthropic", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all anthropic models.");
    } else {
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for anthropic {MAGENTA}(all vision-capable){RESET}:",
            selected.len()
        );
        for &idx in &selected {
            println!("  - anthropic:{}", models[idx].id);
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Nebius (API fetch) ─────────────────────────────────────────────────────

async fn select_nebius() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.nebius_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!(
            "{RED}Error:{RESET} Nebius API key not found. Set NEBIUS_API_KEY or run: lukan setup"
        );
        return Ok(());
    }

    println!("{DIM}Fetching models from Nebius API...{RESET}");
    let models = lukan_providers::nebius::fetch_nebius_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let existing = get_existing_model_ids("nebius").await;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let items: Vec<String> = models
        .iter()
        .map(|m| {
            let vision = if m.supports_image_input {
                " [vision]"
            } else {
                ""
            };
            format!("{}{vision}", m.id)
        })
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("nebius:{}", models[i].id))
        .collect();
    let vision_ids: Vec<String> = selected
        .iter()
        .filter(|&&i| models[i].supports_image_input)
        .map(|&i| models[i].id.clone())
        .collect();

    ConfigManager::set_provider_models("nebius", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all nebius models.");
    } else {
        let vision_count = vision_ids.len();
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for nebius:",
            selected.len()
        );
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
            println!("\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s).");
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Fireworks (API fetch) ──────────────────────────────────────────────────

async fn select_fireworks() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.fireworks_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!(
            "{RED}Error:{RESET} Fireworks API key not found. Set FIREWORKS_API_KEY or run: lukan setup"
        );
        return Ok(());
    }

    println!("{DIM}Fetching models from Fireworks API...{RESET}");
    let models = lukan_providers::fireworks::fetch_fireworks_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let existing = get_existing_model_ids("fireworks").await;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let items: Vec<String> = models
        .iter()
        .map(|m| {
            let vision = if m.supports_image_input {
                " [vision]"
            } else {
                ""
            };
            format!("{:<35} {}{vision}", m.display_name, m.id)
        })
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("fireworks:{}", models[i].id))
        .collect();
    let vision_ids: Vec<String> = selected
        .iter()
        .filter(|&&i| models[i].supports_image_input)
        .map(|&i| models[i].id.clone())
        .collect();

    ConfigManager::set_provider_models("fireworks", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all fireworks models.");
    } else {
        let vision_count = vision_ids.len();
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for fireworks:",
            selected.len()
        );
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
            println!("\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s).");
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Ollama Cloud (API fetch) ────────────────────────────────────────────────

async fn select_ollama_cloud() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.ollama_cloud_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!(
            "{RED}Error:{RESET} Ollama Cloud API key not found. Set OLLAMA_API_KEY or run: lukan setup"
        );
        return Ok(());
    }

    println!("{DIM}Fetching models from Ollama Cloud...{RESET}");
    let models = lukan_providers::ollama_cloud::fetch_ollama_cloud_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let existing = get_existing_model_ids("ollama-cloud").await;
    let model_ids: Vec<String> = models.iter().map(|m| m.name.clone()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let vision_flags: Vec<bool> = model_ids.iter().map(|id| is_vision_model(id)).collect();

    let items: Vec<String> = models
        .iter()
        .zip(vision_flags.iter())
        .map(|(m, &is_vision)| {
            if is_vision {
                format!("{} [vision]", m.name)
            } else {
                m.name.clone()
            }
        })
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("ollama-cloud:{}", models[i].name))
        .collect();

    let vision_ids: Vec<String> = selected
        .iter()
        .filter(|&&i| vision_flags[i])
        .map(|&i| models[i].name.clone())
        .collect();

    ConfigManager::set_provider_models("ollama-cloud", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all ollama-cloud models.");
    } else {
        let vision_count = vision_ids.len();
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for ollama-cloud:",
            selected.len()
        );
        for &idx in &selected {
            let badge = if vision_flags[idx] {
                format!(" {MAGENTA}(vision){RESET}")
            } else {
                String::new()
            };
            println!("  - ollama-cloud:{}{badge}", models[idx].name);
        }
        if vision_count > 0 {
            println!("\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s).");
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Lukan Cloud (API fetch) ─────────────────────────────────────────────────

async fn select_lukan_cloud() -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let api_key = creds.lukan_cloud_api_key.as_deref().unwrap_or("");

    if api_key.is_empty() {
        eprintln!(
            "{RED}Error:{RESET} Lukan Cloud API key not found. Set LUKAN_CLOUD_API_KEY or run: lukan setup"
        );
        return Ok(());
    }

    println!("{DIM}Fetching models from Lukan Cloud...{RESET}");
    let models = lukan_providers::lukan_cloud::fetch_lukan_cloud_models(api_key).await?;

    if models.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let existing = get_existing_model_ids("lukan-cloud").await;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let items: Vec<String> = models
        .iter()
        .map(|m| {
            let tier = format!("[{}]", m.tier);
            format!("{:<30} {:<12} {}", m.name, tier, m.id)
        })
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("lukan-cloud:{}", models[i].id))
        .collect();

    // All models on Lukan Cloud support vision (proxied Anthropic/OpenAI)
    let vision_ids: Vec<String> = selected.iter().map(|&i| models[i].id.clone()).collect();

    ConfigManager::set_provider_models("lukan-cloud", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all lukan-cloud models.");
    } else {
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for lukan-cloud:",
            selected.len()
        );
        for &idx in &selected {
            println!("  - lukan-cloud:{}", models[idx].id);
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}

// ── Vision detection ────────────────────────────────────────────────────────

/// Detect vision-capable models by ID using the same patterns as the Node.js provider.
fn is_vision_model(model_id: &str) -> bool {
    let patterns = [
        r"(?i)\bvl\b",
        r"(?i)vision",
        r"(?i)-v\d*$",
        r"(?i)multimodal",
        r"(?i)qwen2\.5-vl",
        r"(?i)llava",
        r"(?i)llama-4",
    ];
    patterns
        .iter()
        .any(|p| Regex::new(p).unwrap().is_match(model_id))
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

    let normalized = lukan_providers::openai_compat::normalize_base_url(base_url);

    println!("{DIM}Fetching models from {normalized}...{RESET}");

    let client = reqwest::Client::new();
    let url = format!("{}/models", normalized.trim_end_matches('/'));

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

    let existing = get_existing_model_ids("openai-compatible").await;
    let checked = defaults_for(&model_ids, &existing);

    let vision_flags: Vec<bool> = model_ids.iter().map(|id| is_vision_model(id)).collect();

    let items: Vec<String> = model_ids
        .iter()
        .zip(vision_flags.iter())
        .map(|(id, &is_vision)| {
            if is_vision {
                format!("{id} [vision]")
            } else {
                id.clone()
            }
        })
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("openai-compatible:{}", model_ids[i]))
        .collect();

    let vision_ids: Vec<String> = selected
        .iter()
        .filter(|&&i| vision_flags[i])
        .map(|&i| model_ids[i].clone())
        .collect();

    ConfigManager::set_provider_models("openai-compatible", &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all openai-compatible models.");
    } else {
        let vision_count = vision_ids.len();
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for openai-compatible:",
            selected.len()
        );
        for &idx in &selected {
            let badge = if vision_flags[idx] {
                format!(" {MAGENTA}(vision){RESET}")
            } else {
                String::new()
            };
            println!("  - openai-compatible:{}{badge}", model_ids[idx]);
        }
        if vision_count > 0 {
            println!("\n{MAGENTA}✓{RESET} Auto-tagged {vision_count} vision-capable model(s).");
        }
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
    let existing = get_existing_model_ids(provider_name).await;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.to_string()).collect();
    let checked = defaults_for(&model_ids, &existing);

    let items: Vec<String> = models
        .iter()
        .map(|m| format!("{:<25} {}", m.name, m.id))
        .collect();

    let selected = MultiSelect::with_theme(&picker_theme())
        .with_prompt("Select models (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&checked)
        .interact()?;

    let entries: Vec<String> = selected
        .iter()
        .map(|&i| format!("{provider_name}:{}", models[i].id))
        .collect();

    let vision_ids: Vec<String> = selected
        .iter()
        .filter(|&&i| {
            all_vision || models[i].name.contains("vision") || models[i].name.contains("Vision")
        })
        .map(|&i| models[i].id.to_string())
        .collect();

    ConfigManager::set_provider_models(provider_name, &entries, &vision_ids).await?;

    if selected.is_empty() {
        println!("{GREEN}✓{RESET} Cleared all {provider_name} models.");
    } else {
        let vision_label = if all_vision {
            format!(" {MAGENTA}(all vision-capable){RESET}")
        } else {
            String::new()
        };
        println!(
            "\n{GREEN}✓{RESET} Set {} model(s) for {provider_name}{vision_label}:",
            selected.len()
        );
        for &idx in &selected {
            println!("  - {provider_name}:{}", models[idx].id);
        }
    }
    println!("\nUse /model in chat to switch between models.");
    Ok(())
}
