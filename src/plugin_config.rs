use anyhow::{Context, Result};
use console::Style;
use dialoguer::MultiSelect;
use dialoguer::theme::ColorfulTheme;

use lukan_core::config::{LukanPaths, TOOL_GROUPS};
use lukan_core::models::plugin::ConfigFieldType;
use lukan_plugins::PluginManager;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

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

/// Convert a snake_case key to camelCase (for JSON config files).
pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Load a plugin's config.json as a serde_json::Value.
pub async fn load_plugin_config(name: &str) -> Result<serde_json::Value> {
    let config_path = LukanPaths::plugin_config(name);
    if !config_path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = tokio::fs::read_to_string(&config_path)
        .await
        .context("Failed to read plugin config")?;
    let val: serde_json::Value =
        serde_json::from_str(&content).context("Failed to parse plugin config")?;
    Ok(val)
}

/// Save a serde_json::Value as the plugin's config.json.
async fn save_plugin_config(name: &str, val: &serde_json::Value) -> Result<()> {
    let plugin_dir = LukanPaths::plugin_dir(name);
    tokio::fs::create_dir_all(&plugin_dir).await?;
    let config_path = LukanPaths::plugin_config(name);
    let content = serde_json::to_string_pretty(val)?;
    tokio::fs::write(&config_path, content)
        .await
        .context("Failed to write plugin config")?;
    Ok(())
}

/// Generic plugin config handler.
///
/// - No key → show all config with descriptions from schema
/// - Key without action → show current value
/// - Key + action → modify according to type
pub async fn handle_plugin_config(
    name: &str,
    key: Option<&str>,
    action: Option<&str>,
    value: Option<&str>,
) -> Result<()> {
    let manifest = PluginManager::load_manifest(name).await?;
    let mut config = load_plugin_config(name).await?;
    let obj = config.as_object_mut().unwrap();

    match key {
        None => {
            // Show all config
            println!("{BOLD}Configuration for {CYAN}{name}{RESET}{BOLD}:{RESET}\n");

            if manifest.config.is_empty() {
                println!("{DIM}No config schema defined in plugin.toml{RESET}");
                return Ok(());
            }

            let mut keys: Vec<&String> = manifest.config.keys().collect();
            keys.sort();

            for k in keys {
                let schema = &manifest.config[k];
                let camel = snake_to_camel(k);
                let current = obj.get(&camel);

                let type_label = match schema.field_type {
                    ConfigFieldType::String => "string",
                    ConfigFieldType::StringArray => "string[]",
                    ConfigFieldType::Number => "number",
                    ConfigFieldType::Bool => "bool",
                };

                let val_str = match current {
                    Some(v) => format_value(v),
                    None => format!("{DIM}(not set){RESET}"),
                };

                println!("  {CYAN}{k}{RESET} {DIM}({type_label}){RESET}");
                println!("    {val_str}");
                if !schema.description.is_empty() {
                    println!("    {DIM}{}{RESET}", schema.description);
                }
                println!();
            }
            Ok(())
        }
        Some(key) => {
            let schema = manifest.config.get(key).ok_or_else(|| {
                let available: Vec<&String> = manifest.config.keys().collect();
                anyhow::anyhow!(
                    "Unknown config key '{key}'.\nAvailable: {}",
                    available
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

            let camel = snake_to_camel(key);

            match (&schema.field_type, action) {
                // ── Show value (or interactive picker for tools/groups) ──
                (_, None) => {
                    if key == "allowed_groups" {
                        // Fetch groups from bridge via cli.js groups-json
                        let plugin_dir = LukanPaths::plugin_dir(name);
                        let cli_script = plugin_dir.join("cli.js");

                        if !cli_script.exists() {
                            println!(
                                "{RED}Plugin '{name}' has no cli.js — cannot list groups.{RESET}"
                            );
                            println!(
                                "{DIM}Make sure the WhatsApp bridge is running (lukan wa start).{RESET}"
                            );
                            return Ok(());
                        }

                        // Determine run command from manifest (default: "node")
                        let run_command = manifest
                            .run
                            .as_ref()
                            .map(|r| r.command.as_str())
                            .unwrap_or("node");

                        println!("{DIM}Fetching groups from bridge...{RESET}");

                        let output = std::process::Command::new(run_command)
                            .args([cli_script.to_string_lossy().as_ref(), "groups-json"])
                            .current_dir(&plugin_dir)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output()
                            .context("Failed to execute groups-json command")?;

                        // Take only the first line (bridge may print duplicates)
                        let raw = String::from_utf8_lossy(&output.stdout);
                        let json_str = raw.lines().next().unwrap_or("[]").trim().to_string();

                        #[derive(serde::Deserialize)]
                        struct GroupInfo {
                            id: String,
                            subject: String,
                            participants: Option<u64>,
                        }

                        let groups: Vec<GroupInfo> =
                            serde_json::from_str(&json_str).unwrap_or_default();

                        if groups.is_empty() {
                            println!("{YELLOW}No groups found.{RESET}");
                            println!(
                                "{DIM}Make sure the WhatsApp bridge is running (lukan wa start).{RESET}"
                            );
                            return Ok(());
                        }

                        // Build display items
                        let items: Vec<String> = groups
                            .iter()
                            .map(|g| {
                                let members = g.participants.unwrap_or(0);
                                format!("{:<40} {DIM}({members} members){RESET}", g.subject)
                            })
                            .collect();

                        // Current allowed groups → pre-check
                        let active: Vec<String> = obj
                            .get(&camel)
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let defaults: Vec<bool> = groups
                            .iter()
                            .map(|g| active.iter().any(|a| a == &g.id))
                            .collect();

                        let selected = MultiSelect::with_theme(&picker_theme())
                            .with_prompt("Toggle groups (space to select, enter to confirm)")
                            .items(&items)
                            .defaults(&defaults)
                            .interact()?;

                        let new_groups: Vec<serde_json::Value> = selected
                            .iter()
                            .map(|&i| serde_json::Value::String(groups[i].id.clone()))
                            .collect();

                        obj.insert(camel, serde_json::Value::Array(new_groups));
                        save_plugin_config(name, &config).await?;

                        println!(
                            "{GREEN}✓{RESET} Allowed groups updated ({} selected).",
                            selected.len()
                        );
                        return Ok(());
                    }

                    if key == "tools" {
                        // Discover ALL tools in the system (core + all plugins)
                        let all_tools = lukan_tools::all_tool_names();

                        // Build items grouped by TOOL_GROUPS
                        let mut items: Vec<String> = Vec::new();
                        let mut tool_names: Vec<String> = Vec::new();
                        let mut seen = std::collections::HashSet::new();

                        for (group, group_tools) in TOOL_GROUPS {
                            if !group_tools
                                .iter()
                                .any(|t| all_tools.iter().any(|a| a == *t))
                            {
                                continue;
                            }
                            for tool in *group_tools {
                                if all_tools.iter().any(|a| a == *tool) {
                                    items.push(format!("{tool:<20} {DIM}({group}){RESET}"));
                                    tool_names.push(tool.to_string());
                                    seen.insert(tool.to_string());
                                }
                            }
                        }
                        // Tools not in any TOOL_GROUP (e.g. from new plugins)
                        for tool in &all_tools {
                            if !seen.contains(tool.as_str()) {
                                items.push(format!("{tool:<20} {DIM}(plugin){RESET}"));
                                tool_names.push(tool.clone());
                            }
                        }

                        // Current active tools → pre-check defaults
                        let active: Vec<String> = obj
                            .get(&camel)
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_else(|| {
                                manifest
                                    .security
                                    .default_tools
                                    .iter()
                                    .filter(|t| all_tools.iter().any(|a| a == *t))
                                    .cloned()
                                    .collect()
                            });

                        let defaults: Vec<bool> = tool_names
                            .iter()
                            .map(|t| active.iter().any(|a| a == t))
                            .collect();

                        let selected = MultiSelect::with_theme(&picker_theme())
                            .with_prompt("Toggle tools (space to select, enter to confirm)")
                            .items(&items)
                            .defaults(&defaults)
                            .interact()?;

                        let new_tools: Vec<serde_json::Value> = selected
                            .iter()
                            .map(|&i| serde_json::Value::String(tool_names[i].clone()))
                            .collect();

                        obj.insert(camel, serde_json::Value::Array(new_tools));
                        save_plugin_config(name, &config).await?;

                        println!("{GREEN}✓{RESET} Tools updated.");
                        return Ok(());
                    }

                    let current = obj.get(&camel);
                    match current {
                        Some(v) => println!("{key}: {}", format_value(v)),
                        None => println!("{key}: {DIM}(not set){RESET}"),
                    }
                    Ok(())
                }

                // ── String ──
                (ConfigFieldType::String, Some("set")) => {
                    let val = value.ok_or_else(|| anyhow::anyhow!("Usage: {key} set <value>"))?;
                    validate_value(schema, val)?;
                    obj.insert(camel, serde_json::Value::String(val.to_string()));
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} set to \"{val}\"");
                    Ok(())
                }
                (ConfigFieldType::String, Some("unset")) => {
                    obj.remove(&camel);
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} unset");
                    Ok(())
                }

                // ── StringArray ──
                (ConfigFieldType::StringArray, Some("add")) => {
                    let val = value.ok_or_else(|| anyhow::anyhow!("Usage: {key} add <value>"))?;
                    validate_value(schema, val)?;
                    let arr = obj
                        .entry(&camel)
                        .or_insert_with(|| serde_json::json!([]))
                        .as_array_mut()
                        .ok_or_else(|| anyhow::anyhow!("Config key '{key}' is not an array"))?;
                    let val_json = serde_json::Value::String(val.to_string());
                    if arr.contains(&val_json) {
                        println!("{YELLOW}{val} already in {key}.{RESET}");
                    } else {
                        arr.push(val_json);
                        save_plugin_config(name, &config).await?;
                        println!("{GREEN}✓{RESET} Added {val} to {key}.");
                    }
                    Ok(())
                }
                (ConfigFieldType::StringArray, Some("remove")) => {
                    let val =
                        value.ok_or_else(|| anyhow::anyhow!("Usage: {key} remove <value>"))?;
                    let arr = obj.get_mut(&camel).and_then(|v| v.as_array_mut());
                    match arr {
                        Some(arr) => {
                            let val_json = serde_json::Value::String(val.to_string());
                            if let Some(idx) = arr.iter().position(|v| v == &val_json) {
                                arr.remove(idx);
                                save_plugin_config(name, &config).await?;
                                println!("{GREEN}✓{RESET} Removed {val} from {key}.");
                            } else {
                                println!("{YELLOW}{val} not in {key}.{RESET}");
                            }
                        }
                        None => {
                            println!("{YELLOW}{key} is empty.{RESET}");
                        }
                    }
                    Ok(())
                }
                (ConfigFieldType::StringArray, Some("list")) => {
                    if key == "tools" {
                        let all_tools = lukan_tools::all_tool_names();
                        let active_tools: Vec<String> = obj
                            .get(&camel)
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_else(|| {
                                manifest
                                    .security
                                    .default_tools
                                    .iter()
                                    .filter(|t| all_tools.iter().any(|a| a == *t))
                                    .cloned()
                                    .collect()
                            });

                        println!("{BOLD}{key}:{RESET}\n");
                        let mut seen = std::collections::HashSet::new();
                        for (group_name, group_tools) in TOOL_GROUPS {
                            if !group_tools
                                .iter()
                                .any(|t| all_tools.iter().any(|a| a == *t))
                            {
                                continue;
                            }
                            let tool_strs: Vec<String> = group_tools
                                .iter()
                                .filter(|t| all_tools.iter().any(|a| a == **t))
                                .map(|t| {
                                    seen.insert(t.to_string());
                                    if active_tools.iter().any(|a| a == *t) {
                                        format!("{GREEN}●{RESET} {t}")
                                    } else {
                                        format!("{DIM}○ {t}{RESET}")
                                    }
                                })
                                .collect();
                            println!("  {BOLD}{group_name}:{RESET}");
                            for ts in &tool_strs {
                                println!("    {ts}");
                            }
                            println!();
                        }
                        // Ungrouped (plugin-provided tools not in TOOL_GROUPS)
                        let ungrouped: Vec<String> = all_tools
                            .iter()
                            .filter(|t| !seen.contains(t.as_str()))
                            .map(|t| {
                                if active_tools.iter().any(|a| a == t) {
                                    format!("{GREEN}●{RESET} {t}")
                                } else {
                                    format!("{DIM}○ {t}{RESET}")
                                }
                            })
                            .collect();
                        if !ungrouped.is_empty() {
                            println!("  {BOLD}Plugin:{RESET}");
                            for ts in &ungrouped {
                                println!("    {ts}");
                            }
                            println!();
                        }
                        return Ok(());
                    }

                    let arr = obj.get(&camel).and_then(|v| v.as_array());
                    match arr {
                        Some(arr) if !arr.is_empty() => {
                            println!("{BOLD}{key}:{RESET}");
                            for (i, v) in arr.iter().enumerate() {
                                let s = v.as_str().unwrap_or("?");
                                println!("  {}) {CYAN}{s}{RESET}", i + 1);
                            }
                        }
                        _ => {
                            println!("{YELLOW}{key} is empty.{RESET}");
                        }
                    }
                    Ok(())
                }
                (ConfigFieldType::StringArray, Some("clear")) => {
                    obj.remove(&camel);
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} cleared.");
                    Ok(())
                }

                // ── Number ──
                (ConfigFieldType::Number, Some("set")) => {
                    let val = value.ok_or_else(|| anyhow::anyhow!("Usage: {key} set <number>"))?;
                    let n: f64 = val
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid number: {val}"))?;
                    obj.insert(
                        camel,
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(n)
                                .unwrap_or_else(|| serde_json::Number::from(n as i64)),
                        ),
                    );
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} set to {val}");
                    Ok(())
                }
                (ConfigFieldType::Number, Some("unset")) => {
                    obj.remove(&camel);
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} unset");
                    Ok(())
                }

                // ── Bool ──
                (ConfigFieldType::Bool, Some("on")) => {
                    obj.insert(camel, serde_json::Value::Bool(true));
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} enabled");
                    Ok(())
                }
                (ConfigFieldType::Bool, Some("off")) => {
                    obj.insert(camel, serde_json::Value::Bool(false));
                    save_plugin_config(name, &config).await?;
                    println!("{GREEN}✓{RESET} {key} disabled");
                    Ok(())
                }

                // ── Unknown action ──
                (field_type, Some(act)) => {
                    let valid = match field_type {
                        ConfigFieldType::String => "set, unset",
                        ConfigFieldType::StringArray => "add, remove, list, clear",
                        ConfigFieldType::Number => "set, unset",
                        ConfigFieldType::Bool => "on, off",
                    };
                    println!("{RED}Unknown action '{act}' for {key}.{RESET}");
                    println!("{DIM}Valid actions: {valid}{RESET}");
                    Ok(())
                }
            }
        }
    }
}

fn validate_value(schema: &lukan_core::models::plugin::ConfigFieldSchema, val: &str) -> Result<()> {
    if !schema.valid_values.is_empty() && !schema.valid_values.iter().any(|v| v == val) {
        anyhow::bail!(
            "Invalid value '{val}'.\nAllowed: {}",
            schema.valid_values.join(", ")
        );
    }
    Ok(())
}

pub fn format_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => format!("\"{s}\""),
        serde_json::Value::Bool(b) => {
            if *b {
                format!("{GREEN}on{RESET}")
            } else {
                format!("{RED}off{RESET}")
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                format!("{DIM}[]{RESET}")
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| v.as_str().unwrap_or("?").to_string())
                    .collect();
                items.join(", ")
            }
        }
        serde_json::Value::Null => format!("{DIM}null{RESET}"),
        _ => v.to_string(),
    }
}
