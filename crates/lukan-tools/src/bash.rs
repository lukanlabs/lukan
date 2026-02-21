use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use tokio::process::Command;
use tracing::debug;

use crate::{Tool, ToolContext};

const MAX_OUTPUT_BYTES: usize = 30_000;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command. Use for system commands, git operations, and terminal tasks."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)",
                    "default": 120000
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: command"))?;

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_MS);

        debug!(command, timeout_ms, "Executing bash command");

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            Command::new("bash")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.cwd)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let mut combined = String::new();

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                // Truncate if needed
                if combined.len() > MAX_OUTPUT_BYTES {
                    combined.truncate(MAX_OUTPUT_BYTES);
                    combined.push_str("\n... (output truncated)");
                }

                let exit_code = output.status.code().unwrap_or(-1);
                let content = if combined.is_empty() {
                    format!("(exit code: {exit_code})")
                } else {
                    format!("{combined}\n(exit code: {exit_code})")
                };

                if output.status.success() {
                    Ok(ToolResult::success(content))
                } else {
                    Ok(ToolResult::error(content))
                }
            }
            Ok(Err(e)) => Ok(ToolResult::error(format!("Failed to execute command: {e}"))),
            Err(_) => Ok(ToolResult::error(format!(
                "Command timed out after {timeout_ms}ms"
            ))),
        }
    }
}
