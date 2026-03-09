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

/// GET /api/cwd
pub async fn get_cwd() -> impl IntoResponse {
    match std::env::current_dir() {
        Ok(p) => Json(serde_json::json!(p.to_string_lossy())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get cwd: {e}"),
        )
            .into_response(),
    }
}
