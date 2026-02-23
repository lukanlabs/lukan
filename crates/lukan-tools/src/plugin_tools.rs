//! Plugin-provided tools: proxy tools that delegate execution to plugin handlers.
//!
//! Each plugin can include a `tools.json` with tool definitions and a `tools.js`
//! handler script. This module creates proxy `Tool` implementations that spawn
//! the handler process and communicate via stdin/stdout.

use async_trait::async_trait;
use lukan_core::config::LukanPaths;
use lukan_core::models::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::{Tool, ToolContext, ToolRegistry};

/// A tool definition as declared in a plugin's `tools.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

/// The JSON output format expected from plugin tool handlers.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolOutput {
    output: String,
    #[serde(default)]
    is_error: bool,
}

/// A proxy tool that delegates execution to a plugin's handler script.
///
/// When executed, it spawns: `<handler_command> tools.js <tool_name>`
/// in the plugin directory, sends the input JSON via stdin, and reads
/// the result from stdout.
struct PluginProvidedTool {
    tool_name: String,
    tool_description: String,
    tool_input_schema: Value,
    plugin_name: String,
    handler_command: String,
}

#[async_trait]
impl Tool for PluginProvidedTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        self.tool_input_schema.clone()
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let plugin_dir = LukanPaths::plugin_dir(&self.plugin_name);
        let tools_script = plugin_dir.join("tools.js");

        if !tools_script.exists() {
            return Ok(ToolResult::error(format!(
                "Plugin '{}' tools.js not found at {}",
                self.plugin_name,
                tools_script.display()
            )));
        }

        let mut child = Command::new(&self.handler_command)
            .arg(tools_script.to_string_lossy().as_ref())
            .arg(&self.tool_name)
            .current_dir(&plugin_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Write input JSON to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let json_bytes = serde_json::to_vec(&input)?;
            stdin.write_all(&json_bytes).await?;
            // Drop stdin to signal EOF
        }

        // Wait for output
        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if !stderr.is_empty() {
                stderr.to_string()
            } else if !stdout.is_empty() {
                stdout.to_string()
            } else {
                format!(
                    "Plugin tool '{}' exited with code {}",
                    self.tool_name,
                    output.status.code().unwrap_or(-1)
                )
            };
            return Ok(ToolResult::error(msg));
        }

        // Parse stdout as JSON
        match serde_json::from_slice::<ToolOutput>(&output.stdout) {
            Ok(result) => {
                if result.is_error {
                    Ok(ToolResult::error(result.output))
                } else {
                    Ok(ToolResult::success(result.output))
                }
            }
            Err(e) => {
                // If parsing fails, return raw stdout as the output
                let raw = String::from_utf8_lossy(&output.stdout).to_string();
                if raw.is_empty() {
                    Ok(ToolResult::error(format!(
                        "Plugin tool '{}' returned no output (parse error: {e})",
                        self.tool_name
                    )))
                } else {
                    Ok(ToolResult::success(raw))
                }
            }
        }
    }
}

/// Scan installed plugins for `tools.json` and register proxy tools.
///
/// For each plugin that has a `tools.json` file, parses the tool definitions
/// and registers a `PluginProvidedTool` for each one. The handler command
/// is determined from the plugin's `plugin.toml` manifest (run.command) or
/// defaults to "node".
pub fn register_plugin_tools(registry: &mut ToolRegistry) {
    let plugins_dir = LukanPaths::plugins_dir();

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(entries) => entries,
        Err(_) => return, // No plugins directory
    };

    for entry in entries.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }

        let plugin_name = match plugin_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let tools_path = plugin_dir.join("tools.json");
        if !tools_path.exists() {
            continue;
        }

        // Determine handler command from manifest or default to "node"
        let handler_command = {
            let manifest_path = plugin_dir.join("plugin.toml");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Ok(manifest) =
                    toml::from_str::<lukan_core::models::plugin::PluginManifest>(&content)
                {
                    manifest
                        .run
                        .map(|r| r.command)
                        .unwrap_or_else(|| "node".to_string())
                } else {
                    "node".to_string()
                }
            } else {
                "node".to_string()
            }
        };

        // Parse tools.json
        let tools_content = match std::fs::read_to_string(&tools_path) {
            Ok(content) => content,
            Err(e) => {
                warn!(
                    plugin = %plugin_name,
                    "Failed to read tools.json: {e}"
                );
                continue;
            }
        };

        let tool_defs: Vec<ToolDef> = match serde_json::from_str(&tools_content) {
            Ok(defs) => defs,
            Err(e) => {
                warn!(
                    plugin = %plugin_name,
                    "Failed to parse tools.json: {e}"
                );
                continue;
            }
        };

        debug!(
            plugin = %plugin_name,
            count = tool_defs.len(),
            "Registering plugin-provided tools"
        );

        for def in tool_defs {
            registry.register(Box::new(PluginProvidedTool {
                tool_name: def.name,
                tool_description: def.description,
                tool_input_schema: def.input_schema,
                plugin_name: plugin_name.clone(),
                handler_command: handler_command.clone(),
            }));
        }
    }
}
