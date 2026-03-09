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

/// List files and directories at the given path.
#[tauri::command]
pub async fn list_directory(path: Option<String>) -> Result<DirectoryListing, String> {
    let dir = match path {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("Failed to get cwd: {e}"))?,
    };

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read directory: {e}"))?;

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files starting with .
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

    // Sort: directories first, then alphabetically
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(DirectoryListing {
        path: dir.to_string_lossy().to_string(),
        entries,
    })
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

const MAX_TEXT_SIZE: u64 = 2 * 1024 * 1024;
const MAX_BINARY_SIZE: u64 = 10 * 1024 * 1024;

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

/// Read file contents for inline preview.
#[tauri::command]
pub async fn read_file(path: String) -> Result<FileContent, String> {
    let file_path = PathBuf::from(&path);
    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("File not found: {e}"))?;

    if metadata.is_dir() {
        return Err("Path is a directory".to_string());
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
            return Err(format!("File too large: {size} bytes (max 10MB)"));
        }
        let bytes = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Failed to read file: {e}"))?;
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Ok(FileContent {
            path,
            name,
            content: encoded,
            encoding: "base64".to_string(),
            size,
            language: language.map(String::from),
            mime_type: mime_type.map(String::from),
        });
    }

    if size > MAX_TEXT_SIZE {
        return Err(format!("File too large: {size} bytes (max 2MB)"));
    }

    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Failed to read file: {e}"))?;

    let check_len = bytes.len().min(8192);
    if is_binary(&bytes[..check_len]) {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Ok(FileContent {
            path,
            name,
            content: encoded,
            encoding: "base64".to_string(),
            size,
            language: None,
            mime_type: Some("application/octet-stream".to_string()),
        });
    }

    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(FileContent {
        path,
        name,
        content: text,
        encoding: "utf8".to_string(),
        size,
        language: language.map(String::from),
        mime_type: None,
    })
}

/// Open a file in an editor (defaults to vscode).
#[tauri::command]
pub async fn open_in_editor(path: String, editor: Option<String>) -> Result<(), String> {
    let editor = editor.unwrap_or_else(|| "code".to_string());

    tokio::process::Command::new(&editor)
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open {path} with {editor}: {e}"))?;

    Ok(())
}

/// Get the current working directory.
#[tauri::command]
pub async fn get_cwd() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to get cwd: {e}"))
}

/// Open a URL in the system's default browser.
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "cmd";

    #[cfg(target_os = "windows")]
    {
        tokio::process::Command::new(cmd)
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        tokio::process::Command::new(cmd)
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    Ok(())
}
