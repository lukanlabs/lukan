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
