use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Subcommand;
use lukan_core::config::CredentialsManager;

// ── Colors ─────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";

// ── CLI subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SkillCommands {
    /// List installed skills
    List,
    /// Remove an installed skill
    Remove {
        /// Skill folder name
        name: String,
    },
    /// Manage environment variables for a skill
    Env {
        /// Skill folder name
        name: String,
        /// Action: "set" or "unset" (omit to list)
        action: Option<String>,
        /// Variable name (required for set/unset)
        key: Option<String>,
        /// Variable value (required for set)
        value: Option<String>,
    },
}

// ── Dispatch ───────────────────────────────────────────────────────────

pub async fn handle_skill_command(command: SkillCommands) -> Result<()> {
    match command {
        SkillCommands::List => skill_list().await,
        SkillCommands::Remove { name } => skill_remove(&name).await,
        SkillCommands::Env {
            name,
            action,
            key,
            value,
        } => skill_env(&name, action.as_deref(), key.as_deref(), value.as_deref()).await,
    }
}

// ── List ───────────────────────────────────────────────────────────────

async fn skill_list() -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let skills = lukan_tools::skills::discover_skills(&cwd).await;

    if skills.is_empty() {
        println!("{YELLOW}No skills installed.{RESET}");
        println!("{DIM}Copy a skill folder into .lukan/skills/ to install it.{RESET}");
        return Ok(());
    }

    println!("{BOLD}Installed skills:{RESET}\n");
    for s in &skills {
        println!("  {CYAN}{}{RESET}", s.folder);
        println!("    {DIM}{} — {}{RESET}", s.name, s.description);
    }

    Ok(())
}

// ── Env ────────────────────────────────────────────────────────────────

async fn skill_env(
    name: &str,
    action: Option<&str>,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<()> {
    match action {
        None => skill_env_list(name).await,
        Some("set") => {
            let key = key.ok_or_else(|| {
                anyhow::anyhow!("Usage: lukan skill env {name} set <KEY> <VALUE>")
            })?;
            let value = value.ok_or_else(|| {
                anyhow::anyhow!("Usage: lukan skill env {name} set {key} <VALUE>")
            })?;
            skill_env_set(name, key, value).await
        }
        Some("unset") => {
            let key =
                key.ok_or_else(|| anyhow::anyhow!("Usage: lukan skill env {name} unset <KEY>"))?;
            skill_env_unset(name, key).await
        }
        Some(other) => anyhow::bail!("Unknown action '{other}'. Use 'set' or 'unset'."),
    }
}

async fn skill_env_list(name: &str) -> Result<()> {
    let creds = CredentialsManager::load().await?;
    let empty = creds
        .skill_credentials
        .get(name)
        .map(|m| m.is_empty())
        .unwrap_or(true);

    if empty {
        println!("{YELLOW}No env vars configured for skill '{name}'.{RESET}");
        println!("{DIM}Set one with: lukan skill env {name} set <KEY> <VALUE>{RESET}");
    } else {
        let map = &creds.skill_credentials[name];
        println!("{BOLD}Env vars for '{CYAN}{name}{RESET}{BOLD}':{RESET}\n");
        for (k, v) in map {
            let redacted = redact_value(v);
            println!("  {BOLD}{k}{RESET} = {DIM}{redacted}{RESET}");
        }
    }
    Ok(())
}

async fn skill_env_set(name: &str, key: &str, value: &str) -> Result<()> {
    let mut creds = CredentialsManager::load().await?;
    creds
        .skill_credentials
        .entry(name.to_string())
        .or_insert_with(HashMap::new)
        .insert(key.to_string(), value.to_string());
    CredentialsManager::save(&creds).await?;
    println!("{GREEN}✓{RESET} Set {BOLD}{key}{RESET} for skill '{CYAN}{name}{RESET}'.");
    Ok(())
}

async fn skill_env_unset(name: &str, key: &str) -> Result<()> {
    let mut creds = CredentialsManager::load().await?;
    let removed = creds
        .skill_credentials
        .get_mut(name)
        .and_then(|m| m.remove(key))
        .is_some();

    // Clean up empty map
    if let Some(map) = creds.skill_credentials.get(name)
        && map.is_empty()
    {
        creds.skill_credentials.remove(name);
    }

    CredentialsManager::save(&creds).await?;
    if removed {
        println!("{GREEN}✓{RESET} Removed {BOLD}{key}{RESET} from skill '{CYAN}{name}{RESET}'.");
    } else {
        println!("{YELLOW}Key '{key}' was not set for skill '{name}'.{RESET}");
    }
    Ok(())
}

/// Redact a credential value, showing first 4 and last 2 chars.
fn redact_value(v: &str) -> String {
    if v.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...{}", &v[..4], &v[v.len() - 2..])
}

// ── Remove ─────────────────────────────────────────────────────────────

async fn skill_remove(name: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let skill_dir = cwd.join(".lukan").join("skills").join(name);

    if !skill_dir.exists() {
        anyhow::bail!("Skill '{name}' not found in .lukan/skills/.");
    }

    tokio::fs::remove_dir_all(&skill_dir).await?;
    println!("{GREEN}✓{RESET} Skill '{CYAN}{name}{RESET}' removed.");

    Ok(())
}
