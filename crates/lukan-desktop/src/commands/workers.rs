use lukan_core::workers::{
    WorkerCreateInput, WorkerDefinition, WorkerDetail, WorkerManager, WorkerRun, WorkerSummary,
    WorkerUpdateInput,
};

#[tauri::command]
pub async fn list_workers() -> Result<Vec<WorkerSummary>, String> {
    WorkerManager::get_summaries()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_worker(input: WorkerCreateInput) -> Result<WorkerDefinition, String> {
    WorkerManager::create(input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_worker(id: String, patch: WorkerUpdateInput) -> Result<WorkerDefinition, String> {
    WorkerManager::update(&id, patch)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Worker '{id}' not found"))
}

#[tauri::command]
pub async fn delete_worker(id: String) -> Result<bool, String> {
    WorkerManager::delete(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_worker(id: String, enabled: bool) -> Result<WorkerDefinition, String> {
    let patch = WorkerUpdateInput {
        name: None,
        schedule: None,
        prompt: None,
        tools: None,
        provider: None,
        model: None,
        enabled: Some(enabled),
        notify: None,
    };
    WorkerManager::update(&id, patch)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Worker '{id}' not found"))
}

#[tauri::command]
pub async fn get_worker_detail(id: String) -> Result<WorkerDetail, String> {
    WorkerManager::get_detail(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Worker '{id}' not found"))
}

#[tauri::command]
pub async fn get_worker_run(worker_id: String, run_id: String) -> Result<WorkerRun, String> {
    WorkerManager::get_run(&worker_id, &run_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Run '{run_id}' not found for worker '{worker_id}'"))
}
