use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use lukan_core::config::LukanPaths;
use lukan_plugins::PluginManager;
use serde::{Deserialize, Serialize};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDto {
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolsInfoDto {
    pub default_tools: Vec<String>,
    pub all_core_tools: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppGroup {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub participants: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStatusDto {
    pub installed: bool,
    pub running: bool,
    pub port: u16,
}

fn is_plugin_running(name: &str) -> bool {
    let pid_path = LukanPaths::plugin_pid(name);
    if let Ok(content) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        return unsafe { libc::kill(pid, 0) } == 0;
    }
    false
}

fn find_lukan_bin() -> Result<std::path::PathBuf, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("lukan");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("lukan");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err("lukan binary not found".into())
}

/// GET /api/plugins
pub async fn list_plugins() -> impl IntoResponse {
    let manager = PluginManager::new();
    match manager.list().await {
        Ok(plugins) => {
            let list: Vec<PluginInfoDto> = plugins
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
                .collect();
            Json(list).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/plugins/install
pub async fn install_plugin(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let path = body["path"].as_str().unwrap_or_default();
    match PluginManager::install(path, None, None).await {
        Ok(msg) => Json(serde_json::json!({ "message": msg })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/plugins/install-remote
pub async fn install_remote_plugin(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let name = body["name"].as_str().unwrap_or_default();
    match lukan_plugins::registry::install_remote(name, None).await {
        Ok(msg) => Json(serde_json::json!({ "message": msg })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /api/plugins/:name
pub async fn remove_plugin(Path(name): Path<String>) -> impl IntoResponse {
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
    match manager.remove(&name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/plugins/:name/start
pub async fn start_plugin(Path(name): Path<String>) -> impl IntoResponse {
    if is_plugin_running(&name) {
        return (
            StatusCode::CONFLICT,
            format!("Plugin '{name}' is already running"),
        )
            .into_response();
    }

    let lukan_bin = match find_lukan_bin() {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let output = match tokio::process::Command::new(&lukan_bin)
        .args(["plugin", "start", &name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to run lukan: {e}"),
            )
                .into_response();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Plugin '{name}' failed to start:\n{stdout}{stderr}"),
        )
            .into_response();
    }

    let pid_path = LukanPaths::plugin_pid(&name);
    if !pid_path.exists() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    StatusCode::OK.into_response()
}

/// POST /api/plugins/:name/stop
pub async fn stop_plugin(Path(name): Path<String>) -> impl IntoResponse {
    let pid_path = LukanPaths::plugin_pid(&name);
    if let Ok(content) = tokio::fs::read_to_string(&pid_path).await
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
        }
        let _ = tokio::fs::remove_file(&pid_path).await;
        return StatusCode::OK.into_response();
    }
    (
        StatusCode::NOT_FOUND,
        format!("Plugin '{name}' is not running"),
    )
        .into_response()
}

/// POST /api/plugins/:name/restart
pub async fn restart_plugin(Path(name): Path<String>) -> impl IntoResponse {
    // Stop if running
    if is_plugin_running(&name) {
        let pid_path = LukanPaths::plugin_pid(&name);
        if let Ok(content) = tokio::fs::read_to_string(&pid_path).await
            && let Ok(pid) = content.trim().parse::<i32>()
        {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if unsafe { libc::kill(pid, 0) } != 0 {
                    break;
                }
            }
            let _ = tokio::fs::remove_file(&pid_path).await;
        }
    }

    // Start
    let lukan_bin = match find_lukan_bin() {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let output = tokio::process::Command::new(&lukan_bin)
        .args(["plugin", "start", &name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => StatusCode::OK.into_response(),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Plugin '{name}' failed to start: {stderr}"),
            )
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/:name/config
pub async fn get_plugin_config(Path(name): Path<String>) -> impl IntoResponse {
    let path = LukanPaths::plugin_config(&name);
    if !path.exists() {
        return Json(serde_json::json!({})).into_response();
    }
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let val: serde_json::Value =
                serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
            Json(val).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/plugins/:name/config
pub async fn set_plugin_config_field(
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let key = body["key"].as_str().unwrap_or_default().to_string();
    let value = body["value"].clone();
    let path = LukanPaths::plugin_config(&name);

    let mut config: serde_json::Value = if path.exists() {
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        }
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        obj.insert(key, value);
    }

    let content = match serde_json::to_string_pretty(&config) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    match tokio::fs::write(&path, content).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/:name/logs?lines=N
pub async fn get_plugin_logs(
    Path(name): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let lines: usize = params
        .get("lines")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let log_path = LukanPaths::plugin_log(&name);
    if !log_path.exists() {
        return Json(serde_json::json!("")).into_response();
    }

    match tokio::fs::read_to_string(&log_path).await {
        Ok(content) => {
            let all_lines: Vec<&str> = content.lines().collect();
            let start = all_lines.len().saturating_sub(lines);
            let result = all_lines[start..].join("\n");
            Json(serde_json::json!(result)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/remote
pub async fn list_remote_plugins() -> impl IntoResponse {
    match lukan_plugins::registry::list_remote().await {
        Ok(remotes) => {
            let list: Vec<RemotePluginDto> = remotes
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
                .collect();
            Json(list).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/:name/commands
pub async fn get_plugin_commands(Path(name): Path<String>) -> impl IntoResponse {
    match PluginManager::load_manifest(&name).await {
        Ok(manifest) => {
            let mut cmds: Vec<PluginCommandDto> = manifest
                .commands
                .into_iter()
                .map(|(k, v)| PluginCommandDto {
                    name: k,
                    description: v.description,
                })
                .collect();
            cmds.sort_by(|a, b| a.name.cmp(&b.name));
            Json(cmds).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/plugins/:name/commands/:command
pub async fn run_plugin_command(
    Path((name, command)): Path<(String, String)>,
) -> impl IntoResponse {
    let manifest = match PluginManager::load_manifest(&name).await {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let cmd_def = match manifest.commands.get(&command) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                format!("Plugin '{name}' has no command '{command}'"),
            )
                .into_response();
        }
    };

    let plugin_dir = LukanPaths::plugin_dir(&name);
    let run_cmd = manifest
        .run
        .as_ref()
        .map(|r| r.command.as_str())
        .unwrap_or("node");

    let (program, args): (String, Vec<String>) = if run_cmd.starts_with("lukan-") {
        let bin = plugin_dir.join(run_cmd);
        if !bin.exists() {
            return (
                StatusCode::NOT_FOUND,
                format!("Plugin binary not found: {}", bin.display()),
            )
                .into_response();
        }
        (
            bin.to_string_lossy().to_string(),
            vec![cmd_def.handler.clone()],
        )
    } else {
        let cli_js = plugin_dir.join("cli.js");
        if !cli_js.exists() {
            return (
                StatusCode::NOT_FOUND,
                format!("Plugin '{name}' has no cli.js"),
            )
                .into_response();
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

    let result = match tokio::time::timeout(std::time::Duration::from_secs(120), output).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to run command: {e}"),
            )
                .into_response();
        }
        Err(_) => return (StatusCode::GATEWAY_TIMEOUT, "Command timed out (120s)").into_response(),
    };

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();

    if !result.status.success() {
        let msg = if stderr.is_empty() { &stdout } else { &stderr };
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Command failed: {}", msg.trim()),
        )
            .into_response();
    }

    let combined = if !stdout.trim().is_empty() && !stderr.trim().is_empty() {
        format!("{}\n{}", stdout.trim(), stderr.trim())
    } else if !stderr.trim().is_empty() {
        stderr
    } else {
        stdout
    };

    Json(serde_json::json!(combined)).into_response()
}

/// GET /api/plugins/:name/tools
pub async fn get_plugin_manifest_tools(Path(name): Path<String>) -> impl IntoResponse {
    match PluginManager::load_manifest(&name).await {
        Ok(manifest) => {
            let all_core: Vec<String> = lukan_tools::all_tool_info()
                .into_iter()
                .filter(|t| t.source.is_none())
                .map(|t| t.name)
                .collect();
            Json(PluginToolsInfoDto {
                default_tools: manifest.security.default_tools,
                all_core_tools: all_core,
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/:name/views/:viewId
pub async fn get_plugin_view_data(
    Path((plugin_name, view_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let path = LukanPaths::plugin_view_file(&plugin_name, &view_id);
    if !path.exists() {
        return Json(serde_json::Value::Null).into_response();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(val) => Json(val).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/whatsapp/qr
pub async fn get_whatsapp_qr() -> impl IntoResponse {
    let qr_path = LukanPaths::whatsapp_auth_dir().join("current-qr.txt");
    if !qr_path.exists() {
        return Json(serde_json::Value::Null).into_response();
    }
    match tokio::fs::read_to_string(&qr_path).await {
        Ok(content) => {
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                Json(serde_json::Value::Null).into_response()
            } else {
                Json(serde_json::json!(trimmed)).into_response()
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/plugins/whatsapp/auth
pub async fn check_whatsapp_auth() -> impl IntoResponse {
    let creds_path = LukanPaths::whatsapp_auth_dir().join("creds.json");
    if !creds_path.exists() {
        return Json(serde_json::json!(false)).into_response();
    }
    match tokio::fs::metadata(&creds_path).await {
        Ok(meta) => Json(serde_json::json!(meta.len() > 2)).into_response(),
        Err(_) => Json(serde_json::json!(false)).into_response(),
    }
}

/// GET /api/plugins/:name/whatsapp-groups
pub async fn fetch_whatsapp_groups(Path(name): Path<String>) -> impl IntoResponse {
    let plugin_dir = LukanPaths::plugin_dir(&name);
    let cli_js = plugin_dir.join("cli.js");
    if !cli_js.exists() {
        return Json(Vec::<WhatsAppGroup>::new()).into_response();
    }

    let output = tokio::process::Command::new("node")
        .args([cli_js.to_string_lossy().as_ref(), "groups-json"])
        .current_dir(&plugin_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let result = match tokio::time::timeout(std::time::Duration::from_secs(10), output).await {
        Ok(Ok(r)) if r.status.success() => r,
        _ => return Json(Vec::<WhatsAppGroup>::new()).into_response(),
    };

    let raw = String::from_utf8_lossy(&result.stdout);
    let json_str = raw.lines().next().unwrap_or("[]").trim();
    let groups: Vec<WhatsAppGroup> = serde_json::from_str(json_str).unwrap_or_default();
    Json(groups).into_response()
}

/// GET /api/whisper/status
pub async fn check_whisper_status() -> Json<WhisperStatusDto> {
    let plugin_dir = LukanPaths::plugin_dir("whisper");
    let installed = plugin_dir.join("plugin.toml").exists();
    if !installed {
        return Json(WhisperStatusDto {
            installed: false,
            running: false,
            port: 0,
        });
    }
    let running = is_plugin_running("whisper");
    let port = read_whisper_port().await;
    Json(WhisperStatusDto {
        installed,
        running,
        port,
    })
}

/// POST /api/whisper/transcribe
pub async fn transcribe_audio(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    if !is_plugin_running("whisper") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Whisper plugin is not running",
        )
            .into_response();
    }

    let audio: Vec<u8> = match body["audio"].as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u8))
            .collect(),
        None => {
            return (StatusCode::BAD_REQUEST, "Missing audio data").into_response();
        }
    };

    let port = read_whisper_port().await;
    let url = format!("http://127.0.0.1:{port}/v1/audio/transcriptions");

    let part = match reqwest::multipart::Part::bytes(audio)
        .file_name("audio.webm")
        .mime_str("audio/webm")
    {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let form = reqwest::multipart::Form::new().part("file", part);

    let client = reqwest::Client::new();
    let resp = match client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Whisper request failed: {e}"),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Whisper error {status}: {body}"),
        )
            .into_response();
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match json.get("text").and_then(|v| v.as_str()) {
        Some(text) => Json(serde_json::json!(text)).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Whisper response missing 'text' field",
        )
            .into_response(),
    }
}

async fn read_whisper_port() -> u16 {
    let config_path = LukanPaths::plugin_config("whisper");
    if let Ok(content) = tokio::fs::read_to_string(&config_path).await
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(port) = json.get("port").and_then(|v| v.as_u64())
    {
        return port as u16;
    }
    8787
}
