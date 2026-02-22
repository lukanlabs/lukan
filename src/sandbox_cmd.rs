use anyhow::Result;
use clap::Subcommand;
use lukan_tools::sandbox;

// ── Colors ─────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

/// OS-level sandbox management subcommands (bwrap)
#[derive(Subcommand)]
pub enum SandboxCommands {
    /// Show sandbox status and diagnostics
    Status,
    /// Enable OS-level sandbox
    On,
    /// Disable OS-level sandbox
    Off,
    /// Install AppArmor profile for bwrap (requires sudo)
    Setup,
    /// Remove AppArmor profile for bwrap (requires sudo)
    Uninstall,
}

/// Handle sandbox CLI subcommands
pub async fn handle_sandbox_command(cmd: SandboxCommands) -> Result<()> {
    match cmd {
        SandboxCommands::Status => sandbox_status().await,
        SandboxCommands::On => sandbox_on().await,
        SandboxCommands::Off => sandbox_off().await,
        SandboxCommands::Setup => sandbox_setup().await,
        SandboxCommands::Uninstall => sandbox_uninstall().await,
    }
}

async fn sandbox_status() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan sandbox status{RESET}");
    println!("{DIM}  OS-level sandbox diagnostics{RESET}\n");

    // Check project config for os_sandbox setting
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_config = lukan_core::config::ProjectConfig::load(&cwd).await?;
    let os_sandbox_enabled = project_config
        .as_ref()
        .map(|(_, cfg)| cfg.permissions.os_sandbox)
        .unwrap_or(true);

    let config_source = if project_config.is_some() {
        "project (.lukan/config.json)"
    } else {
        "default"
    };

    println!(
        "  Config source:     {DIM}{config_source}{RESET}"
    );
    println!(
        "  os_sandbox:        {}",
        if os_sandbox_enabled {
            format!("{GREEN}enabled{RESET}")
        } else {
            format!("{YELLOW}disabled{RESET}")
        }
    );

    // bwrap availability
    let bwrap_available = sandbox::is_bwrap_available();
    println!(
        "  bwrap available:   {}",
        if bwrap_available {
            format!("{GREEN}yes{RESET}")
        } else {
            format!("{RED}no{RESET}")
        }
    );

    // AppArmor profile
    let has_profile = sandbox::has_apparmor_profile();
    println!(
        "  AppArmor profile:  {}",
        if has_profile {
            format!("{GREEN}installed{RESET}")
        } else {
            format!("{DIM}not installed{RESET}")
        }
    );

    // Diagnosis
    let diagnosis = sandbox::diagnose_bwrap();
    println!("\n  {BOLD}Diagnosis:{RESET} {diagnosis}");

    // Effective state
    let effective = os_sandbox_enabled && bwrap_available;
    println!(
        "\n  {BOLD}Effective:{RESET} {}",
        if effective {
            format!("{GREEN}Sandbox is active{RESET}")
        } else if os_sandbox_enabled {
            format!("{YELLOW}Sandbox enabled but bwrap not available{RESET}")
        } else {
            format!("{YELLOW}Sandbox is disabled{RESET}")
        }
    );

    println!();
    Ok(())
}

async fn sandbox_on() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_path = cwd.join(".lukan").join("config.json");

    if !config_path.exists() {
        println!(
            "{YELLOW}No .lukan/config.json found. Run `lukan init` first to create project config.{RESET}"
        );
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&config_path).await?;
    let mut config: lukan_core::config::ProjectConfig = serde_json::from_str(&content)?;
    config.permissions.os_sandbox = true;
    let updated = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, updated).await?;

    println!("{GREEN}Sandbox enabled.{RESET} Bash commands will run inside bwrap (if available).");
    Ok(())
}

async fn sandbox_off() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_path = cwd.join(".lukan").join("config.json");

    if !config_path.exists() {
        println!(
            "{YELLOW}No .lukan/config.json found. Run `lukan init` first to create project config.{RESET}"
        );
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&config_path).await?;
    let mut config: lukan_core::config::ProjectConfig = serde_json::from_str(&content)?;
    config.permissions.os_sandbox = false;
    let updated = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, updated).await?;

    println!("{YELLOW}Sandbox disabled.{RESET} Bash commands will run without OS-level isolation.");
    Ok(())
}

async fn sandbox_setup() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan sandbox setup{RESET}");
    println!("{DIM}  Installing AppArmor profile for bwrap{RESET}\n");

    let message = sandbox::setup_apparmor()?;
    println!("  {message}");
    println!();
    Ok(())
}

async fn sandbox_uninstall() -> Result<()> {
    println!("\n{BOLD}{CYAN}  lukan sandbox uninstall{RESET}");
    println!("{DIM}  Removing AppArmor profile for bwrap{RESET}\n");

    let message = sandbox::uninstall_apparmor()?;
    println!("  {message}");
    println!();
    Ok(())
}
