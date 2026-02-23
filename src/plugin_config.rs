use anyhow::{Context, Result};

use lukan_core::config::LukanPaths;
use lukan_core::models::plugin::ConfigFieldType;
use lukan_plugins::PluginManager;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";

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
                    available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?;

            let camel = snake_to_camel(key);

            match (&schema.field_type, action) {
                // ── Show value ──
                (_, None) => {
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
                    let arr = obj
                        .get_mut(&camel)
                        .and_then(|v| v.as_array_mut());
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
                    let n: f64 = val.parse().map_err(|_| {
                        anyhow::anyhow!("Invalid number: {val}")
                    })?;
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

fn validate_value(
    schema: &lukan_core::models::plugin::ConfigFieldSchema,
    val: &str,
) -> Result<()> {
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
