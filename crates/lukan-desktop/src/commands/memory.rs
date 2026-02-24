use lukan_core::config::LukanPaths;

#[tauri::command]
pub async fn get_global_memory() -> Result<String, String> {
    let path = LukanPaths::global_memory_file();
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_global_memory(content: String) -> Result<(), String> {
    let path = LukanPaths::global_memory_file();
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
pub async fn get_project_memory(path: String) -> Result<String, String> {
    let memory_file = std::path::PathBuf::from(&path)
        .join(".lukan")
        .join("memories")
        .join("MEMORY.md");

    if !memory_file.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&memory_file)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_project_memory(path: String, content: String) -> Result<(), String> {
    let memory_dir = std::path::PathBuf::from(&path)
        .join(".lukan")
        .join("memories");
    let memory_file = memory_dir.join("MEMORY.md");

    tokio::fs::create_dir_all(&memory_dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(&memory_file, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_project_memory_active(path: String) -> Result<bool, String> {
    let active_file = std::path::PathBuf::from(&path)
        .join(".lukan")
        .join("memories")
        .join(".active");
    Ok(active_file.exists())
}

#[tauri::command]
pub async fn toggle_project_memory(path: String, active: bool) -> Result<(), String> {
    let memory_dir = std::path::PathBuf::from(&path)
        .join(".lukan")
        .join("memories");
    let active_file = memory_dir.join(".active");

    tokio::fs::create_dir_all(&memory_dir)
        .await
        .map_err(|e| e.to_string())?;

    if active {
        tokio::fs::write(&active_file, "")
            .await
            .map_err(|e| e.to_string())?;
    } else if active_file.exists() {
        tokio::fs::remove_file(&active_file)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
