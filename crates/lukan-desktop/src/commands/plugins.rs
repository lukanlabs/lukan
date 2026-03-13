use lukan_core::config::LukanPaths;
use lukan_plugins::PluginManager;
use reqwest::multipart;
use serde::Serialize;
use std::sync::atomic::{AtomicI32, Ordering};

/// PID of the web UI process (0 = not running).
static WEB_UI_PID: AtomicI32 = AtomicI32::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityBarDto {
    pub icon: String,
    pub label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDeclarationDto {
    pub id: String,
    pub view_type: String,
    pub label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoDto {
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: String,
    pub running: bool,
    pub alias: Option<String>,
    pub activity_bar: Option<ActivityBarDto>,
    pub views: Vec<ViewDeclarationDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePluginDto {
    pub name: String,
    pub description: String,
    pub version: String,
    pub plugin_type: String,
    pub source: String,
    pub available: bool,
    pub installed: bool,
}

/// Check if a plugin process is alive via its PID file.
fn is_plugin_running(name: &str) -> bool {
    let pid_path = LukanPaths::plugin_pid(name);
    if let Ok(content) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        return unsafe { libc::kill(pid, 0) } == 0;
    }
    false
}

/// Find the lukan CLI binary.
///
/// Search order:
/// 1. Next to our own binary (dev builds, curl installs)
/// 2. Tauri bundle resources (`.deb`, `.app` installs)
/// 3. In PATH
fn find_lukan_bin() -> Result<std::path::PathBuf, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // 1. Next to our own binary
        let candidate = dir.join("lukan");
        if candidate.exists() {
            return Ok(candidate);
        }

        // 2. Tauri bundle resource paths
        //    macOS: Contents/MacOS/../Resources/lukan
        //    Linux .deb: /usr/bin/../lib/Lukan Desktop/lukan
        for relative in &["../Resources/lukan", "../lib/Lukan Desktop/lukan"] {
            let candidate = dir.join(relative);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // 3. In PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("lukan");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err("lukan binary not found. Install the CLI with: curl -sSf https://get.lukan.ai | sh".into())
}

#[tauri::command]
pub async fn list_plugins() -> Result<Vec<PluginInfoDto>, String> {
    let manager = PluginManager::new();
    let plugins = manager.list().await.map_err(|e| e.to_string())?;

    Ok(plugins
        .into_iter()
        .map(|p| PluginInfoDto {
            running: is_plugin_running(&p.name),
            name: p.name,
            version: p.version,
            description: p.description,
            plugin_type: p.plugin_type,
            alias: p.alias,
            activity_bar: p.activity_bar.map(|ab| ActivityBarDto {
                icon: ab.icon,
                label: ab.label,
            }),
            views: p
                .views
                .into_iter()
                .map(|v| ViewDeclarationDto {
                    id: v.id,
                    view_type: v.view_type,
                    label: v.label,
                })
                .collect(),
        })
        .collect())
}

#[tauri::command]
pub fn get_plugin_view_data(
    plugin_name: String,
    view_id: String,
) -> Result<serde_json::Value, String> {
    let path = LukanPaths::plugin_view_file(&plugin_name, &view_id);
    if !path.exists() {
        return Ok(serde_json::Value::Null);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_plugin(path: String) -> Result<String, String> {
    PluginManager::install(&path, None, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_remote_plugin(name: String) -> Result<String, String> {
    lukan_plugins::registry::install_remote(&name, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_plugin(name: String) -> Result<(), String> {
    // Stop first if running
    if is_plugin_running(&name) {
        let pid_path = LukanPaths::plugin_pid(&name);
        if let Ok(content) = std::fs::read_to_string(&pid_path)
            && let Ok(pid) = content.trim().parse::<i32>()
        {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let _ = tokio::fs::remove_file(&pid_path).await;
        }
    }
    let mut manager = PluginManager::new();
    manager.remove(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_plugin(name: String) -> Result<(), String> {
    if is_plugin_running(&name) {
        return Err(format!("Plugin '{name}' is already running"));
    }

    let lukan_bin = find_lukan_bin()?;

    // Let `lukan plugin start <name>` handle daemonization itself.
    // daemon_spawn() will self-respawn with LUKAN_DAEMON=1, write PID file,
    // redirect logs, and exit. We just wait for it to finish.
    let output = tokio::process::Command::new(&lukan_bin)
        .args(["plugin", "start", &name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run {}: {e}", lukan_bin.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = format!(
            "{}{}",
            stdout.trim(),
            if stderr.is_empty() {
                String::new()
            } else {
                format!("\n{}", stderr.trim())
            }
        );
        return Err(format!("Plugin '{name}' failed to start:\n{msg}"));
    }

    // Verify PID file was created (daemon_spawn writes it before exiting)
    let pid_path = LukanPaths::plugin_pid(&name);
    if !pid_path.exists() {
        // Small grace period in case of filesystem delay
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if !pid_path.exists() {
            return Err(format!(
                "Plugin '{name}' started but no PID file was created"
            ));
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_plugin(name: String) -> Result<(), String> {
    let pid_path = LukanPaths::plugin_pid(&name);
    if let Ok(content) = tokio::fs::read_to_string(&pid_path).await
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        // Wait for process to die (up to 3s)
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
        }
        let _ = tokio::fs::remove_file(&pid_path).await;
        return Ok(());
    }
    Err(format!("Plugin '{name}' is not running (no PID file)"))
}

#[tauri::command]
pub async fn restart_plugin(name: String) -> Result<(), String> {
    // Stop if running
    if is_plugin_running(&name) {
        let pid_path = LukanPaths::plugin_pid(&name);
        if let Ok(content) = tokio::fs::read_to_string(&pid_path).await
            && let Ok(pid) = content.trim().parse::<i32>()
        {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if unsafe { libc::kill(pid, 0) } != 0 {
                    break;
                }
            }
            let _ = tokio::fs::remove_file(&pid_path).await;
        }
    }

    // Start fresh
    start_plugin(name).await
}

#[tauri::command]
pub async fn get_plugin_config(name: String) -> Result<serde_json::Value, String> {
    let path = LukanPaths::plugin_config(&name);
    let mut config: serde_json::Value = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Merge manifest-declared config fields so the UI always shows them
    let manifest_path = LukanPaths::plugin_manifest(&name);
    if let Ok(manifest_content) = tokio::fs::read_to_string(&manifest_path).await
        && let Ok(manifest) =
            toml::from_str::<lukan_core::models::plugin::PluginManifest>(&manifest_content)
        && let Some(obj) = config.as_object_mut()
    {
        for (key, schema) in &manifest.config {
            obj.entry(key as &str).or_insert_with(|| {
                schema
                    .default
                    .clone()
                    .unwrap_or(serde_json::Value::String(String::new()))
            });
        }
    }

    Ok(config)
}

#[tauri::command]
pub async fn set_plugin_config_field(
    name: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let path = LukanPaths::plugin_config(&name);

    let mut config: serde_json::Value = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        obj.insert(key, value);
    }

    let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    tokio::fs::write(&path, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_plugin_logs(name: String, lines: u32) -> Result<String, String> {
    let log_path = LukanPaths::plugin_log(&name);
    if !log_path.exists() {
        return Ok(String::new());
    }

    let content = tokio::fs::read_to_string(&log_path)
        .await
        .map_err(|e| e.to_string())?;

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines as usize);
    Ok(all_lines[start..].join("\n"))
}

#[tauri::command]
pub async fn list_remote_plugins() -> Result<Vec<RemotePluginDto>, String> {
    let remotes = lukan_plugins::registry::list_remote()
        .await
        .map_err(|e| e.to_string())?;

    Ok(remotes
        .into_iter()
        .map(|r| RemotePluginDto {
            name: r.name,
            description: r.description,
            version: r.version,
            plugin_type: r.plugin_type,
            source: r.source,
            available: r.available,
            installed: r.installed,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDto {
    pub name: String,
    pub description: String,
}

/// Return the custom commands declared in the plugin manifest.
#[tauri::command]
pub async fn get_plugin_commands(name: String) -> Result<Vec<PluginCommandDto>, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    let mut cmds: Vec<PluginCommandDto> = manifest
        .commands
        .into_iter()
        .map(|(k, v)| PluginCommandDto {
            name: k,
            description: v.description,
        })
        .collect();
    cmds.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(cmds)
}

/// Execute a custom plugin command (e.g. `node cli.js auth` or `lukan-whisper download`).
#[tauri::command]
pub async fn run_plugin_command(name: String, command: String) -> Result<String, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    let cmd_def = manifest
        .commands
        .get(&command)
        .ok_or_else(|| format!("Plugin '{name}' has no command '{command}'"))?;

    let plugin_dir = LukanPaths::plugin_dir(&name);

    // Determine how to run the command based on plugin type:
    // - JS plugins: node cli.js <handler>
    // - Binary plugins (lukan-*): ./binary <handler>
    let run_cmd = manifest
        .run
        .as_ref()
        .map(|r| r.command.as_str())
        .unwrap_or("node");

    let (program, args): (String, Vec<String>) = if run_cmd.starts_with("lukan-") {
        // Binary plugin — resolve to plugin dir
        let bin = plugin_dir.join(run_cmd);
        if !bin.exists() {
            return Err(format!(
                "Plugin '{name}' binary not found: {}",
                bin.display()
            ));
        }
        (
            bin.to_string_lossy().to_string(),
            vec![cmd_def.handler.clone()],
        )
    } else {
        // JS plugin — use cli.js
        let cli_js = plugin_dir.join("cli.js");
        if !cli_js.exists() {
            return Err(format!("Plugin '{name}' has no cli.js"));
        }
        (
            run_cmd.to_string(),
            vec![
                cli_js.to_string_lossy().to_string(),
                cmd_def.handler.clone(),
            ],
        )
    };

    let output = tokio::process::Command::new(&program)
        .args(&args)
        .current_dir(&plugin_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let result = tokio::time::timeout(std::time::Duration::from_secs(300), output)
        .await
        .map_err(|_| "Command timed out (5m)".to_string())?
        .map_err(|e| format!("Failed to run command: {e}"))?;

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();

    if !result.status.success() {
        let msg = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!("Command failed: {}", msg.trim()));
    }

    // Combine stdout and stderr — some plugins (e.g. whisper) write to stderr
    let combined = if !stdout.trim().is_empty() && !stderr.trim().is_empty() {
        format!("{}\n{}", stdout.trim(), stderr.trim())
    } else if !stderr.trim().is_empty() {
        stderr
    } else {
        stdout
    };

    Ok(combined)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolsInfo {
    pub default_tools: Vec<String>,
    pub all_core_tools: Vec<String>,
}

#[tauri::command]
pub async fn get_plugin_manifest_tools(name: String) -> Result<PluginToolsInfo, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    let all_core: Vec<String> = lukan_tools::all_tool_info()
        .into_iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.name)
        .collect();

    Ok(PluginToolsInfo {
        default_tools: manifest.security.default_tools,
        all_core_tools: all_core,
    })
}

/// Read the QR code string for a plugin that uses QR-based auth.
/// Returns None if the plugin has no QR auth or no QR is available.
#[tauri::command]
pub async fn get_plugin_auth_qr(name: String) -> Result<Option<String>, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    let qr_file = match &manifest.auth {
        Some(lukan_core::models::plugin::AuthDeclaration::Qr { qr_file, .. }) => qr_file.clone(),
        _ => return Ok(None),
    };

    let qr_path = LukanPaths::plugin_data_dir(&name).join(&qr_file);
    if !qr_path.exists() {
        return Ok(None);
    }
    let content = tokio::fs::read_to_string(&qr_path)
        .await
        .map_err(|e| e.to_string())?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// Check whether a plugin is authenticated, according to its auth declaration.
/// - QR: check if status_file exists and has content
/// - Token: check if config field exists and is non-empty
/// - Command / None: always returns true
#[tauri::command]
pub async fn check_plugin_auth(name: String) -> Result<bool, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    match &manifest.auth {
        Some(lukan_core::models::plugin::AuthDeclaration::Qr { status_file, .. }) => {
            let creds_path = LukanPaths::plugin_data_dir(&name).join(status_file);
            if !creds_path.exists() {
                return Ok(false);
            }
            match tokio::fs::metadata(&creds_path).await {
                Ok(meta) => Ok(meta.len() > 2),
                Err(_) => Ok(false),
            }
        }
        Some(lukan_core::models::plugin::AuthDeclaration::Token { check_field }) => {
            let config_path = LukanPaths::plugin_config(&name);
            if !config_path.exists() {
                return Ok(false);
            }
            let content = tokio::fs::read_to_string(&config_path)
                .await
                .map_err(|e| e.to_string())?;
            let json: serde_json::Value =
                serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
            Ok(json
                .get(check_field)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty()))
        }
        Some(lukan_core::models::plugin::AuthDeclaration::Command) | None => Ok(true),
    }
}

/// DTO for config field schema — camelCase for the frontend JSON API.
/// ConfigFieldSchema itself uses snake_case for TOML deserialization.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFieldSchemaDto {
    #[serde(rename = "type")]
    field_type: String,
    description: String,
    valid_values: Vec<String>,
    label: Option<String>,
    format: Option<String>,
    group: Option<String>,
    depends_on: Option<DependsOnDto>,
    options_command: Option<String>,
    hidden: bool,
    default: Option<serde_json::Value>,
    order: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DependsOnDto {
    field: String,
    values: Vec<String>,
}

impl From<&lukan_core::models::plugin::ConfigFieldSchema> for ConfigFieldSchemaDto {
    fn from(s: &lukan_core::models::plugin::ConfigFieldSchema) -> Self {
        Self {
            field_type: serde_json::to_value(&s.field_type)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            description: s.description.clone(),
            valid_values: s.valid_values.clone(),
            label: s.label.clone(),
            format: s.format.clone(),
            group: s.group.clone(),
            depends_on: s.depends_on.as_ref().map(|d| DependsOnDto {
                field: d.field.clone(),
                values: d.values.clone(),
            }),
            options_command: s.options_command.clone(),
            hidden: s.hidden,
            default: s.default.clone(),
            order: s.order,
        }
    }
}

/// Return the manifest info (config schema + auth declaration) for a plugin.
/// This gives the frontend everything it needs to render a generic config form.
#[tauri::command]
pub async fn get_plugin_manifest_info(name: String) -> Result<serde_json::Value, String> {
    let manifest = PluginManager::load_manifest(&name)
        .await
        .map_err(|e| e.to_string())?;

    let config_schema: std::collections::HashMap<String, ConfigFieldSchemaDto> = manifest
        .config
        .iter()
        .map(|(k, v)| (k.clone(), ConfigFieldSchemaDto::from(v)))
        .collect();

    Ok(serde_json::json!({
        "config": config_schema,
        "auth": manifest.auth,
    }))
}

// ── Web UI management ────────────────────────────────────────────────────

fn is_web_ui_alive() -> bool {
    let pid = WEB_UI_PID.load(Ordering::Relaxed);
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebUiStatus {
    pub running: bool,
    pub port: u16,
}

#[tauri::command]
pub async fn get_web_ui_status() -> Result<WebUiStatus, String> {
    Ok(WebUiStatus {
        running: is_web_ui_alive(),
        port: std::env::var("LUKAN_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000),
    })
}

#[tauri::command]
pub async fn start_web_ui(port: u16, cwd: Option<String>) -> Result<(), String> {
    if is_web_ui_alive() {
        return Err("Web UI is already running".into());
    }

    let lukan_bin = find_lukan_bin()?;

    let working_dir = cwd.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let child = tokio::process::Command::new(&lukan_bin)
        .args(["chat", "--ui", "web"])
        .env("LUKAN_PORT", port.to_string())
        .current_dir(&working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start web UI: {e}"))?;

    let pid = child.id().ok_or("Failed to get web UI PID")? as i32;
    WEB_UI_PID.store(pid, Ordering::Relaxed);

    // Give it a moment to bind the port
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    if !is_web_ui_alive() {
        WEB_UI_PID.store(0, Ordering::Relaxed);
        return Err("Web UI process exited immediately — check config/credentials".into());
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_web_ui() -> Result<(), String> {
    let pid = WEB_UI_PID.load(Ordering::Relaxed);
    if pid <= 0 || !is_web_ui_alive() {
        WEB_UI_PID.store(0, Ordering::Relaxed);
        return Err("Web UI is not running".into());
    }

    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait up to 3s for process to die
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if unsafe { libc::kill(pid, 0) } != 0 {
            break;
        }
    }

    WEB_UI_PID.store(0, Ordering::Relaxed);
    Ok(())
}

// ── Audio transcription (contribution-based discovery) ───────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionStatusDto {
    pub installed: bool,
    pub running: bool,
    pub port: u16,
}

/// Resolved transcription endpoint from a plugin's contributions.
struct TranscriptionProvider {
    plugin_name: String,
    port: u16,
    endpoint: String,
}

/// Scan all installed plugins for `contributions.transcription`,
/// return the first one that is running.
async fn find_transcription_provider() -> Option<TranscriptionProvider> {
    let plugins_dir = LukanPaths::plugins_dir();
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let manifest_path = entry.path().join("plugin.toml");
        let Ok(content) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = toml::from_str::<lukan_core::models::plugin::PluginManifest>(&content)
        else {
            continue;
        };
        let Some(tc) = manifest.contributions.transcription else {
            continue;
        };
        if !is_plugin_running(&name) {
            continue;
        }
        let port = read_plugin_port(&name, &tc.port_field, tc.default_port).await;
        return Some(TranscriptionProvider {
            plugin_name: name,
            port,
            endpoint: tc.endpoint,
        });
    }
    None
}

/// Read a port from a plugin's config.json, falling back to a default.
async fn read_plugin_port(name: &str, port_field: &str, default_port: u16) -> u16 {
    let config_path = LukanPaths::plugin_config(name);
    if let Ok(content) = tokio::fs::read_to_string(&config_path).await
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(port) = json.get(port_field).and_then(|v| v.as_u64())
    {
        return port as u16;
    }
    default_port
}

/// Check whether any transcription plugin is installed and running.
#[tauri::command]
pub async fn check_transcription_status() -> Result<TranscriptionStatusDto, String> {
    match find_transcription_provider().await {
        Some(provider) => Ok(TranscriptionStatusDto {
            installed: true,
            running: true,
            port: provider.port,
        }),
        None => {
            // Check if any plugin with transcription contribution is at least installed
            let plugins_dir = LukanPaths::plugins_dir();
            let installed = std::fs::read_dir(&plugins_dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| {
                    let manifest_path = entry.path().join("plugin.toml");
                    std::fs::read_to_string(&manifest_path)
                        .ok()
                        .and_then(|c| {
                            toml::from_str::<lukan_core::models::plugin::PluginManifest>(&c).ok()
                        })
                        .is_some_and(|m| m.contributions.transcription.is_some())
                });
            Ok(TranscriptionStatusDto {
                installed,
                running: false,
                port: 0,
            })
        }
    }
}

/// Transcribe audio by forwarding it to whichever transcription plugin is running.
/// Accepts raw audio bytes (webm/opus from the browser MediaRecorder).
#[tauri::command]
pub async fn transcribe_audio(audio: Vec<u8>) -> Result<String, String> {
    let provider = find_transcription_provider()
        .await
        .ok_or("No transcription plugin is running")?;

    let url = format!("http://127.0.0.1:{}{}", provider.port, provider.endpoint);

    // Build multipart form with the audio file
    let part = multipart::Part::bytes(audio)
        .file_name("audio.webm")
        .mime_str("audio/webm")
        .map_err(|e| format!("Failed to build multipart: {e}"))?;

    let form = multipart::Form::new().part("file", part);

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| {
            format!(
                "Transcription request failed ({}): {e}",
                provider.plugin_name
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Transcription server error ({}) {status}: {body}",
            provider.plugin_name
        ));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    json.get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Transcription response missing 'text' field".into())
}
