use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use lukan_core::config::LukanPaths;
use lukan_plugins::PluginManager;
use serde::Serialize;

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionStatusDto {
    pub installed: bool,
    pub running: bool,
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_info_dto_serialization() {
        let dto = PluginInfoDto {
            name: "discord".into(),
            version: "1.0.0".into(),
            description: "Discord bridge".into(),
            plugin_type: "bridge".into(),
            running: true,
            alias: Some("dc".into()),
            activity_bar: Some(ActivityBarDto {
                icon: "chat".into(),
                label: "Discord".into(),
            }),
            views: vec![ViewDeclarationDto {
                id: "discord-chat".into(),
                view_type: "webview".into(),
                label: "Chat".into(),
            }],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains(r#""pluginType""#),
            "pluginType camelCase: {json}"
        );
        assert!(
            json.contains(r#""activityBar""#),
            "activityBar camelCase: {json}"
        );
        assert!(json.contains(r#""viewType""#), "viewType camelCase: {json}");
        assert!(
            !json.contains("plugin_type"),
            "no snake_case plugin_type: {json}"
        );
        assert!(
            !json.contains("activity_bar"),
            "no snake_case activity_bar: {json}"
        );
        assert!(
            !json.contains("view_type"),
            "no snake_case view_type: {json}"
        );
    }

    #[test]
    fn test_remote_plugin_dto_serialization() {
        let dto = RemotePluginDto {
            name: "whisper".into(),
            description: "Speech to text".into(),
            version: "0.1.0".into(),
            plugin_type: "service".into(),
            source: "https://example.com/whisper.tar.gz".into(),
            available: true,
            installed: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains(r#""pluginType""#),
            "pluginType camelCase: {json}"
        );
        assert!(!json.contains("plugin_type"), "no snake_case: {json}");
    }

    #[test]
    fn test_plugin_command_dto_serialization() {
        let dto = PluginCommandDto {
            name: "sync".into(),
            description: "Sync data".into(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""name":"sync""#), "name: {json}");
        assert!(
            json.contains(r#""description":"Sync data""#),
            "description: {json}"
        );
    }

    #[test]
    fn test_plugin_tools_info_dto_serialization() {
        let dto = PluginToolsInfoDto {
            default_tools: vec!["Bash".into(), "ReadFile".into()],
            all_core_tools: vec!["Bash".into(), "ReadFile".into(), "WriteFile".into()],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains(r#""defaultTools""#),
            "defaultTools camelCase: {json}"
        );
        assert!(
            json.contains(r#""allCoreTools""#),
            "allCoreTools camelCase: {json}"
        );
        assert!(!json.contains("default_tools"), "no snake_case: {json}");
        assert!(!json.contains("all_core_tools"), "no snake_case: {json}");
    }

    #[test]
    fn test_transcription_status_dto_serialization() {
        let dto = TranscriptionStatusDto {
            installed: true,
            running: false,
            port: 8080,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""installed":true"#), "installed: {json}");
        assert!(json.contains(r#""running":false"#), "running: {json}");
        assert!(json.contains(r#""port":8080"#), "port: {json}");
    }

    #[test]
    fn test_activity_bar_dto_serialization() {
        let dto = ActivityBarDto {
            icon: "settings".into(),
            label: "Settings".into(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""icon":"settings""#), "icon: {json}");
        assert!(json.contains(r#""label":"Settings""#), "label: {json}");
    }
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
    let mut config: serde_json::Value = if path.exists() {
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
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

    Json(config).into_response()
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

    let result = match tokio::time::timeout(std::time::Duration::from_secs(300), output).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to run command: {e}"),
            )
                .into_response();
        }
        Err(_) => return (StatusCode::GATEWAY_TIMEOUT, "Command timed out (5m)").into_response(),
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

/// DTO for config field schema — camelCase for the frontend JSON API.
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

/// GET /api/plugins/:name/manifest-info
pub async fn get_plugin_manifest_info(Path(name): Path<String>) -> impl IntoResponse {
    match PluginManager::load_manifest(&name).await {
        Ok(manifest) => {
            let config_schema: std::collections::HashMap<String, ConfigFieldSchemaDto> = manifest
                .config
                .iter()
                .map(|(k, v)| (k.clone(), ConfigFieldSchemaDto::from(v)))
                .collect();
            Json(serde_json::json!({
                "config": config_schema,
                "auth": manifest.auth,
            }))
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
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

/// GET /api/plugins/:name/auth/qr
pub async fn get_plugin_auth_qr(Path(name): Path<String>) -> impl IntoResponse {
    let manifest = match PluginManager::load_manifest(&name).await {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let qr_file = match &manifest.auth {
        Some(lukan_core::models::plugin::AuthDeclaration::Qr { qr_file, .. }) => qr_file.clone(),
        _ => return Json(serde_json::Value::Null).into_response(),
    };

    let qr_path = LukanPaths::plugin_data_dir(&name).join(&qr_file);
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

/// GET /api/plugins/:name/auth/status
pub async fn check_plugin_auth(Path(name): Path<String>) -> impl IntoResponse {
    let manifest = match PluginManager::load_manifest(&name).await {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match &manifest.auth {
        Some(lukan_core::models::plugin::AuthDeclaration::Qr { status_file, .. }) => {
            let creds_path = LukanPaths::plugin_data_dir(&name).join(status_file);
            if !creds_path.exists() {
                return Json(serde_json::json!(false)).into_response();
            }
            match tokio::fs::metadata(&creds_path).await {
                Ok(meta) => Json(serde_json::json!(meta.len() > 2)).into_response(),
                Err(_) => Json(serde_json::json!(false)).into_response(),
            }
        }
        Some(lukan_core::models::plugin::AuthDeclaration::Token { check_field }) => {
            let config_path = LukanPaths::plugin_config(&name);
            if !config_path.exists() {
                return Json(serde_json::json!(false)).into_response();
            }
            match tokio::fs::read_to_string(&config_path).await {
                Ok(content) => {
                    let json: serde_json::Value =
                        serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
                    let authed = json
                        .get(check_field)
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty());
                    Json(serde_json::json!(authed)).into_response()
                }
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            }
        }
        Some(lukan_core::models::plugin::AuthDeclaration::Command) | None => {
            Json(serde_json::json!(true)).into_response()
        }
    }
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

/// GET /api/transcription/status — checks all plugins with transcription contributions
pub async fn check_transcription_status() -> Json<TranscriptionStatusDto> {
    match find_transcription_provider().await {
        Some(provider) => Json(TranscriptionStatusDto {
            installed: true,
            running: true,
            port: provider.port,
        }),
        None => {
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
            Json(TranscriptionStatusDto {
                installed,
                running: false,
                port: 0,
            })
        }
    }
}

/// POST /api/transcription/transcribe — discovers transcription provider dynamically
pub async fn transcribe_audio(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let provider = match find_transcription_provider().await {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "No transcription plugin is running",
            )
                .into_response();
        }
    };

    let audio: Vec<u8> = match body["audio"].as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u8))
            .collect(),
        None => {
            return (StatusCode::BAD_REQUEST, "Missing audio data").into_response();
        }
    };

    let url = format!("http://127.0.0.1:{}{}", provider.port, provider.endpoint);

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
                format!(
                    "Transcription request failed ({}): {e}",
                    provider.plugin_name
                ),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Transcription error ({}) {status}: {body}",
                provider.plugin_name
            ),
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
            "Transcription response missing 'text' field",
        )
            .into_response(),
    }
}
