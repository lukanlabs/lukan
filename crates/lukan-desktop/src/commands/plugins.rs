use lukan_core::config::LukanPaths;
use lukan_plugins::PluginManager;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI32, Ordering};

/// PID of the web UI process (0 = not running).
static WEB_UI_PID: AtomicI32 = AtomicI32::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoDto {
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: String,
    pub running: bool,
    pub alias: Option<String>,
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

/// Find the lukan CLI binary (next to lukan-desktop, or in PATH).
fn find_lukan_bin() -> Result<std::path::PathBuf, String> {
    // 1. Next to our own binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("lukan");
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // 2. In PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("lukan");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(
        "lukan binary not found. Ensure it's in the same directory as lukan-desktop or in PATH."
            .into(),
    )
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
        })
        .collect())
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
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
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

    let result = tokio::time::timeout(std::time::Duration::from_secs(120), output)
        .await
        .map_err(|_| "Command timed out (120s)".to_string())?
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

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppGroup {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub participants: Option<u64>,
}

/// Fetch WhatsApp groups by running `node cli.js groups-json` in the plugin directory.
#[tauri::command]
pub async fn fetch_whatsapp_groups(name: String) -> Result<Vec<WhatsAppGroup>, String> {
    let plugin_dir = LukanPaths::plugin_dir(&name);
    let cli_js = plugin_dir.join("cli.js");
    if !cli_js.exists() {
        return Ok(vec![]);
    }

    let output = tokio::process::Command::new("node")
        .args([cli_js.to_string_lossy().as_ref(), "groups-json"])
        .current_dir(&plugin_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let result = tokio::time::timeout(std::time::Duration::from_secs(10), output)
        .await
        .map_err(|_| "Timed out fetching groups (10s)".to_string())?
        .map_err(|e| format!("Failed to run cli.js: {e}"))?;

    if !result.status.success() {
        return Ok(vec![]);
    }

    // Take only the first line — bridge may print duplicates
    let raw = String::from_utf8_lossy(&result.stdout);
    let json_str = raw.lines().next().unwrap_or("[]").trim();
    let groups: Vec<WhatsAppGroup> = serde_json::from_str(json_str).unwrap_or_default();
    Ok(groups)
}

/// Read the current WhatsApp QR code string (if pending authentication).
#[tauri::command]
pub async fn get_whatsapp_qr() -> Result<Option<String>, String> {
    let qr_path = LukanPaths::whatsapp_auth_dir().join("current-qr.txt");
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

/// Check whether WhatsApp has valid auth credentials (creds.json exists and has content).
#[tauri::command]
pub async fn check_whatsapp_auth() -> Result<bool, String> {
    let creds_path = LukanPaths::whatsapp_auth_dir().join("creds.json");
    if !creds_path.exists() {
        return Ok(false);
    }
    match tokio::fs::metadata(&creds_path).await {
        Ok(meta) => Ok(meta.len() > 2), // non-empty JSON (not just "{}")
        Err(_) => Ok(false),
    }
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
pub async fn start_web_ui(port: u16) -> Result<(), String> {
    if is_web_ui_alive() {
        return Err("Web UI is already running".into());
    }

    let lukan_bin = find_lukan_bin()?;

    let child = tokio::process::Command::new(&lukan_bin)
        .args(["chat", "--ui", "web"])
        .env("LUKAN_PORT", port.to_string())
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
