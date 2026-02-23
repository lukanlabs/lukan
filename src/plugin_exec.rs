use anyhow::{Context, Result};

use lukan_core::config::LukanPaths;
use lukan_plugins::PluginManager;

const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// Execute a custom plugin command defined in the manifest's [commands] section.
///
/// Runs: `<run.command> cli.js <handler> [args...]` in the plugin directory.
pub async fn handle_plugin_exec(name: &str, command: &str, args: &[String]) -> Result<()> {
    let manifest = PluginManager::load_manifest(name).await?;

    let cmd_def = manifest.commands.get(command).ok_or_else(|| {
        let available: Vec<&String> = manifest.commands.keys().collect();
        if available.is_empty() {
            anyhow::anyhow!("Plugin '{name}' has no custom commands.")
        } else {
            anyhow::anyhow!(
                "Unknown command '{command}' for plugin '{name}'.\nAvailable: {}",
                available
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    })?;

    let plugin_dir = LukanPaths::plugin_dir(name);
    let cli_script = plugin_dir.join("cli.js");

    if !cli_script.exists() {
        anyhow::bail!(
            "Plugin '{name}' declares command '{command}' but cli.js not found at {}",
            cli_script.display()
        );
    }

    // Build command: <run.command> cli.js <handler> [args...]
    // Default to "node" if no [run] section in manifest
    let run_command = manifest
        .run
        .as_ref()
        .map(|r| r.command.as_str())
        .unwrap_or("node");

    let mut cmd_args = vec![
        cli_script.to_string_lossy().to_string(),
        cmd_def.handler.clone(),
    ];
    cmd_args.extend_from_slice(args);

    let status = std::process::Command::new(run_command)
        .args(&cmd_args)
        .current_dir(&plugin_dir)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to execute plugin command")?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        println!("{RED}Command exited with code {code}{RESET}");
    }

    Ok(())
}
