use std::path::PathBuf;
use std::sync::Arc;

use lukan_agent::{AgentConfig, AgentLoop};
use lukan_core::config::types::PermissionMode;
use lukan_core::config::{LukanPaths, ResolvedConfig};
use lukan_core::models::events::{ApprovalResponse, PlanReviewResponse};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_configured_registry;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Shared chat state managed via tauri::State
pub struct ChatState {
    pub agent: Mutex<Option<AgentLoop>>,
    pub config: Mutex<Option<ResolvedConfig>>,
    pub is_processing: Mutex<bool>,
    pub permission_mode: Mutex<PermissionMode>,
    pub provider_name: Mutex<String>,
    pub model_name: Mutex<String>,
    pub approval_tx: Mutex<Option<mpsc::Sender<ApprovalResponse>>>,
    pub plan_review_tx: Mutex<Option<mpsc::Sender<PlanReviewResponse>>>,
    pub planner_answer_tx: Mutex<Option<mpsc::Sender<String>>>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            agent: Mutex::new(None),
            config: Mutex::new(None),
            is_processing: Mutex::new(false),
            permission_mode: Mutex::new(PermissionMode::Auto),
            provider_name: Mutex::new(String::new()),
            model_name: Mutex::new(String::new()),
            approval_tx: Mutex::new(None),
            plan_review_tx: Mutex::new(None),
            planner_answer_tx: Mutex::new(None),
        }
    }
}

impl ChatState {
    /// Create a new agent, storing approval/plan channels in state
    pub async fn create_agent(&self, config: &ResolvedConfig) -> anyhow::Result<AgentLoop> {
        let provider = create_provider(config)?;
        let system_prompt = build_system_prompt().await;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let provider_name = self.provider_name.lock().await.clone();
        let model_name = self.model_name.lock().await.clone();
        let permission_mode = self.permission_mode.lock().await.clone();

        let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
            .await
            .ok()
            .flatten()
            .map(|(_, cfg)| cfg);

        let permissions = project_cfg
            .as_ref()
            .map(|c| c.permissions.clone())
            .unwrap_or_default();

        let allowed = project_cfg
            .as_ref()
            .map(|c| c.resolve_allowed_paths(&cwd))
            .unwrap_or_else(|| vec![cwd.clone()]);

        // Create approval channel
        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalResponse>(1);
        *self.approval_tx.lock().await = Some(approval_tx);

        // Create plan review channel
        let (plan_review_tx, plan_review_rx) = mpsc::channel::<PlanReviewResponse>(1);
        *self.plan_review_tx.lock().await = Some(plan_review_tx);

        // Create planner answer channel
        let (planner_answer_tx, planner_answer_rx) = mpsc::channel::<String>(1);
        *self.planner_answer_tx.lock().await = Some(planner_answer_tx);

        let agent_config = AgentConfig {
            provider: Arc::from(provider),
            tools: create_configured_registry(&permissions, &allowed),
            system_prompt,
            cwd,
            provider_name,
            model_name,
            bg_signal: None,
            allowed_paths: Some(allowed),
            permission_mode,
            permissions,
            approval_rx: Some(approval_rx),
            plan_review_rx: Some(plan_review_rx),
            planner_answer_rx: Some(planner_answer_rx),
            browser_tools: false,
        };

        AgentLoop::new(agent_config).await
    }

    /// Create an agent with a loaded session
    pub async fn create_agent_with_session(
        &self,
        config: &ResolvedConfig,
        session_id: &str,
    ) -> anyhow::Result<AgentLoop> {
        let provider = create_provider(config)?;
        let system_prompt = build_system_prompt().await;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let provider_name = self.provider_name.lock().await.clone();
        let model_name = self.model_name.lock().await.clone();
        let permission_mode = self.permission_mode.lock().await.clone();

        let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
            .await
            .ok()
            .flatten()
            .map(|(_, cfg)| cfg);

        let permissions = project_cfg
            .as_ref()
            .map(|c| c.permissions.clone())
            .unwrap_or_default();

        let allowed = project_cfg
            .as_ref()
            .map(|c| c.resolve_allowed_paths(&cwd))
            .unwrap_or_else(|| vec![cwd.clone()]);

        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalResponse>(1);
        *self.approval_tx.lock().await = Some(approval_tx);

        let (plan_review_tx, plan_review_rx) = mpsc::channel::<PlanReviewResponse>(1);
        *self.plan_review_tx.lock().await = Some(plan_review_tx);

        let (planner_answer_tx, planner_answer_rx) = mpsc::channel::<String>(1);
        *self.planner_answer_tx.lock().await = Some(planner_answer_tx);

        let agent_config = AgentConfig {
            provider: Arc::from(provider),
            tools: create_configured_registry(&permissions, &allowed),
            system_prompt,
            cwd,
            provider_name,
            model_name,
            bg_signal: None,
            allowed_paths: Some(allowed),
            permission_mode,
            permissions,
            approval_rx: Some(approval_rx),
            plan_review_rx: Some(plan_review_rx),
            planner_answer_rx: Some(planner_answer_rx),
            browser_tools: false,
        };

        AgentLoop::load_session(agent_config, session_id).await
    }
}

/// Build system prompt (matches web/TUI logic)
async fn build_system_prompt() -> SystemPrompt {
    const BASE: &str = include_str!("../../../prompts/base.txt");
    let mut cached = vec![BASE.to_string()];

    // Load global memory
    let global_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            cached.push(format!("## Global Memory\n\n{trimmed}"));
        }
    }

    // Load project memory if active
    let active_path = LukanPaths::project_memory_active_file();
    if tokio::fs::metadata(&active_path).await.is_ok() {
        let project_path = LukanPaths::project_memory_file();
        if let Ok(memory) = tokio::fs::read_to_string(&project_path).await {
            let trimmed = memory.trim();
            if !trimmed.is_empty() {
                cached.push(format!("## Project Memory\n\n{trimmed}"));
            }
        }
    }

    // Load plugin prompts
    let plugins_dir = LukanPaths::plugins_dir();
    if let Ok(mut entries) = tokio::fs::read_dir(&plugins_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let prompt_path = entry.path().join("prompt.txt");
            if let Ok(prompt) = tokio::fs::read_to_string(&prompt_path).await {
                let trimmed = prompt.trim();
                if !trimmed.is_empty() {
                    cached.push(trimmed.to_string());
                }
            }
        }
    }

    let now = chrono::Utc::now();
    let tz_name = lukan_core::config::ConfigManager::load()
        .await
        .ok()
        .and_then(|c| c.timezone)
        .unwrap_or_else(|| "UTC".to_string());
    let dynamic = format!(
        "Current date: {} ({}). Use this for any time-relative operations.",
        now.format("%Y-%m-%d %H:%M UTC"),
        tz_name
    );

    SystemPrompt::Structured { cached, dynamic }
}
