//! MCP (Model Context Protocol) client for stdio-based JSON-RPC servers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use lukan_core::config::types::McpServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

// ── JSON-RPC types ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: Option<i64>,
    message: String,
}

// ── MCP types ───────────────────────────────────────────────────────────

/// Tool definition discovered from an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Result from an MCP tool call.
#[derive(Debug)]
pub struct McpToolResult {
    pub content: String,
    pub is_error: bool,
}

// ── MCP Client ──────────────────────────────────────────────────────────

/// Client for a single MCP server communicating over stdio JSON-RPC.
pub struct McpClient {
    name: String,
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: Mutex<BufReader<tokio::process::ChildStdout>>,
    next_id: AtomicU64,
}

impl McpClient {
    /// Start an MCP server process and perform the initialize handshake.
    pub async fn start(name: &str, config: &McpServerConfig) -> Result<Self> {
        debug!(server = name, command = %config.command, "Starting MCP server");

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server '{name}': {}", config.command))?;

        let stdin = child
            .stdin
            .take()
            .context("MCP server stdin not available")?;
        let stdout = child
            .stdout
            .take()
            .context("MCP server stdout not available")?;

        let mut client = Self {
            name: name.to_string(),
            child,
            stdin,
            reader: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
        };

        // Perform initialize handshake
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "lukan",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let response = client
            .send_request("initialize", Some(init_params))
            .await
            .with_context(|| format!("MCP server '{name}' initialize handshake failed"))?;

        debug!(server = name, ?response, "MCP initialize response");

        // Send initialized notification (no response expected)
        client
            .send_notification("notifications/initialized", None)
            .await?;

        Ok(client)
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        let response = self.send_request("tools/list", None).await?;

        let tools_value = response
            .as_object()
            .and_then(|o| o.get("tools"))
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        let tools_array = tools_value.as_array().cloned().unwrap_or_default();

        let mut tools = Vec::new();
        for tool in tools_array {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = tool
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }));

            if !name.is_empty() {
                tools.push(McpToolDef {
                    name,
                    description,
                    input_schema,
                });
            }
        }

        debug!(server = %self.name, count = tools.len(), "Discovered MCP tools");
        Ok(tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<McpToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let response = self.send_request("tools/call", Some(params)).await?;

        // Parse content array from response
        let is_error = response
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content =
            if let Some(content_array) = response.get("content").and_then(|v| v.as_array()) {
                let mut text_parts = Vec::new();
                for item in content_array {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                text_parts.join("\n")
            } else {
                // Fallback: stringify the whole response
                serde_json::to_string_pretty(&response).unwrap_or_default()
            };

        Ok(McpToolResult { content, is_error })
    }

    /// Gracefully shut down the MCP server.
    pub async fn shutdown(&mut self) {
        // Close stdin to signal EOF
        drop(unsafe { std::ptr::read(&self.stdin) });

        // Wait briefly for process to exit
        match tokio::time::timeout(std::time::Duration::from_secs(3), self.child.wait()).await {
            Ok(Ok(status)) => {
                debug!(server = %self.name, ?status, "MCP server exited");
            }
            _ => {
                warn!(server = %self.name, "MCP server did not exit in time, killing");
                let _ = self.child.kill().await;
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────────

    async fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let mut line = serde_json::to_string(&request)?;
        line.push('\n');

        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        // Read response lines until we get one with a matching id
        let mut reader = self.reader.lock().await;
        let mut buf = String::new();
        loop {
            buf.clear();
            let bytes_read = reader.read_line(&mut buf).await?;
            if bytes_read == 0 {
                anyhow::bail!("MCP server '{}' closed stdout unexpectedly", self.name);
            }

            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as JSON-RPC response
            let response: JsonRpcResponse = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(_) => {
                    // Could be a notification — skip
                    debug!(server = %self.name, line = trimmed, "Skipping non-response line from MCP server");
                    continue;
                }
            };

            // Check if this is a response (has id) vs notification (no id)
            if response.id.is_none() {
                // Server notification — skip
                continue;
            }

            if let Some(error) = response.error {
                anyhow::bail!("MCP server '{}' error: {}", self.name, error.message);
            }

            return Ok(response.result.unwrap_or(Value::Null));
        }
    }

    async fn send_notification(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        // Notifications have no id
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(Value::Object(serde_json::Map::new())),
        });

        let mut line = serde_json::to_string(&notification)?;
        line.push('\n');

        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        Ok(())
    }
}

// ── MCP Manager ─────────────────────────────────────────────────────────

/// Manages multiple MCP server connections.
pub struct McpManager {
    clients: HashMap<String, std::sync::Arc<tokio::sync::Mutex<McpClient>>>,
    /// Tool definitions discovered from all servers, keyed by tool name.
    pub tool_defs: Vec<(String, McpToolDef)>, // (server_name, tool_def)
}

impl McpManager {
    /// Initialize all configured MCP servers and discover their tools.
    ///
    /// Returns a list of per-server errors alongside the successfully initialized manager.
    pub async fn init(
        configs: &HashMap<String, McpServerConfig>,
    ) -> (Self, Vec<(String, String)>) {
        let mut clients = HashMap::new();
        let mut tool_defs = Vec::new();
        let mut errors: Vec<(String, String)> = Vec::new();

        for (name, config) in configs {
            match McpClient::start(name, config).await {
                Ok(mut client) => {
                    match client.list_tools().await {
                        Ok(tools) => {
                            if tools.is_empty() {
                                warn!(server = %name, "MCP server returned 0 tools");
                                errors.push((
                                    name.clone(),
                                    "Server started but returned 0 tools".to_string(),
                                ));
                            }
                            for tool in &tools {
                                debug!(
                                    server = %name,
                                    tool = %tool.name,
                                    "Discovered MCP tool"
                                );
                            }
                            for tool in tools {
                                tool_defs.push((name.clone(), tool));
                            }
                        }
                        Err(e) => {
                            warn!(server = %name, "Failed to list MCP tools: {e}");
                            errors.push((name.clone(), format!("list_tools failed: {e}")));
                        }
                    }
                    clients.insert(
                        name.clone(),
                        std::sync::Arc::new(tokio::sync::Mutex::new(client)),
                    );
                }
                Err(e) => {
                    warn!(server = %name, "Failed to start MCP server: {e}");
                    errors.push((name.clone(), format!("Failed to start: {e}")));
                }
            }
        }

        (Self { clients, tool_defs }, errors)
    }

    /// Get a client by server name.
    pub fn get_client(
        &self,
        server_name: &str,
    ) -> Option<std::sync::Arc<tokio::sync::Mutex<McpClient>>> {
        self.clients.get(server_name).cloned()
    }

    /// Gracefully shut down all MCP servers.
    pub async fn shutdown(&self) {
        for (name, client) in &self.clients {
            debug!(server = %name, "Shutting down MCP server");
            let mut client = client.lock().await;
            client.shutdown().await;
        }
    }
}
