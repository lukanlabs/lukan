use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

#[derive(serde::Deserialize)]
pub struct PathQuery {
    path: Option<String>,
}

/// GET /api/files?path=...
pub async fn list_directory(Query(q): Query<PathQuery>) -> impl IntoResponse {
    let dir = match q.path {
        Some(p) => PathBuf::from(p),
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to get cwd: {e}"),
                )
                    .into_response();
            }
        },
    };

    let mut entries = Vec::new();
    let mut read_dir = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read directory: {e}"),
            )
                .into_response();
        }
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let metadata = entry.metadata().await.ok();
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = metadata.as_ref().and_then(|m| m.modified().ok()).map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        });

        entries.push(FileEntry {
            name,
            is_dir,
            size,
            modified,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Json(DirectoryListing {
        path: dir.to_string_lossy().to_string(),
        entries,
    })
    .into_response()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub name: String,
    pub content: String,
    pub encoding: String,
    pub size: u64,
    pub language: Option<String>,
    pub mime_type: Option<String>,
}

const MAX_TEXT_SIZE: u64 = 2 * 1024 * 1024; // 2MB
const MAX_BINARY_SIZE: u64 = 10 * 1024 * 1024; // 10MB

fn language_from_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" => Some("python"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "cs" => Some("csharp"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kotlin"),
        "sh" | "bash" | "zsh" => Some("bash"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "xml" => Some("xml"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "scss" | "sass" => Some("scss"),
        "sql" => Some("sql"),
        "md" | "markdown" => Some("markdown"),
        "dockerfile" => Some("dockerfile"),
        "lua" => Some("lua"),
        "r" => Some("r"),
        "zig" => Some("zig"),
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        _ => None,
    }
}

fn mime_from_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "ico" => Some("image/x-icon"),
        "bmp" => Some("image/bmp"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

fn is_binary(buf: &[u8]) -> bool {
    buf.contains(&0)
}

/// GET /api/files/read?path=...
pub async fn read_file(Query(q): Query<PathQuery>) -> impl IntoResponse {
    let path_str = match q.path {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing 'path' parameter".to_string(),
            )
                .into_response();
        }
    };

    let file_path = PathBuf::from(&path_str);
    let metadata = match tokio::fs::metadata(&file_path).await {
        Ok(m) => m,
        Err(e) => {
            return (StatusCode::NOT_FOUND, format!("File not found: {e}")).into_response();
        }
    };

    if metadata.is_dir() {
        return (StatusCode::BAD_REQUEST, "Path is a directory".to_string()).into_response();
    }

    let size = metadata.len();
    let name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let language = language_from_ext(&ext);
    let mime_type = mime_from_ext(&ext);
    let is_image_or_pdf = mime_type.is_some();

    if is_image_or_pdf {
        if size > MAX_BINARY_SIZE {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("File too large: {size} bytes (max 10MB)"),
            )
                .into_response();
        }
        let bytes = match tokio::fs::read(&file_path).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to read file: {e}"),
                )
                    .into_response();
            }
        };
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Json(FileContent {
            path: path_str,
            name,
            content: encoded,
            encoding: "base64".to_string(),
            size,
            language: language.map(String::from),
            mime_type: mime_type.map(String::from),
        })
        .into_response();
    }

    if size > MAX_TEXT_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("File too large: {size} bytes (max 2MB)"),
        )
            .into_response();
    }

    let bytes = match tokio::fs::read(&file_path).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read file: {e}"),
            )
                .into_response();
        }
    };

    let check_len = bytes.len().min(8192);
    if is_binary(&bytes[..check_len]) {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Json(FileContent {
            path: path_str,
            name,
            content: encoded,
            encoding: "base64".to_string(),
            size,
            language: None,
            mime_type: Some("application/octet-stream".to_string()),
        })
        .into_response();
    }

    let text = String::from_utf8_lossy(&bytes).to_string();
    Json(FileContent {
        path: path_str,
        name,
        content: text,
        encoding: "utf8".to_string(),
        size,
        language: language.map(String::from),
        mime_type: None,
    })
    .into_response()
}

#[derive(serde::Deserialize)]
pub struct WriteFileBody {
    path: String,
    content: String,
}

/// PUT /api/files/write
pub async fn write_file(Json(body): Json<WriteFileBody>) -> impl IntoResponse {
    let file_path = PathBuf::from(&body.path);

    // Refuse to write to directories
    if file_path.is_dir() {
        return (StatusCode::BAD_REQUEST, "Path is a directory".to_string()).into_response();
    }

    // Ensure parent directory exists
    if let Some(parent) = file_path.parent()
        && !parent.exists()
    {
        return (
            StatusCode::BAD_REQUEST,
            "Parent directory does not exist".to_string(),
        )
            .into_response();
    }

    match tokio::fs::write(&file_path, body.content.as_bytes()).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write file: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- language_from_ext ---

    #[test]
    fn test_language_from_ext_rust() {
        assert_eq!(language_from_ext("rs"), Some("rust"));
    }

    #[test]
    fn test_language_from_ext_typescript() {
        assert_eq!(language_from_ext("ts"), Some("typescript"));
        assert_eq!(language_from_ext("tsx"), Some("typescript"));
    }

    #[test]
    fn test_language_from_ext_javascript() {
        assert_eq!(language_from_ext("js"), Some("javascript"));
        assert_eq!(language_from_ext("jsx"), Some("javascript"));
    }

    #[test]
    fn test_language_from_ext_python() {
        assert_eq!(language_from_ext("py"), Some("python"));
    }

    #[test]
    fn test_language_from_ext_go() {
        assert_eq!(language_from_ext("go"), Some("go"));
    }

    #[test]
    fn test_language_from_ext_java() {
        assert_eq!(language_from_ext("java"), Some("java"));
    }

    #[test]
    fn test_language_from_ext_c_family() {
        assert_eq!(language_from_ext("c"), Some("c"));
        assert_eq!(language_from_ext("h"), Some("c"));
        assert_eq!(language_from_ext("cpp"), Some("cpp"));
        assert_eq!(language_from_ext("cc"), Some("cpp"));
        assert_eq!(language_from_ext("cxx"), Some("cpp"));
        assert_eq!(language_from_ext("hpp"), Some("cpp"));
    }

    #[test]
    fn test_language_from_ext_csharp() {
        assert_eq!(language_from_ext("cs"), Some("csharp"));
    }

    #[test]
    fn test_language_from_ext_scripting() {
        assert_eq!(language_from_ext("rb"), Some("ruby"));
        assert_eq!(language_from_ext("php"), Some("php"));
        assert_eq!(language_from_ext("lua"), Some("lua"));
        assert_eq!(language_from_ext("r"), Some("r"));
    }

    #[test]
    fn test_language_from_ext_shell() {
        assert_eq!(language_from_ext("sh"), Some("bash"));
        assert_eq!(language_from_ext("bash"), Some("bash"));
        assert_eq!(language_from_ext("zsh"), Some("bash"));
    }

    #[test]
    fn test_language_from_ext_config() {
        assert_eq!(language_from_ext("json"), Some("json"));
        assert_eq!(language_from_ext("toml"), Some("toml"));
        assert_eq!(language_from_ext("yaml"), Some("yaml"));
        assert_eq!(language_from_ext("yml"), Some("yaml"));
        assert_eq!(language_from_ext("xml"), Some("xml"));
    }

    #[test]
    fn test_language_from_ext_web() {
        assert_eq!(language_from_ext("html"), Some("html"));
        assert_eq!(language_from_ext("htm"), Some("html"));
        assert_eq!(language_from_ext("css"), Some("css"));
        assert_eq!(language_from_ext("scss"), Some("scss"));
        assert_eq!(language_from_ext("sass"), Some("scss"));
        assert_eq!(language_from_ext("vue"), Some("vue"));
        assert_eq!(language_from_ext("svelte"), Some("svelte"));
    }

    #[test]
    fn test_language_from_ext_other() {
        assert_eq!(language_from_ext("swift"), Some("swift"));
        assert_eq!(language_from_ext("kt"), Some("kotlin"));
        assert_eq!(language_from_ext("kts"), Some("kotlin"));
        assert_eq!(language_from_ext("sql"), Some("sql"));
        assert_eq!(language_from_ext("md"), Some("markdown"));
        assert_eq!(language_from_ext("markdown"), Some("markdown"));
        assert_eq!(language_from_ext("dockerfile"), Some("dockerfile"));
        assert_eq!(language_from_ext("zig"), Some("zig"));
    }

    #[test]
    fn test_language_from_ext_unknown() {
        assert_eq!(language_from_ext("xyz"), None);
        assert_eq!(language_from_ext(""), None);
        assert_eq!(language_from_ext("doc"), None);
        assert_eq!(language_from_ext("png"), None);
    }

    // --- mime_from_ext ---

    #[test]
    fn test_mime_from_ext_images() {
        assert_eq!(mime_from_ext("png"), Some("image/png"));
        assert_eq!(mime_from_ext("jpg"), Some("image/jpeg"));
        assert_eq!(mime_from_ext("jpeg"), Some("image/jpeg"));
        assert_eq!(mime_from_ext("gif"), Some("image/gif"));
        assert_eq!(mime_from_ext("svg"), Some("image/svg+xml"));
        assert_eq!(mime_from_ext("webp"), Some("image/webp"));
        assert_eq!(mime_from_ext("ico"), Some("image/x-icon"));
        assert_eq!(mime_from_ext("bmp"), Some("image/bmp"));
    }

    #[test]
    fn test_mime_from_ext_pdf() {
        assert_eq!(mime_from_ext("pdf"), Some("application/pdf"));
    }

    #[test]
    fn test_mime_from_ext_unknown() {
        assert_eq!(mime_from_ext("rs"), None);
        assert_eq!(mime_from_ext("txt"), None);
        assert_eq!(mime_from_ext(""), None);
        assert_eq!(mime_from_ext("html"), None);
    }

    // --- is_binary ---

    #[test]
    fn test_is_binary_with_null_byte() {
        let buf = b"hello\x00world";
        assert!(is_binary(buf), "Buffer with null byte is binary");
    }

    #[test]
    fn test_is_binary_without_null_byte() {
        let buf = b"hello world\nline two\n";
        assert!(!is_binary(buf), "Normal text is not binary");
    }

    #[test]
    fn test_is_binary_empty() {
        let buf: &[u8] = b"";
        assert!(!is_binary(buf), "Empty buffer is not binary");
    }

    #[test]
    fn test_is_binary_only_null() {
        let buf = b"\x00";
        assert!(is_binary(buf));
    }

    #[test]
    fn test_is_binary_utf8_text() {
        let buf = "hello, \u{00e9}l\u{00e8}ve".as_bytes();
        assert!(
            !is_binary(buf),
            "UTF-8 text without null bytes is not binary"
        );
    }

    // --- DTO serialization ---

    #[test]
    fn test_file_entry_serialization() {
        let entry = FileEntry {
            name: "main.rs".into(),
            is_dir: false,
            size: 1024,
            modified: Some("2024-01-01T00:00:00Z".into()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""name":"main.rs""#), "name: {json}");
        assert!(json.contains(r#""isDir":false"#), "isDir camelCase: {json}");
        assert!(!json.contains("is_dir"), "no snake_case: {json}");
        assert!(json.contains(r#""size":1024"#), "size: {json}");
    }

    #[test]
    fn test_directory_listing_serialization() {
        let listing = DirectoryListing {
            path: "/home/user".into(),
            entries: vec![FileEntry {
                name: "src".into(),
                is_dir: true,
                size: 0,
                modified: None,
            }],
        };
        let json = serde_json::to_string(&listing).unwrap();
        assert!(json.contains(r#""path":"/home/user""#), "path: {json}");
        assert!(json.contains(r#""entries""#), "entries: {json}");
        assert!(json.contains(r#""isDir":true"#), "isDir: {json}");
    }

    #[test]
    fn test_file_content_serialization() {
        let content = FileContent {
            path: "/tmp/test.rs".into(),
            name: "test.rs".into(),
            content: "fn main() {}".into(),
            encoding: "utf8".into(),
            size: 13,
            language: Some("rust".into()),
            mime_type: None,
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""mimeType""#), "mimeType camelCase: {json}");
        assert!(json.contains(r#""language":"rust""#), "language: {json}");
        assert!(json.contains(r#""encoding":"utf8""#), "encoding: {json}");
    }
}

/// GET /api/cwd
/// GET /api/git — execute read-only git commands
/// Query params: cmd (log|branch|status|diff|remote), dir (optional), args (optional)
pub async fn git_command(
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let cmd = q.get("cmd").map(|s| s.as_str()).unwrap_or("");
    let dir = q.get("dir").map(|s| s.as_str()).unwrap_or(".");
    let extra_args = q.get("args").map(|s| s.as_str()).unwrap_or("");

    // Whitelist: only read-only git commands
    let args: Vec<&str> = match cmd {
        "log" => vec![
            "log",
            "--all",
            "--oneline",
            "--graph",
            "--parents",
            "--decorate=short",
            "-100",
        ],
        "log-full" => vec!["log", "--all", "--format=%H|%P|%an|%aI|%s|%D", "-200"],
        "branch" => vec![
            "branch",
            "-a",
            "--format=%(refname:short) %(objectname:short) %(upstream:short)",
        ],
        "status" => vec!["status", "--porcelain"],
        "diff-staged" => vec!["diff", "--cached", "--name-status"],
        "diff-unstaged" => vec!["diff", "--name-status"],
        "untracked" => vec!["ls-files", "--others", "--exclude-standard"],
        "head" => vec!["rev-parse", "--abbrev-ref", "HEAD"],
        "default-branch" => {
            // Detect the default branch of origin
            // Try symbolic-ref first (fast, local), fallback to remote show
            let result = std::process::Command::new("git")
                .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
                .current_dir(dir)
                .output();
            if let Ok(output) = &result {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !stdout.is_empty() {
                    let branch = stdout.replace("refs/remotes/origin/", "");
                    return Json(serde_json::json!({ "ok": true, "stdout": branch, "stderr": "" }))
                        .into_response();
                }
            }
            // Fallback: git remote show origin (slower, needs network)
            let result = std::process::Command::new("git")
                .args(["remote", "show", "origin"])
                .current_dir(dir)
                .output();
            return match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let branch = stdout
                        .lines()
                        .find(|l| l.contains("HEAD branch:"))
                        .and_then(|l| l.split(':').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    Json(serde_json::json!({ "ok": !branch.is_empty(), "stdout": branch, "stderr": "" })).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")).into_response()
                }
            };
        }
        "rev-list" => {
            // Count ahead/behind — args = "--count --left-right origin/branch...branch"
            let args: Vec<&str> = extra_args.split_whitespace().collect();
            let result = std::process::Command::new("git")
                .arg("rev-list")
                .args(&args)
                .current_dir(dir)
                .output();
            return match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Json(serde_json::json!({ "ok": output.status.success(), "stdout": stdout, "stderr": "" })).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")).into_response()
                }
            };
        }
        "diff-file" => {
            // Diff for a specific file in a commit — args = "sha file_path"
            let mut parts = extra_args.splitn(2, ' ');
            let sha = parts.next().unwrap_or("HEAD");
            let file = parts.next().unwrap_or("");
            let result = std::process::Command::new("git")
                .args(["diff", "-U99999", &format!("{sha}^"), sha, "--", file])
                .current_dir(dir)
                .output();
            return match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Json(serde_json::json!({ "ok": output.status.success(), "stdout": stdout, "stderr": "" })).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")).into_response()
                }
            };
        }
        "diff-working" => {
            // Diff for a working-tree file against HEAD — args = file_path
            let file = extra_args.trim();
            // Try HEAD diff first (tracked files)
            let result = std::process::Command::new("git")
                .args(["diff", "-U99999", "HEAD", "--", file])
                .current_dir(dir)
                .output();
            return match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    if stdout.trim().is_empty() {
                        // Might be untracked or only staged — try unstaged diff
                        let unstaged = std::process::Command::new("git")
                            .args(["diff", "-U99999", "--", file])
                            .current_dir(dir)
                            .output();
                        if let Ok(u) = unstaged {
                            let us = String::from_utf8_lossy(&u.stdout).to_string();
                            if !us.trim().is_empty() {
                                return Json(
                                    serde_json::json!({ "ok": true, "stdout": us, "stderr": "" }),
                                )
                                .into_response();
                            }
                        }
                        // Try showing untracked file as all-additions
                        let file_path = std::path::Path::new(dir).join(file);
                        if let Ok(content) = std::fs::read_to_string(&file_path) {
                            let lines: Vec<String> =
                                content.lines().map(|l| format!("+{l}")).collect();
                            let header = format!(
                                "--- /dev/null\n+++ b/{file}\n@@ -0,0 +1,{} @@\n{}",
                                lines.len(),
                                lines.join("\n")
                            );
                            return Json(
                                serde_json::json!({ "ok": true, "stdout": header, "stderr": "" }),
                            )
                            .into_response();
                        }
                    }
                    Json(serde_json::json!({ "ok": output.status.success(), "stdout": stdout, "stderr": "" })).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")).into_response()
                }
            };
        }
        "remote" => vec!["remote", "-v"],
        "diff-stat" => vec!["diff", "--stat"],
        "show" => {
            // Show files for a specific commit — args = sha
            let sha = extra_args.split_whitespace().next().unwrap_or("HEAD");
            let result = std::process::Command::new("git")
                .args(["show", "--numstat", "--format=%B", sha])
                .current_dir(dir)
                .output();
            return match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Json(serde_json::json!({ "ok": output.status.success(), "stdout": stdout, "stderr": "" })).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")).into_response()
                }
            };
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Invalid git command. Allowed: log, log-full, branch, status, remote, diff-stat",
            )
                .into_response();
        }
    };

    let result = std::process::Command::new("git")
        .args(&args)
        .current_dir(dir)
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(serde_json::json!({
                "ok": output.status.success(),
                "stdout": stdout,
                "stderr": stderr,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to run git: {e}"),
        )
            .into_response(),
    }
}

/// POST /api/active-tab — notify which tab is active (updates cwd for plugins)
pub async fn set_active_tab(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(tab_id) = body.get("tabId").and_then(|v| v.as_str()) {
        // Extract cwd and session_id from in-memory session
        let (mem_cwd, session_id) = {
            let sessions = state.sessions.lock().await;
            if let Some(session) = sessions.get(tab_id) {
                (session.cwd.clone(), session.last_session_id.clone())
            } else {
                (None, None)
            }
        };

        if let Some(ref cwd) = mem_cwd {
            *state.active_cwd.lock().unwrap() = Some(cwd.clone());
        } else if let Some(ref sid) = session_id {
            // Fallback: read cwd from persisted session on disk
            if let Ok(Some(saved)) = lukan_agent::SessionManager::load(sid).await
                && let Some(ref cwd) = saved.cwd
            {
                *state.active_cwd.lock().unwrap() = Some(cwd.clone());
            }
        } else {
            tracing::debug!(tab_id, "No cwd or session_id found for tab");
        }
    } else {
        tracing::warn!("set_active_tab called without tabId");
    }
    StatusCode::OK
}

pub async fn get_terminal_cwd(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.terminal_manager.get_session_cwd(&session_id).await {
        Ok(cwd) => Json(serde_json::json!({ "cwd": cwd })).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("{e}")).into_response(),
    }
}

pub async fn get_cwd(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
) -> impl IntoResponse {
    // Return active session cwd if available, otherwise process cwd
    let active = state.active_cwd.lock().unwrap().clone();
    if let Some(cwd) = active {
        return Json(serde_json::json!(cwd)).into_response();
    }
    match std::env::current_dir() {
        Ok(p) => Json(serde_json::json!(p.to_string_lossy())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get cwd: {e}"),
        )
            .into_response(),
    }
}
