use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use lukan_core::config::LukanPaths;
use lukan_core::config::types::{PermissionMode, PermissionsConfig};
use lukan_core::models::checkpoints::{Checkpoint, FileSnapshot};
use lukan_core::models::events::{
    ApprovalResponse, PlanReviewResponse, PlanTask, PlannerQuestionItem, StopReason, StreamEvent,
    TaskInfo, ToolApprovalRequest,
};
use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};
use lukan_core::models::sessions::ChatSession;
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use lukan_tools::{ToolContext, ToolRegistry};

use crate::permission_matcher::{
    PLANNER_TOOL_WHITELIST, PermissionMatcher, ToolVerdict, generate_allow_pattern,
};
use lukan_core::config::project_config::ProjectConfig;
use rand::Rng;
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::memory_helpers::{active_memory_path, extract_section, format_messages_for_context};
use crate::message_history::MessageHistory;
use crate::session_manager::SessionManager;
use crate::vision_preprocessor::extract_image_urls;

// ── Thresholds ────────────────────────────────────────────────────────────

/// When context tokens reach this, trigger MEMORY.md update
const MEMORY_UPDATE_THRESHOLD: u64 = 20_000;
/// When context tokens reach this, trigger auto-compaction
const COMPACTION_THRESHOLD: u64 = 150_000;
/// Keep last N messages during compaction; summarize everything before
const COMPACTION_KEEP_MESSAGES: usize = 10;

// ── Prompts (embedded at compile time) ────────────────────────────────────

const COMPACTION_SIMPLE_PROMPT: &str = include_str!("../../../prompts/compaction-simple.txt");
const COMPACTION_WITH_MEMORY_PROMPT: &str =
    include_str!("../../../prompts/compaction-with-memory.txt");
#[allow(dead_code)] // kept for behavior profile MEMORY.md fallback
const MEMORY_UPDATE_PROMPT: &str = include_str!("../../../prompts/memory_update.txt");
const STRUCTURED_MEMORY_UPDATE_PROMPT: &str =
    include_str!("../../../prompts/structured_memory_update.txt");
const BEHAVIOR_MEMORY_UPDATE_PROMPT: &str =
    include_str!("../../../prompts/behavior_memory_update.txt");
const PLANNER_PROMPT: &str = include_str!("../../../prompts/planner.txt");
const TASK_TRACKING_PROMPT: &str = include_str!("../../../prompts/task-tracking.txt");

/// Configuration for creating an AgentLoop
pub struct AgentConfig {
    pub provider: Arc<dyn Provider>,
    pub tools: ToolRegistry,
    pub system_prompt: SystemPrompt,
    pub cwd: PathBuf,
    /// Provider name for session metadata
    pub provider_name: String,
    /// Model name for session metadata
    pub model_name: String,
    /// Optional signal receiver for Alt+B (send Bash to background)
    pub bg_signal: Option<watch::Receiver<()>>,
    /// Hard path restrictions for file tools (from plugin security)
    pub allowed_paths: Option<Vec<PathBuf>>,
    /// Permission mode for tool execution
    pub permission_mode: PermissionMode,
    /// Optional watch receiver for live permission mode updates (desktop UI)
    pub permission_mode_rx: Option<watch::Receiver<PermissionMode>>,
    /// Permission rules (deny/ask/allow lists)
    pub permissions: PermissionsConfig,
    /// Channel to receive approval responses from the UI
    pub approval_rx: Option<mpsc::Receiver<ApprovalResponse>>,
    /// Channel to receive plan review responses from the UI
    pub plan_review_rx: Option<mpsc::Receiver<PlanReviewResponse>>,
    /// Channel to receive planner question answers from the UI
    pub planner_answer_rx: Option<mpsc::Receiver<String>>,
    /// Whether browser tools are enabled (auto-allow without asking)
    pub browser_tools: bool,
    /// When true, `save_session()` becomes a no-op (used by workers)
    pub skip_session_save: bool,
    /// Optional vision-capable provider for describing images on non-vision models
    pub vision_provider: Option<Arc<dyn Provider>>,
    /// Extra environment variables injected into Bash commands (e.g. skill credentials)
    #[allow(dead_code)]
    pub extra_env: HashMap<String, String>,
    /// Context token threshold for auto-compaction (None = use default 150k)
    pub compaction_threshold: Option<u64>,
}

/// Pending tool call accumulated from stream events
struct PendingToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
}

/// The agent loop that coordinates LLM ↔ Tools
pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    system_prompt: SystemPrompt,
    cwd: PathBuf,
    history: MessageHistory,
    session: ChatSession,
    input_tokens: u64,
    output_tokens: u64,
    /// Last context size (input tokens from most recent LLM call)
    last_context_size: u64,
    /// Tokens at last memory update
    last_memory_update_tokens: u64,
    read_files: Arc<Mutex<HashMap<PathBuf, Option<std::time::SystemTime>>>>,
    /// Optional signal receiver for Alt+B (send Bash to background)
    bg_signal: Option<watch::Receiver<()>>,
    /// Hard path restrictions for file tools (from plugin security)
    allowed_paths: Option<Vec<PathBuf>>,
    /// Permission matcher for tool approval
    permission_matcher: PermissionMatcher,
    /// Channel to receive approval responses from the UI
    approval_rx: Option<mpsc::Receiver<ApprovalResponse>>,
    /// Channel to receive plan review responses from the UI
    plan_review_rx: Option<mpsc::Receiver<PlanReviewResponse>>,
    /// Channel to receive planner question answers from the UI
    planner_answer_rx: Option<mpsc::Receiver<String>>,
    /// Filename of the currently active plan (e.g. "2024-01-15-add-auth.md")
    current_plan_file: Option<String>,
    /// Full markdown content of the currently active plan
    current_plan_content: Option<String>,
    /// Skills discovered at init from `.lukan/skills/`
    available_skills: Vec<lukan_tools::skills::SkillInfo>,
    /// Skills already loaded in this session (by folder name)
    loaded_skills: HashSet<String>,
    /// Pending system events from plugins (injected into next turn)
    pending_events: Vec<PendingEvent>,
    /// Tool names disabled at runtime by the TUI tool picker
    disabled_tools: HashSet<String>,
    /// When true, `save_session()` is a no-op
    skip_session_save: bool,
    /// Optional vision-capable provider for describing images on non-vision models
    vision_provider: Option<Arc<dyn Provider>>,
    /// Extra environment variables injected into Bash commands (e.g. skill credentials)
    extra_env: HashMap<String, String>,
    /// Human-readable label for this agent (e.g. "Agent 2"), shown in bg process list
    pub label: Option<String>,
    /// Frontend tab ID for matching bg processes with UI tabs
    pub tab_id: Option<String>,
    /// Environment variable names whose values are redacted from tool output
    blocked_env_vars: Vec<String>,
    /// Context token threshold for auto-compaction
    compaction_threshold: u64,
}

/// A system event from a plugin, queued for injection into the agent context.
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub ts: String,
    pub source: String,
    pub level: String,
    pub detail: String,
}

impl AgentLoop {
    /// Create a new agent with a fresh session
    pub async fn new(mut config: AgentConfig) -> Result<Self> {
        // Register sub-agent tools
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentResultTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::ExploreTool));

        // Build sandbox config for sub-agents from registry settings
        let sub_agent_sandbox = if config.tools.is_sandbox_enabled() {
            Some(lukan_tools::sandbox::SandboxConfig {
                enabled: true,
                allowed_dirs: config.tools.allowed_dirs().to_vec(),
                sensitive_patterns: config.tools.sensitive_patterns().to_vec(),
            })
        } else {
            None
        };

        // Configure the global sub-agent manager
        crate::sub_agent::configure(
            Arc::clone(&config.provider),
            config.system_prompt.clone(),
            config.cwd.clone(),
            config.provider_name.clone(),
            config.model_name.clone(),
            sub_agent_sandbox,
            config.allowed_paths.clone(),
        )
        .await;

        let bg_signal = config.bg_signal.take();
        let allowed_paths = Self::expand_allowed_paths(config.allowed_paths.take());
        let mut permission_matcher =
            PermissionMatcher::new(config.permission_mode, &config.permissions);
        if let Some(rx) = config.permission_mode_rx.take() {
            permission_matcher.set_mode_watch(rx);
        }
        if config.browser_tools {
            permission_matcher.enable_browser_tools();
        }
        let approval_rx = config.approval_rx;
        let plan_review_rx = config.plan_review_rx;
        let planner_answer_rx = config.planner_answer_rx;
        let skip_session_save = config.skip_session_save;
        let mut session = SessionManager::create(&config.provider_name, &config.model_name).await?;
        session.cwd = Some(config.cwd.to_string_lossy().to_string());
        let available_skills = lukan_tools::skills::discover_skills(&config.cwd).await;
        Ok(Self {
            provider: config.provider,
            tools: Arc::new(config.tools),
            system_prompt: config.system_prompt,
            cwd: config.cwd,
            history: MessageHistory::new(),
            session,
            input_tokens: 0,
            output_tokens: 0,
            last_context_size: 0,
            last_memory_update_tokens: 0,
            read_files: Arc::new(Mutex::new(HashMap::new())),
            bg_signal,
            allowed_paths,
            permission_matcher,
            approval_rx,
            plan_review_rx,
            planner_answer_rx,
            current_plan_file: None,
            current_plan_content: None,
            available_skills,
            loaded_skills: HashSet::new(),
            pending_events: Vec::new(),
            disabled_tools: HashSet::new(),
            skip_session_save,
            vision_provider: config.vision_provider,
            extra_env: config.extra_env,
            label: None,
            tab_id: None,
            blocked_env_vars: Vec::new(),
            compaction_threshold: config.compaction_threshold.unwrap_or(COMPACTION_THRESHOLD),
        })
    }

    /// Load an existing session and restore history
    pub async fn load_session(mut config: AgentConfig, session_id: &str) -> Result<Self> {
        // Register sub-agent tools
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentResultTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::ExploreTool));

        // Build sandbox config for sub-agents from registry settings
        let sub_agent_sandbox = if config.tools.is_sandbox_enabled() {
            Some(lukan_tools::sandbox::SandboxConfig {
                enabled: true,
                allowed_dirs: config.tools.allowed_dirs().to_vec(),
                sensitive_patterns: config.tools.sensitive_patterns().to_vec(),
            })
        } else {
            None
        };

        crate::sub_agent::configure(
            Arc::clone(&config.provider),
            config.system_prompt.clone(),
            config.cwd.clone(),
            config.provider_name.clone(),
            config.model_name.clone(),
            sub_agent_sandbox,
            config.allowed_paths.clone(),
        )
        .await;

        let bg_signal = config.bg_signal.take();
        let allowed_paths = Self::expand_allowed_paths(config.allowed_paths.take());
        let mut permission_matcher =
            PermissionMatcher::new(config.permission_mode, &config.permissions);
        if let Some(rx) = config.permission_mode_rx.take() {
            permission_matcher.set_mode_watch(rx);
        }
        if config.browser_tools {
            permission_matcher.enable_browser_tools();
        }
        let approval_rx = config.approval_rx;
        let plan_review_rx = config.plan_review_rx;
        let planner_answer_rx = config.planner_answer_rx;
        let skip_session_save = config.skip_session_save;
        let session = SessionManager::load(session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {session_id}"))?;

        let mut history = MessageHistory::new();
        history.load_from_json(session.messages.clone());

        let available_skills = lukan_tools::skills::discover_skills(&config.cwd).await;
        Ok(Self {
            provider: config.provider,
            tools: Arc::new(config.tools),
            system_prompt: config.system_prompt,
            cwd: config.cwd,
            history,
            input_tokens: session.total_input_tokens,
            output_tokens: session.total_output_tokens,
            last_context_size: session.last_context_size,
            last_memory_update_tokens: session.last_memory_update_tokens,
            session,
            read_files: Arc::new(Mutex::new(HashMap::new())),
            bg_signal,
            allowed_paths,
            permission_matcher,
            approval_rx,
            plan_review_rx,
            planner_answer_rx,
            current_plan_file: None,
            current_plan_content: None,
            available_skills,
            loaded_skills: HashSet::new(),
            pending_events: Vec::new(),
            disabled_tools: HashSet::new(),
            skip_session_save,
            vision_provider: config.vision_provider,
            extra_env: config.extra_env,
            label: None,
            tab_id: None,
            blocked_env_vars: Vec::new(),
            compaction_threshold: config.compaction_threshold.unwrap_or(COMPACTION_THRESHOLD),
        })
    }

    /// Expand allowed_paths to include lukan's own config/data directories.
    /// The agent needs access to these for sessions, memory, plugin configs, etc.
    fn expand_allowed_paths(paths: Option<Vec<PathBuf>>) -> Option<Vec<PathBuf>> {
        let mut dirs = paths?;

        // Always allow lukan's config directory (~/.config/lukan/)
        let config_dir = lukan_core::config::LukanPaths::config_dir();
        if !dirs.iter().any(|d| config_dir.starts_with(d)) {
            dirs.push(config_dir);
        }

        // Allow lukan's data directory (~/.local/share/lukan/)
        let data_dir = Some(lukan_core::config::LukanPaths::data_dir());
        if let Some(data_dir) = data_dir
            && !dirs.iter().any(|d| data_dir.starts_with(d))
        {
            dirs.push(data_dir);
        }

        // Allow /tmp for temporary files
        let tmp = PathBuf::from("/tmp");
        if !dirs.iter().any(|d| tmp.starts_with(d)) {
            dirs.push(tmp);
        }

        Some(dirs)
    }

    /// Set a human-readable name for this session and persist to disk.
    pub async fn set_session_name(&mut self, name: String) -> Result<()> {
        self.session.name = Some(name);
        if !self.skip_session_save {
            SessionManager::save(&mut self.session).await?;
        }
        Ok(())
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    /// Total input tokens used across all turns
    pub fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Total output tokens used across all turns
    pub fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Number of messages in the current history
    pub fn message_count(&self) -> usize {
        self.history.messages().len()
    }

    /// Checkpoints recorded in this session
    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.session.checkpoints
    }

    /// Get messages as serializable JSON values (for web UI)
    pub fn messages_json(&self) -> Vec<Message> {
        self.history.messages().to_vec()
    }

    /// Last context size (input tokens from most recent LLM call)
    pub fn last_context_size(&self) -> u64 {
        self.last_context_size
    }

    /// Access the underlying session
    pub fn session(&self) -> &ChatSession {
        &self.session
    }

    /// Save session to disk (public wrapper)
    pub async fn save_session_public(&mut self) -> Result<()> {
        self.save_session().await
    }

    /// Save session to disk only if our in-memory copy is not stale.
    /// If another client has written a newer version to disk, reload from
    /// disk instead of overwriting with our (potentially outdated) data.
    pub async fn save_session_if_not_stale(&mut self) -> Result<()> {
        let disk_session = match SessionManager::load(&self.session.id).await {
            Ok(Some(s)) => s,
            Ok(None) => return self.save_session().await, // file gone, safe to write
            Err(_) => return self.save_session().await,   // can't read, try saving anyway
        };

        if disk_session.updated_at > self.session.updated_at {
            // Disk is newer — another client updated this session.
            // Reload instead of overwriting.
            info!(
                session_id = %self.session.id,
                in_memory = %self.session.updated_at,
                on_disk = %disk_session.updated_at,
                "Skipping save: disk session is newer (updated by another client)"
            );
            self.history = MessageHistory::new();
            self.history.load_from_json(disk_session.messages.clone());
            self.input_tokens = disk_session.total_input_tokens;
            self.output_tokens = disk_session.total_output_tokens;
            self.last_context_size = disk_session.last_context_size;
            self.last_memory_update_tokens = disk_session.last_memory_update_tokens;
            self.session = disk_session;
            Ok(())
        } else {
            self.save_session().await
        }
    }

    /// Reload session from disk if another process has updated it.
    /// Compares the on-disk `updated_at` with our in-memory copy;
    /// if the disk version is newer, replaces our history and metadata.
    async fn reload_if_stale(&mut self) {
        let disk_session = match SessionManager::load(&self.session.id).await {
            Ok(Some(s)) => s,
            _ => return, // file missing or unreadable — keep in-memory state
        };

        if disk_session.updated_at > self.session.updated_at {
            info!(
                session_id = %self.session.id,
                in_memory = %self.session.updated_at,
                on_disk = %disk_session.updated_at,
                "Session updated by another client, reloading from disk"
            );
            self.history = MessageHistory::new();
            self.history.load_from_json(disk_session.messages.clone());
            self.input_tokens = disk_session.total_input_tokens;
            self.output_tokens = disk_session.total_output_tokens;
            self.last_context_size = disk_session.last_context_size;
            self.last_memory_update_tokens = disk_session.last_memory_update_tokens;
            self.session = disk_session;
        }
    }

    /// Get a reference to the provider name stored in the session
    pub fn provider_name(&self) -> Option<&str> {
        self.session.provider.as_deref()
    }

    /// Get a reference to the model name stored in the session
    pub fn model_name(&self) -> Option<&str> {
        self.session.model.as_deref()
    }

    /// Restore to a checkpoint, truncating history and optionally reverting files.
    ///
    /// Returns `true` if the checkpoint was found and the restore succeeded.
    /// When `restore_code` is true, files are reverted to their state *before*
    /// the checkpoint's turn using the `before` snapshots.
    pub async fn restore_checkpoint(
        &mut self,
        checkpoint_id: &str,
        restore_code: bool,
    ) -> Result<bool> {
        // Find the target checkpoint index
        let Some(target_idx) = self
            .session
            .checkpoints
            .iter()
            .position(|c| c.id == checkpoint_id)
        else {
            return Ok(false);
        };

        let target = &self.session.checkpoints[target_idx];
        let message_index = target.message_index;

        // If restoring code, revert files from all checkpoints at and after target
        // (including target, since we're rewinding to *before* that turn).
        if restore_code {
            // Process in reverse order so earlier snapshots win on conflicts
            for cp in self.session.checkpoints[target_idx..].iter().rev() {
                for snap in &cp.snapshots {
                    let path = std::path::Path::new(&snap.path);
                    match snap.operation {
                        lukan_core::models::checkpoints::FileOperation::Created => {
                            // File was created during this turn → delete it
                            let _ = tokio::fs::remove_file(path).await;
                        }
                        lukan_core::models::checkpoints::FileOperation::Modified
                        | lukan_core::models::checkpoints::FileOperation::Deleted => {
                            // Restore the "before" content
                            if let Some(ref before) = snap.before {
                                if let Some(parent) = path.parent() {
                                    let _ = tokio::fs::create_dir_all(parent).await;
                                }
                                let _ = tokio::fs::write(path, before).await;
                            }
                        }
                    }
                }
            }
        }

        // Truncate history to the point before the target checkpoint's turn
        self.history.truncate(message_index);

        // Remove checkpoints at and after the target
        self.session.checkpoints.truncate(target_idx);

        // Clear read_files cache since the file state has changed
        self.read_files.lock().await.clear();

        // Save session
        self.save_session().await?;

        info!(
            checkpoint_id,
            message_index, restore_code, "Restored checkpoint"
        );

        Ok(true)
    }

    /// Manually trigger conversation compaction
    pub async fn compact(&mut self, event_tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        self.compact_history(&event_tx).await
    }

    /// Replace the system prompt (e.g. after memory changes)
    pub fn reload_system_prompt(&mut self, new_prompt: lukan_providers::SystemPrompt) {
        self.system_prompt = new_prompt;
    }

    /// Swap the LLM provider (e.g. after model switch) without losing history
    pub fn swap_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider;
    }

    /// Swap the tool registry (e.g. after config change) without losing history
    pub fn reload_tools(&mut self, new_registry: ToolRegistry) {
        self.tools = Arc::new(new_registry);
    }

    /// Get the current permission mode
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_matcher.mode()
    }

    /// Update the permission mode at runtime
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_matcher.set_mode(mode);
    }

    /// Add an allow rule to the in-memory permission matcher
    pub fn add_allow_rule(&mut self, pattern: &str) {
        self.permission_matcher.add_allow_rule(pattern);
    }

    /// Enable browser tools in the permission matcher (auto-allow without asking)
    pub fn enable_browser_tools(&mut self) {
        self.permission_matcher.enable_browser_tools();
    }

    /// Get the current plan file name (if any)
    pub fn current_plan_file(&self) -> Option<&str> {
        self.current_plan_file.as_deref()
    }

    /// Load pending system events from disk (`~/.config/lukan/events/pending.jsonl`).
    /// Drains the file after reading.
    pub async fn load_pending_events(&mut self) {
        let path = LukanPaths::pending_events_file();
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => return,
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                self.pending_events.push(PendingEvent {
                    ts: val
                        .get("ts")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    source: val
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    level: val
                        .get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string(),
                    detail: val
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                });
            }
        }
        // Truncate the file after reading
        let _ = tokio::fs::write(&path, "").await;
        if !self.pending_events.is_empty() {
            info!(
                count = self.pending_events.len(),
                "Loaded pending system events from disk"
            );
        }
    }

    /// Push a live system event from a plugin (called by PluginChannel).
    pub fn push_event(&mut self, source: &str, level: &str, detail: &str) {
        self.pending_events.push(PendingEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            source: source.to_string(),
            level: level.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Get pending events (for TUI display).
    pub fn pending_events(&self) -> &[PendingEvent] {
        &self.pending_events
    }

    /// Replace the approval/plan/planner channels and bg_signal on an existing agent.
    /// This MUST be called before each turn when reusing an agent, so that
    /// stale receivers (whose senders were dropped) don't cause auto-denial.
    pub fn set_channels(
        &mut self,
        approval_rx: Option<mpsc::Receiver<ApprovalResponse>>,
        plan_review_rx: Option<mpsc::Receiver<PlanReviewResponse>>,
        planner_answer_rx: Option<mpsc::Receiver<String>>,
        bg_signal: Option<watch::Receiver<()>>,
    ) {
        self.approval_rx = approval_rx;
        self.plan_review_rx = plan_review_rx;
        self.planner_answer_rx = planner_answer_rx;
        self.bg_signal = bg_signal;
    }

    /// Set tools that should be excluded from tool definitions.
    pub fn set_disabled_tools(&mut self, disabled: HashSet<String>) {
        self.disabled_tools = disabled.clone();
        // Propagate to sub-agent manager so spawned sub-agents inherit the restriction
        tokio::spawn(async move {
            crate::sub_agent::set_disabled_tools(disabled).await;
        });
    }

    /// Set env var names whose values should be redacted from tool output.
    pub fn set_blocked_env_vars(&mut self, vars: Vec<String>) {
        self.blocked_env_vars = vars;
    }

    /// Rebuild the system prompt based on current mode and plan state.
    /// Call this after mode changes (e.g. planner → auto on plan accept).
    pub async fn rebuild_system_prompt(&mut self) {
        let base = if self.permission_matcher.mode() == PermissionMode::Planner {
            PLANNER_PROMPT
        } else {
            include_str!("../../../prompts/base.txt")
        };

        let mut cached = vec![base.to_string(), TASK_TRACKING_PROMPT.to_string()];

        // Load global memory
        let global_path = LukanPaths::global_memory_file();
        if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
            let trimmed = memory.trim();
            if !trimmed.is_empty() {
                cached.push(format!("## Global Memory\n\n{trimmed}"));
            }
        }

        // Load project memories — prefer structured files, fallback to MEMORY.md
        if let Some(summaries) =
            lukan_tools::remember::get_memory_summaries_for_prompt(&self.cwd).await
        {
            cached.push(summaries);
        }
        // Always load behavior profile (MEMORY.md) if it exists
        let active_path = LukanPaths::project_memory_active_file();
        if tokio::fs::metadata(&active_path).await.is_ok() {
            let project_path = LukanPaths::project_memory_file();
            if let Ok(memory) = tokio::fs::read_to_string(&project_path).await {
                let trimmed = memory.trim();
                if !trimmed.is_empty() {
                    cached.push(format!("## Project Behavior Profile\n\n{trimmed}"));
                }
            }
        }

        // Dynamic section: date + tasks + active plan
        let now = chrono::Utc::now();
        let tz_name = lukan_core::config::ConfigManager::load()
            .await
            .ok()
            .and_then(|c| c.timezone)
            .unwrap_or_else(|| "UTC".to_string());
        let mut dynamic = format!(
            "Current date: {} ({}). Use this for any time-relative operations.\n\
             Working directory: {}. All file operations and commands run here. Do NOT cd to other directories or operate on files outside this workspace.",
            now.format("%Y-%m-%d %H:%M UTC"),
            tz_name,
            self.cwd.display()
        );

        // Include current tasks in dynamic section
        if let Some(tasks_section) = lukan_tools::tasks::get_tasks_for_prompt(&self.cwd).await {
            dynamic.push_str("\n\n## Current Tasks\n\n");
            dynamic.push_str(&tasks_section);
        }

        // Include available skills in dynamic section
        if !self.available_skills.is_empty() {
            dynamic.push_str("\n\n## Available Skills\nIMPORTANT: When the user's request matches a skill below, you MUST call the LoadSkill tool with the skill's folder name BEFORE starting the task. The skill may contain project-specific instructions that override default behavior.\n");
            for skill in &self.available_skills {
                dynamic.push_str(&format!("- **{}**: {}\n", skill.folder, skill.description));
            }
            if !self.loaded_skills.is_empty() {
                let mut list: Vec<&str> = self.loaded_skills.iter().map(|s| s.as_str()).collect();
                list.sort();
                dynamic.push_str(&format!(
                    "\nAlready loaded (no need to reload): {}",
                    list.join(", ")
                ));
            }
        }

        // Include active plan content in dynamic section (when not in planner mode)
        if self.permission_matcher.mode() != PermissionMode::Planner
            && let Some(ref plan_content) = self.current_plan_content
        {
            dynamic.push_str("\n\n## Active Plan\n\n");
            dynamic.push_str(plan_content);
        }

        self.system_prompt = SystemPrompt::Structured { cached, dynamic };
    }

    /// Add user context (e.g. shell command output) without triggering a turn
    pub fn add_user_context(&mut self, content: &str) {
        self.history.add_user_message(content);
    }

    /// Run a single user turn: sends the message and loops until no more tool calls.
    /// The optional `cancel` token allows the caller (TUI) to abort mid-turn.
    pub async fn run_turn(
        &mut self,
        user_message: &str,
        event_tx: mpsc::Sender<StreamEvent>,
        cancel: Option<CancellationToken>,
        queued: Option<Arc<std::sync::Mutex<Vec<String>>>>,
    ) -> Result<()> {
        // Reload from disk if another client (CLI / Web) updated this session
        self.reload_if_stale().await;

        // Capture message index *before* adding the user message.
        // This is the truncation point used by restore_checkpoint().
        let message_index_before = self.history.messages().len();

        // Inject pending system events as context before the user message
        if !self.pending_events.is_empty() {
            let mut ctx = String::from(
                "[SYSTEM EVENTS — the following occurred since your last interaction]\n",
            );
            for ev in self.pending_events.drain(..) {
                ctx.push_str(&format!(
                    "- [{}] ({}) {}: {}\n",
                    ev.ts,
                    ev.level.to_uppercase(),
                    ev.source,
                    ev.detail
                ));
            }
            self.history.add_user_message(&ctx);
        }

        // Add user message to history, extracting any image URLs as vision blocks
        let (clean_text, image_blocks) = extract_image_urls(user_message).await;
        if !image_blocks.is_empty() {
            tracing::info!(
                count = image_blocks.len(),
                "Extracted image URLs from user message"
            );
        }
        if image_blocks.is_empty() {
            self.history.add_user_message(user_message);
        } else {
            let mut blocks = vec![ContentBlock::Text { text: clean_text }];
            blocks.extend(image_blocks);
            self.history.add_user_blocks(blocks);
        }

        // Accumulate file snapshots across all tool rounds in this turn
        let mut turn_snapshots: Vec<FileSnapshot> = Vec::new();

        // Inner loop: call LLM → execute tools → repeat until done
        loop {
            let mut tool_defs = self.tools.default_definitions();
            // In planner mode, only expose read-only tools to the LLM
            if self.permission_matcher.mode() == PermissionMode::Planner {
                tool_defs.retain(|d| {
                    PLANNER_TOOL_WHITELIST.contains(&d.name.as_str())
                        && self
                            .tools
                            .get(&d.name)
                            .map(|tool| tool.is_read_only())
                            .unwrap_or(false)
                });
            }
            // Also hide tools disabled at runtime by the TUI
            tool_defs.retain(|d| !self.disabled_tools.contains(&d.name));

            if !tool_defs.iter().any(|d| d.name == "ToolSearch")
                && self.tools.get("ToolSearch").is_some()
                && !self.disabled_tools.contains("ToolSearch")
                && (self.tools.deferred_definitions().iter().any(|d| !self.disabled_tools.contains(&d.name)))
            {
                if let Some(tool_search_def) = self
                    .tools
                    .definitions()
                    .into_iter()
                    .find(|d| d.name == "ToolSearch")
                {
                    tool_defs.push(tool_search_def);
                    tool_defs.sort_by(|a, b| a.name.cmp(&b.name));
                }
            }

            // Preprocess images for non-vision providers
            let messages = crate::vision_preprocessor::preprocess_images(
                self.history.messages(),
                self.provider.as_ref(),
                &self.vision_provider,
            )
            .await;

            let params = StreamParams {
                system_prompt: self.system_prompt.clone(),
                messages,
                tools: tool_defs,
            };

            // Stream from LLM
            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
            let provider = Arc::clone(&self.provider);

            let stream_handle = tokio::spawn(async move {
                if let Err(e) = provider.stream(params, stream_tx.clone()).await {
                    error!("Provider stream error: {e}");
                    // Send error event so the TUI/UI can display the error
                    // instead of hanging silently.
                    let _ = stream_tx
                        .send(StreamEvent::Error {
                            error: e.to_string(),
                        })
                        .await;
                    let _ = stream_tx
                        .send(StreamEvent::MessageEnd {
                            stop_reason: StopReason::Error,
                        })
                        .await;
                }
            });

            // Accumulate the response
            let mut text_content = String::new();
            let mut thinking_content = String::new();
            let mut pending_tools: Vec<PendingToolCall> = Vec::new();
            let mut stop_reason = StopReason::EndTurn;

            let mut cancelled = false;
            loop {
                let event = if let Some(ref token) = cancel {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            cancelled = true;
                            break;
                        }
                        ev = stream_rx.recv() => ev,
                    }
                } else {
                    stream_rx.recv().await
                };

                let Some(event) = event else { break };

                match &event {
                    StreamEvent::TextDelta { text } => {
                        text_content.push_str(text);
                    }
                    StreamEvent::ThinkingDelta { text } => {
                        thinking_content.push_str(text);
                    }
                    StreamEvent::ToolUseEnd { id, name, input } => {
                        pending_tools.push(PendingToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        });
                    }
                    StreamEvent::Usage {
                        input_tokens,
                        output_tokens,
                        ..
                    } => {
                        self.input_tokens += input_tokens;
                        self.output_tokens += output_tokens;
                        // Track last context size for compaction decisions
                        self.last_context_size = *input_tokens;
                    }
                    StreamEvent::MessageEnd {
                        stop_reason: reason,
                    } => {
                        stop_reason = reason.clone();
                    }
                    _ => {}
                }

                let is_message_end = matches!(&event, StreamEvent::MessageEnd { .. });

                // Forward all events to the TUI
                if event_tx.send(event).await.is_err() {
                    warn!("Event receiver dropped, aborting turn");
                    stream_handle.abort();
                    // Save any partial text so the conversation stays coherent
                    if !text_content.is_empty() {
                        self.history
                            .add_assistant_blocks(vec![ContentBlock::Text { text: text_content }]);
                    }
                    self.save_session().await?;
                    return Ok(());
                }

                // Break after MessageEnd — don't wait for the channel to close,
                // as some providers may keep the HTTP connection alive after [DONE].
                if is_message_end {
                    break;
                }
            }

            if cancelled {
                stream_handle.abort();
                info!("Turn cancelled by user");
                // Save any partial text so the conversation stays coherent
                if !text_content.is_empty() {
                    self.history
                        .add_assistant_blocks(vec![ContentBlock::Text { text: text_content }]);
                }
                self.save_session().await?;
                return Ok(());
            }

            // Wait for provider task to finish
            let _ = stream_handle.await;

            // Build assistant message with all content blocks
            let mut blocks = Vec::new();

            if !thinking_content.is_empty() {
                blocks.push(ContentBlock::Thinking {
                    text: thinking_content,
                });
            }

            if !text_content.is_empty() {
                blocks.push(ContentBlock::Text {
                    text: text_content.clone(),
                });
            }

            for tool in &pending_tools {
                blocks.push(ContentBlock::ToolUse {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                });
            }

            if !blocks.is_empty() {
                self.history.add_assistant_blocks(blocks);
                // Incremental save: persist after each LLM response so nothing
                // is lost if the connection drops or the process exits.
                self.save_session().await?;
            }

            // If no tool calls, we're done — unless queued messages are waiting
            if stop_reason != StopReason::ToolUse || pending_tools.is_empty() {
                if let Some(ref queue) = queued {
                    let messages: Vec<String> = queue.lock().unwrap().drain(..).collect();
                    if !messages.is_empty() {
                        let injected = messages.join("\n");
                        let _ = event_tx
                            .send(StreamEvent::QueuedMessageInjected {
                                text: injected.clone(),
                            })
                            .await;
                        self.history.add_user_message(&injected);
                        continue;
                    }
                }
                debug!(
                    stop_reason = ?stop_reason,
                    "Turn complete, no tool calls"
                );
                break;
            }

            // Check cancellation before executing tools
            if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                info!("Turn cancelled before tool execution");
                self.save_session().await?;
                return Ok(());
            }

            // ── Intercept planner tools ──────────────────────────────────
            // PlannerQuestion and SubmitPlan are handled specially and
            // removed from pending_tools before the normal permission check.
            let intercepted = self
                .intercept_planner_tools(&mut pending_tools, &event_tx)
                .await;

            // If all tools were intercepted (nothing left), continue loop
            if pending_tools.is_empty() && intercepted {
                continue;
            }

            // ── Pre-validation: catch tools that will fail before asking approval ──
            // EditFile requires the file to have been read first; reject early
            // so the user isn't asked to approve a tool that will just error.
            let read_files_snapshot = self.read_files.lock().await.clone();
            let mut preflight_failed: Vec<(usize, lukan_core::models::tools::ToolResult)> =
                Vec::new();
            for (idx, tool) in pending_tools.iter().enumerate() {
                if tool.name == "EditFile"
                    && let Some(fp) = tool.input.get("file_path").and_then(|v| v.as_str())
                {
                    let path = std::path::PathBuf::from(fp);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        self.cwd.join(&path)
                    };
                    if !read_files_snapshot.contains_key(&path) {
                        preflight_failed.push((
                            idx,
                            lukan_core::models::tools::ToolResult::error(format!(
                                "File has not been read yet. Use ReadFiles first: {fp}"
                            )),
                        ));
                    }
                }

                // WriteFile with missing/empty params — skip approval, return error
                if tool.name == "WriteFile" {
                    let fp = tool
                        .input
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let content = tool
                        .input
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if fp.is_empty() || content.is_empty() {
                        preflight_failed.push((
                            idx,
                            lukan_core::models::tools::ToolResult::error(
                                "WriteFile called with empty file_path or content. Re-generate the full file content and try again."
                            ),
                        ));
                    }
                }
            }
            // Remove preflight-failed tools from pending so they skip approval
            let preflight_ids: std::collections::HashSet<usize> =
                preflight_failed.iter().map(|(i, _)| *i).collect();

            // ── Permission check ─────────────────────────────────────────
            // Classify each pending tool as allowed, needs_approval, or denied
            let mut allowed_tools: Vec<&PendingToolCall> = Vec::new();
            let mut needs_approval: Vec<&PendingToolCall> = Vec::new();
            let mut denied_results: Vec<(usize, lukan_core::models::tools::ToolResult)> =
                Vec::new();
            // Start with preflight failures
            denied_results.extend(preflight_failed);

            for (idx, tool) in pending_tools.iter().enumerate() {
                if preflight_ids.contains(&idx) {
                    continue;
                }
                match self.permission_matcher.verdict(&tool.name, &tool.input) {
                    ToolVerdict::Allow => allowed_tools.push(tool),
                    ToolVerdict::Ask => needs_approval.push(tool),
                    ToolVerdict::Deny => {
                        denied_results.push((
                            idx,
                            lukan_core::models::tools::ToolResult::error(format!(
                                "Tool '{}' is denied by permission rules.",
                                tool.name
                            )),
                        ));
                    }
                }
            }

            // Handle tools that need approval
            if !needs_approval.is_empty() {
                let approval_requests: Vec<ToolApprovalRequest> = needs_approval
                    .iter()
                    .map(|t| {
                        let tool_meta = self.tools.get(&t.name);
                        ToolApprovalRequest {
                            id: t.id.clone(),
                            name: t.name.clone(),
                            input: t.input.clone(),
                            activity_label: tool_meta.and_then(|tool| tool.activity_label(&t.input)),
                            read_only: tool_meta.map(|tool| tool.is_read_only()),
                            search_hint: tool_meta.and_then(|tool| tool.search_hint().map(|s| s.to_string())),
                        }
                    })
                    .collect();

                // Send approval request to UI
                let _ = event_tx
                    .send(StreamEvent::ApprovalRequired {
                        tools: approval_requests,
                    })
                    .await;

                // Wait for approval response (cancel-aware so abort doesn't hang)
                if let Some(ref mut rx) = self.approval_rx {
                    let response = if let Some(ref token) = cancel {
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => {
                                cancelled = true;
                                None
                            }
                            resp = rx.recv() => resp,
                        }
                    } else {
                        rx.recv().await
                    };

                    match response {
                        Some(ApprovalResponse::Approved { approved_ids }) => {
                            for tool in &needs_approval {
                                if approved_ids.contains(&tool.id) {
                                    allowed_tools.push(tool);
                                } else {
                                    // Find original index for denied
                                    let idx = pending_tools
                                        .iter()
                                        .position(|t| t.id == tool.id)
                                        .unwrap_or(0);
                                    denied_results.push((
                                        idx,
                                        lukan_core::models::tools::ToolResult::error(
                                            "Tool denied by user.".to_string(),
                                        ),
                                    ));
                                }
                            }
                        }
                        Some(ApprovalResponse::AlwaysAllow {
                            approved_ids,
                            tools,
                        }) => {
                            // Same approval logic as Approved
                            for tool in &needs_approval {
                                if approved_ids.contains(&tool.id) {
                                    allowed_tools.push(tool);
                                } else {
                                    let idx = pending_tools
                                        .iter()
                                        .position(|t| t.id == tool.id)
                                        .unwrap_or(0);
                                    denied_results.push((
                                        idx,
                                        lukan_core::models::tools::ToolResult::error(
                                            "Tool denied by user.".to_string(),
                                        ),
                                    ));
                                }
                            }
                            // Generate and persist allow rules
                            let cwd = self.cwd.clone();
                            let mut patterns = Vec::new();
                            for tool_req in &tools {
                                let pattern =
                                    generate_allow_pattern(&tool_req.name, &tool_req.input);
                                self.permission_matcher.add_allow_rule(&pattern);
                                if let Err(e) = ProjectConfig::add_allow_rule(&cwd, &pattern).await
                                {
                                    warn!(error = %e, pattern, "Failed to persist allow rule");
                                }
                                patterns.push(pattern);
                            }
                            info!(rules = patterns.join(", "), "Persisted always-allow rules");
                        }
                        Some(ApprovalResponse::DeniedAll) | None => {
                            // Deny all pending tools
                            for tool in &needs_approval {
                                let idx = pending_tools
                                    .iter()
                                    .position(|t| t.id == tool.id)
                                    .unwrap_or(0);
                                denied_results.push((
                                    idx,
                                    lukan_core::models::tools::ToolResult::error(
                                        "Tool denied by user.".to_string(),
                                    ),
                                ));
                            }
                        }
                    }
                } else {
                    // No approval channel (backward compat) — auto-approve all
                    for tool in &needs_approval {
                        allowed_tools.push(tool);
                    }
                }
            }

            // Execute only the approved tool calls
            debug!(
                allowed = allowed_tools.len(),
                denied = denied_results.len(),
                "Executing tool calls after permission check"
            );

            let executed_results = if allowed_tools.is_empty() {
                vec![]
            } else {
                self.execute_tools(&allowed_tools, &event_tx, cancel.as_ref())
                    .await
            };

            // Merge executed + denied results in original order
            let tool_results: Vec<lukan_core::models::tools::ToolResult> = {
                let mut merged: Vec<(usize, lukan_core::models::tools::ToolResult)> = Vec::new();

                // Map allowed tools back to their original indices
                let mut exec_iter = executed_results.into_iter();
                for tool in &allowed_tools {
                    let idx = pending_tools
                        .iter()
                        .position(|t| t.id == tool.id)
                        .unwrap_or(0);
                    if let Some(result) = exec_iter.next() {
                        merged.push((idx, result));
                    }
                }

                merged.extend(denied_results);
                merged.sort_by_key(|(idx, _)| *idx);
                merged.into_iter().map(|(_, r)| r).collect()
            };

            // Add tool results to history and forward events
            let mut result_blocks = Vec::new();
            for (tool, result) in pending_tools.iter().zip(tool_results.iter()) {
                // Redact blocked env var values from tool output
                let content = lukan_tools::redact_env_vars(&result.content, &self.blocked_env_vars);

                result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: content.clone(),
                    is_error: if result.is_error { Some(true) } else { None },
                    diff: result.diff.clone(),
                    image: result.image.clone(),
                });

                // Send ToolResult event to TUI (also redacted)
                let after_content = result.snapshot.as_ref().and_then(|s| s.after.clone());
                let _ = event_tx
                    .send(StreamEvent::ToolResult {
                        id: tool.id.clone(),
                        name: tool.name.clone(),
                        content,
                        is_error: if result.is_error { Some(true) } else { None },
                        diff: result.diff.clone(),
                        image: result.image.clone(),
                        after_content,
                    })
                    .await;

                // Emit updated task list after task tool calls
                if matches!(tool.name.as_str(), "TaskAdd" | "TaskUpdate") && !result.is_error {
                    let all_tasks = lukan_tools::tasks::read_all_tasks(&self.cwd).await;
                    let _ = event_tx
                        .send(StreamEvent::TasksUpdate {
                            tasks: all_tasks
                                .iter()
                                .map(|t| TaskInfo {
                                    id: t.id,
                                    title: t.title.clone(),
                                    status: t.status.label().to_string(),
                                })
                                .collect(),
                        })
                        .await;
                }

                // Track loaded skills for prompt injection
                if tool.name == "LoadSkill"
                    && !result.is_error
                    && let Some(name) = tool.input.get("name").and_then(|v| v.as_str())
                {
                    self.loaded_skills.insert(name.to_string());
                    self.rebuild_system_prompt().await;
                }

                // Collect file snapshots for checkpoint
                if let Some(snapshot) = result.snapshot.clone() {
                    turn_snapshots.push(snapshot);
                }
            }

            self.history.add(Message {
                role: Role::User,
                content: MessageContent::Blocks(result_blocks),
                tool_call_id: None,
                name: None,
            });

            // Incremental save: persist after tool results
            self.save_session().await?;

            // Inject queued messages after tool results so the LLM sees them
            if let Some(ref queue) = queued {
                let messages: Vec<String> = queue.lock().unwrap().drain(..).collect();
                if !messages.is_empty() {
                    let injected = messages.join("\n");
                    let _ = event_tx
                        .send(StreamEvent::QueuedMessageInjected {
                            text: injected.clone(),
                        })
                        .await;
                    self.history.add_user_message(&injected);
                }
            }

            // If cancel fired during approval waiting, stop the turn now
            if cancelled {
                info!("Turn cancelled during tool approval");
                self.save_session().await?;
                return Ok(());
            }

            // Loop continues — LLM will see the tool results and decide next action
        }

        // Create checkpoint if any files were modified during this turn
        if !turn_snapshots.is_empty() {
            let id = {
                let bytes: [u8; 3] = rand::rng().random();
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            };
            let checkpoint = Checkpoint {
                id,
                message: user_message.to_string(),
                snapshots: turn_snapshots,
                created_at: Utc::now(),
                message_index: message_index_before,
            };
            self.session.checkpoints.push(checkpoint);
        }

        // Auto-save session after each turn
        self.save_session().await?;

        // Check if we need compaction or memory update (non-blocking)
        self.check_auto_ops(&event_tx).await;

        Ok(())
    }

    /// Intercept PlannerQuestion and SubmitPlan tool calls.
    /// Removes intercepted calls from `pending_tools`, handles them,
    /// and injects tool results into history.
    /// Returns `true` if any tools were intercepted.
    async fn intercept_planner_tools(
        &mut self,
        pending_tools: &mut Vec<PendingToolCall>,
        event_tx: &mpsc::Sender<StreamEvent>,
    ) -> bool {
        let mut intercepted = false;

        // Process PlannerQuestion calls
        let mut i = 0;
        while i < pending_tools.len() {
            if pending_tools[i].name == "PlannerQuestion" {
                let tool = pending_tools.remove(i);
                intercepted = true;

                // Parse questions from input
                let questions: Vec<PlannerQuestionItem> = tool
                    .input
                    .get("questions")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                if questions.is_empty() {
                    // Invalid input — return error
                    self.history.add(Message {
                        role: Role::User,
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: tool.id.clone(),
                            content: "No questions provided.".to_string(),
                            is_error: Some(true),
                            diff: None,
                            image: None,
                        }]),
                        tool_call_id: None,
                        name: None,
                    });
                    let _ = event_tx
                        .send(StreamEvent::ToolResult {
                            id: tool.id,
                            name: "PlannerQuestion".to_string(),
                            content: "No questions provided.".to_string(),
                            is_error: Some(true),
                            diff: None,
                            image: None,
                            after_content: None,
                        })
                        .await;
                    continue;
                }

                // Send event to UI
                let _ = event_tx
                    .send(StreamEvent::PlannerQuestion {
                        id: tool.id.clone(),
                        questions,
                    })
                    .await;

                // Wait for user answer
                let answer = if let Some(ref mut rx) = self.planner_answer_rx {
                    rx.recv()
                        .await
                        .unwrap_or_else(|| "No answer provided.".to_string())
                } else {
                    "PlannerQuestion channel not connected.".to_string()
                };

                // Inject answer as tool result
                self.history.add(Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: tool.id.clone(),
                        content: answer.clone(),
                        is_error: None,
                        diff: None,
                        image: None,
                    }]),
                    tool_call_id: None,
                    name: None,
                });
                let _ = event_tx
                    .send(StreamEvent::ToolResult {
                        id: tool.id,
                        name: "PlannerQuestion".to_string(),
                        content: answer,
                        is_error: None,
                        diff: None,
                        image: None,
                        after_content: None,
                    })
                    .await;
            } else {
                i += 1;
            }
        }

        // Process SubmitPlan calls
        let mut i = 0;
        while i < pending_tools.len() {
            if pending_tools[i].name == "SubmitPlan" {
                let mut tool = pending_tools.remove(i);
                intercepted = true;

                // Normalize input (accept "description" as alias for "detail")
                lukan_tools::planner_tools::normalize_submit_plan_input(&mut tool.input);

                // Parse plan data
                let title = tool
                    .input
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled Plan")
                    .to_string();
                let plan = tool
                    .input
                    .get("plan")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tasks: Vec<PlanTask> = tool
                    .input
                    .get("tasks")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                if tasks.is_empty() {
                    self.history.add(Message {
                        role: Role::User,
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: tool.id.clone(),
                            content: "Plan must include at least one task.".to_string(),
                            is_error: Some(true),
                            diff: None,
                            image: None,
                        }]),
                        tool_call_id: None,
                        name: None,
                    });
                    let _ = event_tx
                        .send(StreamEvent::ToolResult {
                            id: tool.id,
                            name: "SubmitPlan".to_string(),
                            content: "Plan must include at least one task.".to_string(),
                            is_error: Some(true),
                            diff: None,
                            image: None,
                            after_content: None,
                        })
                        .await;
                    continue;
                }

                // Send plan review event to UI
                let _ = event_tx
                    .send(StreamEvent::PlanReview {
                        id: tool.id.clone(),
                        title: title.clone(),
                        plan: plan.clone(),
                        tasks: tasks.clone(),
                    })
                    .await;

                // Wait for user review
                let response = if let Some(ref mut rx) = self.plan_review_rx {
                    rx.recv().await
                } else {
                    None
                };

                match response {
                    Some(PlanReviewResponse::Accepted { modified_tasks }) => {
                        let final_tasks = modified_tasks.unwrap_or(tasks);

                        // Save plan to disk
                        let cwd = self.cwd.clone();
                        match ProjectConfig::save_plan(&cwd, &title, &plan).await {
                            Ok(filename) => {
                                self.current_plan_file = Some(filename);
                                self.current_plan_content = Some(plan.clone());
                            }
                            Err(e) => {
                                warn!(error = %e, "Failed to save plan file");
                                self.current_plan_content = Some(plan.clone());
                            }
                        }

                        // Create tasks from plan
                        let task_titles: Vec<String> =
                            final_tasks.iter().map(|t| t.title.clone()).collect();
                        lukan_tools::tasks::create_tasks_from_plan(&cwd, &task_titles).await;

                        // Emit task list to frontend
                        let all_tasks = lukan_tools::tasks::read_all_tasks(&cwd).await;
                        let _ = event_tx
                            .send(StreamEvent::TasksUpdate {
                                tasks: all_tasks
                                    .iter()
                                    .map(|t| TaskInfo {
                                        id: t.id,
                                        title: t.title.clone(),
                                        status: t.status.label().to_string(),
                                    })
                                    .collect(),
                            })
                            .await;

                        // Switch from Planner to Auto so the agent can implement.
                        // Other modes (Auto, Skip, Manual) stay unchanged.
                        if self.permission_matcher.mode() == PermissionMode::Planner {
                            self.permission_matcher.set_mode(PermissionMode::Auto);
                            let _ = event_tx
                                .send(StreamEvent::ModeChanged {
                                    mode: "Auto".to_string(),
                                })
                                .await;
                        }
                        // Rebuild system prompt with latest tasks + plan context.
                        self.rebuild_system_prompt().await;

                        // Inject success tool result with instructions
                        let task_summary: String = task_titles
                            .iter()
                            .enumerate()
                            .map(|(i, t)| format!("  {}. {}", i + 1, t))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let result_content = format!(
                            "Plan accepted! {} tasks created.\n\n\
                             Tasks:\n{}\n\n\
                             Start implementing task #1 now. Use TaskUpdate to mark tasks in_progress/done as you go.",
                            task_titles.len(),
                            task_summary
                        );

                        self.history.add(Message {
                            role: Role::User,
                            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: tool.id.clone(),
                                content: result_content.clone(),
                                is_error: None,
                                diff: None,
                                image: None,
                            }]),
                            tool_call_id: None,
                            name: None,
                        });
                        let _ = event_tx
                            .send(StreamEvent::ToolResult {
                                id: tool.id,
                                name: "SubmitPlan".to_string(),
                                content: result_content,
                                is_error: None,
                                diff: None,
                                image: None,
                                after_content: None,
                            })
                            .await;
                    }
                    Some(PlanReviewResponse::Rejected { feedback }) => {
                        let result_content = format!(
                            "Plan rejected by user. Feedback: {feedback}\n\n\
                             Please revise the plan based on this feedback and resubmit."
                        );
                        self.history.add(Message {
                            role: Role::User,
                            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: tool.id.clone(),
                                content: result_content.clone(),
                                is_error: Some(true),
                                diff: None,
                                image: None,
                            }]),
                            tool_call_id: None,
                            name: None,
                        });
                        let _ = event_tx
                            .send(StreamEvent::ToolResult {
                                id: tool.id,
                                name: "SubmitPlan".to_string(),
                                content: result_content,
                                is_error: Some(true),
                                diff: None,
                                image: None,
                                after_content: None,
                            })
                            .await;
                    }
                    Some(PlanReviewResponse::TaskFeedback {
                        task_index,
                        feedback,
                    }) => {
                        let result_content = format!(
                            "User wants changes to task #{}: {feedback}\n\n\
                             Please revise this task and resubmit the plan.",
                            task_index + 1
                        );
                        self.history.add(Message {
                            role: Role::User,
                            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: tool.id.clone(),
                                content: result_content.clone(),
                                is_error: Some(true),
                                diff: None,
                                image: None,
                            }]),
                            tool_call_id: None,
                            name: None,
                        });
                        let _ = event_tx
                            .send(StreamEvent::ToolResult {
                                id: tool.id,
                                name: "SubmitPlan".to_string(),
                                content: result_content,
                                is_error: Some(true),
                                diff: None,
                                image: None,
                                after_content: None,
                            })
                            .await;
                    }
                    None => {
                        // Channel closed — treat as rejection
                        let result_content = "Plan review cancelled (channel closed).".to_string();
                        self.history.add(Message {
                            role: Role::User,
                            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: tool.id.clone(),
                                content: result_content.clone(),
                                is_error: Some(true),
                                diff: None,
                                image: None,
                            }]),
                            tool_call_id: None,
                            name: None,
                        });
                        let _ = event_tx
                            .send(StreamEvent::ToolResult {
                                id: tool.id,
                                name: "SubmitPlan".to_string(),
                                content: result_content,
                                is_error: Some(true),
                                diff: None,
                                image: None,
                                after_content: None,
                            })
                            .await;
                    }
                }
            } else {
                i += 1;
            }
        }

        intercepted
    }

    /// Save current state to disk
    async fn save_session(&mut self) -> Result<()> {
        if self.skip_session_save {
            return Ok(());
        }
        self.session.messages = self.history.to_json();
        self.session.total_input_tokens = self.input_tokens;
        self.session.total_output_tokens = self.output_tokens;
        self.session.last_context_size = self.last_context_size;
        self.session.last_memory_update_tokens = self.last_memory_update_tokens;
        self.session.updated_at = Utc::now();
        SessionManager::save(&mut self.session).await
    }

    // ── Auto Operations ───────────────────────────────────────────────────

    /// Check if compaction or memory update is needed
    async fn check_auto_ops(&mut self, event_tx: &mpsc::Sender<StreamEvent>) {
        let ctx = self.last_context_size;

        // Auto-compaction when context exceeds threshold
        if ctx >= self.compaction_threshold {
            if let Err(e) = self.compact_history(event_tx).await {
                error!("Compaction failed: {e}");
            }
            // Also trigger memory update after compaction
            if let Err(e) = self.update_memory().await {
                error!("Memory update after compaction failed: {e}");
            }
            return;
        }

        // Memory update when context window reaches threshold
        // Reset stale tracker if it's from old system (was tracking total tokens, not ctx)
        if self.last_memory_update_tokens > ctx {
            self.last_memory_update_tokens = 0;
        }
        if ctx >= MEMORY_UPDATE_THRESHOLD
            && ctx.saturating_sub(self.last_memory_update_tokens) >= MEMORY_UPDATE_THRESHOLD
            && let Err(e) = self.update_memory().await
        {
            error!("Memory update failed: {e}");
        }
    }

    // ── Compaction ────────────────────────────────────────────────────────

    /// Compact history: summarize old messages, keep last N
    async fn compact_history(&mut self, event_tx: &mpsc::Sender<StreamEvent>) -> Result<()> {
        let messages = self.history.messages();
        if messages.len() <= COMPACTION_KEEP_MESSAGES {
            return Ok(());
        }

        // Notify TUI
        let _ = event_tx
            .send(StreamEvent::ToolProgress {
                id: String::new(),
                name: "system".to_string(),
                content: "Compacting conversation...".to_string(),
            })
            .await;

        let msg_count_before = messages.len();
        let mut split = messages.len() - COMPACTION_KEEP_MESSAGES;

        // Adjust split point: don't cut between a tool_use and its tool_result.
        // If the first "recent" message contains a ToolResult whose ToolUse is in "old",
        // move the split earlier to include the ToolUse message.
        while split > 0 {
            let has_orphan_tool_result = messages[split..]
                .iter()
                .filter(|m| m.role == Role::User)
                .flat_map(|m| match &m.content {
                    MessageContent::Blocks(blocks) => blocks.clone(),
                    _ => vec![],
                })
                .any(|block| {
                    if let ContentBlock::ToolResult { tool_use_id, .. } = &block {
                        // Check if the corresponding ToolUse is NOT in the recent messages
                        !messages[split..].iter().any(|m| {
                            match &m.content {
                            MessageContent::Blocks(blocks) => blocks.iter().any(|b| {
                                matches!(b, ContentBlock::ToolUse { id, .. } if id == tool_use_id)
                            }),
                            _ => false,
                        }
                        })
                    } else {
                        false
                    }
                });

            if !has_orphan_tool_result {
                break;
            }
            split -= 1;
        }

        let old_messages = &messages[..split];
        let recent_messages = &messages[split..];

        let old_context = format_messages_for_context(old_messages);
        let recent_context = format_messages_for_context(recent_messages);

        let full_context = format!(
            "--- OLDER MESSAGES (to be summarized) ---\n{old_context}\n\n\
             --- RECENT MESSAGES (still in context, shown for reference) ---\n{recent_context}"
        );

        // Check if an active memory file exists (project or global)
        let memory_path = active_memory_path();

        let summary;

        if let Some(ref mem_path) = memory_path {
            let current_memory = tokio::fs::read_to_string(mem_path)
                .await
                .unwrap_or_else(|_| "# Memory\n\n".to_string());

            let user_prompt = format!(
                "Current MEMORY.md:\n```\n{current_memory}\n```\n\nConversation to summarize:\n{full_context}"
            );

            let result = self
                .call_llm_for_memory(COMPACTION_WITH_MEMORY_PROMPT, &user_prompt)
                .await?;

            // Parse ---SUMMARY--- and ---MEMORY--- sections
            let summary_match = extract_section(&result, "---SUMMARY---", "---MEMORY---");
            let memory_match = extract_section(&result, "---MEMORY---", "");

            summary = summary_match
                .unwrap_or_else(|| "Previous conversation context was compacted.".to_string());

            // Memory update is handled separately by update_memory() after compaction
            let _ = memory_match;
        } else {
            summary = self
                .call_llm_for_memory(COMPACTION_SIMPLE_PROMPT, &full_context)
                .await
                .unwrap_or_else(|_| "Previous conversation context was compacted.".to_string());
        }

        // Rebuild history: compaction message + recent messages
        let compaction_msg = format!(
            "[System: Conversation was auto-compacted. Below is a summary of earlier context. \
             Continue working from where you left off — check the \"Active Task\" section for what you were doing.]\n\n\
             {summary}"
        );

        let recent = self.history.messages()[split..].to_vec();
        self.history.clear();
        self.history.add_user_message(&compaction_msg);
        for msg in recent {
            self.history.add(msg);
        }

        // Reset token counters
        self.session.compaction_count += 1;
        self.session.compaction_summary = Some(summary);
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.last_context_size = 0;
        self.last_memory_update_tokens = 0;

        self.save_session().await?;

        let msg_count_after = self.history.messages().len();
        info!(
            before = msg_count_before,
            after = msg_count_after,
            "Compacted history"
        );

        let _ = event_tx
            .send(StreamEvent::ToolProgress {
                id: String::new(),
                name: "system".to_string(),
                content: format!("Compacted: {msg_count_before} msgs → {msg_count_after} msgs."),
            })
            .await;

        Ok(())
    }

    // ── Memory Update ─────────────────────────────────────────────────────

    /// Update project memories with insights from the conversation.
    /// Uses structured memory files when available, falls back to behavior profile MEMORY.md.
    async fn update_memory(&mut self) -> Result<()> {
        let _active = active_memory_path();
        let cwd = self.cwd.clone();

        // Check for structured memory files
        let memory_files = lukan_tools::remember::discover_memory_files(&cwd).await;
        let has_profile = lukan_tools::remember::has_behavior_profile(&cwd).await;

        // Always run memory update — create .lukan/memories/ if needed
        {
            let messages = self.history.messages().to_vec();
            let context = format_messages_for_context(&messages);
            let frontmatter_listing =
                lukan_tools::remember::format_frontmatters_for_llm(&memory_files);
            let profile_content = if has_profile {
                lukan_tools::remember::read_behavior_profile(&cwd).await
            } else {
                None
            };
            let provider = std::sync::Arc::clone(&self.provider);

            // Fire-and-forget background task
            tokio::spawn(async move {
                if let Err(e) = run_structured_memory_update(
                    provider,
                    &cwd,
                    &context,
                    &frontmatter_listing,
                    profile_content.as_deref(),
                )
                .await
                {
                    tracing::error!("Background memory update failed: {e}");
                }
            });
        }

        self.last_memory_update_tokens = self.last_context_size;
        Ok(())
    }

    // ── LLM Helper ────────────────────────────────────────────────────────

    /// Make a simple LLM call for compaction/memory (no tools, no streaming to UI)
    async fn call_llm_for_memory(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        let params = StreamParams {
            system_prompt: SystemPrompt::Text(system_prompt.to_string()),
            messages: vec![Message::user(user_message)],
            tools: vec![],
        };

        let (tx, mut rx) = mpsc::channel::<StreamEvent>(256);
        let provider = Arc::clone(&self.provider);

        tokio::spawn(async move {
            if let Err(e) = provider.stream(params, tx).await {
                error!("Memory LLM call error: {e}");
            }
        });

        let mut result = String::new();
        while let Some(event) = rx.recv().await {
            if let StreamEvent::TextDelta { text } = event {
                result.push_str(&text);
            }
        }

        Ok(result)
    }

    /// Execute multiple tool calls in parallel
    async fn execute_tools(
        &self,
        tools: &[&PendingToolCall],
        event_tx: &mpsc::Sender<StreamEvent>,
        cancel: Option<&CancellationToken>,
    ) -> Vec<lukan_core::models::tools::ToolResult> {
        let mut handles = Vec::new();

        // Build sandbox config from registry settings
        let sandbox_cfg = if self.tools.is_sandbox_enabled() {
            Some(lukan_tools::sandbox::SandboxConfig {
                enabled: true,
                allowed_dirs: self.tools.allowed_dirs().to_vec(),
                sensitive_patterns: self.tools.sensitive_patterns().to_vec(),
            })
        } else {
            None
        };

        for tool_call in tools {
            let registry = Arc::clone(&self.tools);
            let read_files = Arc::clone(&self.read_files);
            let cwd = self.cwd.clone();
            let name = tool_call.name.clone();
            let id = tool_call.id.clone();
            let input = tool_call.input.clone();
            let tx = event_tx.clone();
            let bg_signal = self.bg_signal.clone();
            let sandbox_cfg = sandbox_cfg.clone();
            let allowed_paths = self.allowed_paths.clone();
            let cancel_token = cancel.cloned();
            let session_id = Some(self.session.id.clone());
            let extra_env = self.extra_env.clone();
            let agent_label = self.label.clone();
            let tab_id = self.tab_id.clone();

            handles.push(tokio::spawn(async move {
                // Send progress start
                let _ = tx
                    .send(StreamEvent::ToolProgress {
                        id: id.clone(),
                        name: name.clone(),
                        content: format!("Running {name}..."),
                    })
                    .await;

                let ctx = ToolContext {
                    progress_tx: None,
                    event_tx: Some(tx.clone()),
                    tool_call_id: Some(id.clone()),
                    read_files,
                    cwd,
                    bg_signal,
                    sandbox: sandbox_cfg.clone(),
                    allowed_paths,
                    cancel: cancel_token,
                    session_id,
                    extra_env,
                    agent_label,
                    tab_id,
                    blocked_env_vars: Vec::new(),
                };

                match registry.execute(&name, input, &ctx).await {
                    Ok(result) => result,
                    Err(e) => {
                        error!(tool = name, error = %e, "Tool execution failed");
                        lukan_core::models::tools::ToolResult::error(format!(
                            "Tool execution error: {e}"
                        ))
                    }
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) if e.is_cancelled() => {
                    results.push(lukan_core::models::tools::ToolResult::error(
                        "Cancelled by user.".to_string(),
                    ));
                }
                Err(e) => {
                    results.push(lukan_core::models::tools::ToolResult::error(format!(
                        "Task join error: {e}"
                    )));
                }
            }
        }
        results
    }
}

// ── Structured Memory Update (runs in background) ────────────────────────

async fn run_structured_memory_update(
    provider: Arc<dyn lukan_providers::Provider>,
    cwd: &std::path::Path,
    conversation_context: &str,
    frontmatter_listing: &str,
    profile_content: Option<&str>,
) -> Result<()> {
    use lukan_core::models::events::StreamEvent;
    use lukan_core::models::messages::Message;
    use lukan_providers::{StreamParams, SystemPrompt};

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let user_prompt = if let Some(profile) = profile_content {
        format!(
            "Today: {today}\n\n\
             Existing structured memory files:\n{frontmatter_listing}\n\n\
             Behavior profile MEMORY.md (migrate useful content into structured files):\n```\n{profile}\n```\n\n\
             Recent conversation:\n{conversation_context}"
        )
    } else {
        format!(
            "Today: {today}\n\n\
             Existing structured memory files:\n{frontmatter_listing}\n\n\
             Recent conversation:\n{conversation_context}"
        )
    };

    let params = StreamParams {
        system_prompt: SystemPrompt::Text(STRUCTURED_MEMORY_UPDATE_PROMPT.to_string()),
        messages: vec![Message::user(&user_prompt)],
        tools: vec![],
    };

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(256);
    let prov = Arc::clone(&provider);
    tokio::spawn(async move {
        if let Err(e) = prov.stream(params, tx).await {
            tracing::error!("Structured memory LLM call error: {e}");
        }
    });

    let mut response = String::new();
    while let Some(event) = rx.recv().await {
        if let StreamEvent::TextDelta { text } = event {
            response.push_str(&text);
        }
    }

    // Parse action blocks from response
    let memories_dir = cwd.join(".lukan").join("memories");
    tokio::fs::create_dir_all(&memories_dir).await.ok();

    let mut pos = 0;
    while let Some(action_start) = response[pos..].find("---ACTION:") {
        let abs_start = pos + action_start;
        let action_line_end = response[abs_start..]
            .find("---\n")
            .map(|i| abs_start + i + 4);
        let Some(action_line_end) = action_line_end else {
            break;
        };

        let action_line = response[abs_start..action_line_end].trim().to_string();

        if action_line.contains("NOTHING") {
            tracing::info!("Structured memory update: no changes needed");
            break;
        }

        // Find content between ---CONTENT--- and ---END---
        let content_start = response[action_line_end..]
            .find("---CONTENT---")
            .map(|i| action_line_end + i + "---CONTENT---".len());
        let content_end = response[action_line_end..]
            .find("---END---")
            .map(|i| action_line_end + i);

        if let (Some(cs), Some(ce)) = (content_start, content_end) {
            let content = response[cs..ce].trim();

            // Extract filename
            let filename = if let Some(fname_line) = response[abs_start..action_line_end]
                .lines()
                .chain(response[action_line_end..cs].lines())
                .find(|l| l.starts_with("filename:"))
            {
                fname_line
                    .strip_prefix("filename:")
                    .unwrap_or("")
                    .trim()
                    .to_string()
            } else {
                // Try next line after action
                response[action_line_end..cs]
                    .lines()
                    .find(|l| l.starts_with("filename:"))
                    .and_then(|l| l.strip_prefix("filename:"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            };

            if !filename.is_empty() && !content.is_empty() {
                let file_path = memories_dir.join(&filename);
                if action_line.contains("CREATE") {
                    tracing::info!(filename, "Creating structured memory file");
                    tokio::fs::write(&file_path, content).await.ok();
                } else if action_line.contains("UPDATE") {
                    tracing::info!(filename, "Updating structured memory file");
                    tokio::fs::write(&file_path, content).await.ok();
                }
            }

            pos = ce + "---END---".len();
        } else {
            break;
        }
    }

    // If behavior profile was provided and we created structured files, back it up
    if profile_content.is_some() && response.contains("CREATE") {
        let profile_path = memories_dir.join("MEMORY.md");
        let backup_path = memories_dir.join("MEMORY.md.bak");
        // Read current content before renaming (we'll use it as seed for behavior profile)
        let old_content = tokio::fs::read_to_string(&profile_path)
            .await
            .unwrap_or_default();
        tokio::fs::rename(&profile_path, &backup_path).await.ok();
        tracing::info!("Behavior profile MEMORY.md migrated to MEMORY.md.bak");
        // Write a fresh behavior profile seeded from old content
        if !old_content.trim().is_empty() {
            let header = "# Behavior Profile\n\n";
            tokio::fs::write(&profile_path, format!("{header}{old_content}"))
                .await
                .ok();
        }
    }

    // Update MEMORY.md as behavior profile
    if let Err(e) = update_behavior_profile(Arc::clone(&provider), cwd, conversation_context).await
    {
        tracing::error!("Behavior profile update failed: {e}");
    }

    Ok(())
}

/// Update MEMORY.md as a behavior profile using a full agent turn with tool access.
async fn update_behavior_profile(
    provider: Arc<dyn lukan_providers::Provider>,
    cwd: &std::path::Path,
    conversation_context: &str,
) -> Result<()> {
    use lukan_core::models::events::StreamEvent;
    use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};
    use lukan_providers::{StreamParams, SystemPrompt};

    let memory_path = cwd.join(".lukan").join("memories").join("MEMORY.md");
    let current = tokio::fs::read_to_string(&memory_path)
        .await
        .unwrap_or_else(|_| "(empty)\n".to_string());
    // Strip duplicate title if present
    let current = current
        .trim_start_matches("# Behavior Profile")
        .trim_start()
        .to_string();

    // Build tools registry for the sub-agent
    let tools = lukan_tools::create_default_registry();
    let tool_defs = tools.definitions();

    let read_files: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<std::path::PathBuf, Option<std::time::SystemTime>>,
        >,
    > = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let system_prompt = format!(
        "{}\n\nYou have access to tools (Glob, ReadFiles, Bash, Grep, Remember) to explore the project.\n\
         The project is at: {}\n\
         Write the updated MEMORY.md content using the WriteFile tool at: {}\n\
         If the current profile is empty or outdated, explore the project structure first.\n\n\
         MANDATORY: The MEMORY.md MUST end with a '## Project Structure' section containing the project's \
         directory tree (2 levels deep, excluding node_modules, target, .git, dist, build, __pycache__, .venv). \
         Use Bash with 'find' or 'ls -R' to get the real tree. This section prevents path errors.\n\n\
         DO NOT include '# Behavior Profile' as a title — it is added automatically by the system. \
         Start directly with the first ## section.",
        BEHAVIOR_MEMORY_UPDATE_PROMPT,
        cwd.display(),
        memory_path.display(),
    );

    let user_message = format!(
        "Current behavior profile (MEMORY.md):\n```\n{current}\n```\n\n\
         Recent conversation:\n{conversation_context}\n\n\
         Analyze the project and conversation. Update the behavior profile at {}",
        memory_path.display(),
    );

    let mut messages = vec![Message::user(&user_message)];
    let max_turns = 10;

    for _turn in 0..max_turns {
        let params = StreamParams {
            system_prompt: SystemPrompt::Text(system_prompt.clone()),
            messages: messages.clone(),
            tools: tool_defs.clone(),
        };

        let (tx, mut rx) = mpsc::channel::<StreamEvent>(256);
        let prov = Arc::clone(&provider);
        tokio::spawn(async move {
            if let Err(e) = prov.stream(params, tx).await {
                tracing::error!("Behavior sub-agent LLM error: {e}");
            }
        });

        let mut text = String::new();
        let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input = String::new();

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta { text: t } => text.push_str(&t),
                StreamEvent::ToolUseStart { id, name } => {
                    current_tool_id = id;
                    current_tool_name = name;
                    current_tool_input.clear();
                }
                StreamEvent::ToolUseDelta { input } => {
                    current_tool_input.push_str(&input);
                }
                StreamEvent::ToolUseEnd { .. } => {
                    let input: serde_json::Value =
                        serde_json::from_str(&current_tool_input).unwrap_or_default();
                    tool_calls.push((current_tool_id.clone(), current_tool_name.clone(), input));
                }
                StreamEvent::MessageEnd { .. } => break,
                _ => {}
            }
        }

        if tool_calls.is_empty() {
            // No tool calls — agent is done
            tracing::info!("Behavior sub-agent completed");
            break;
        }

        // Build assistant message with tool use blocks
        let mut blocks: Vec<ContentBlock> = Vec::new();
        if !text.is_empty() {
            blocks.push(ContentBlock::Text { text: text.clone() });
        }
        for (id, name, input) in &tool_calls {
            blocks.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }
        messages.push(Message {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
            tool_call_id: None,
            name: None,
        });

        // Execute tools and collect results
        let mut result_blocks: Vec<ContentBlock> = Vec::new();
        for (id, name, input) in &tool_calls {
            let ctx = lukan_tools::ToolContext {
                progress_tx: None,
                event_tx: None,
                tool_call_id: Some(id.clone()),
                read_files: std::sync::Arc::clone(&read_files),
                cwd: cwd.to_path_buf(),
                bg_signal: None,
                sandbox: None,
                allowed_paths: Some(vec![cwd.to_path_buf()]),
                cancel: None,
                session_id: Some(String::new()),
                extra_env: std::collections::HashMap::new(),
                agent_label: None,
                tab_id: None,
                blocked_env_vars: vec![],
            };
            let result = if let Some(tool) = tools.get(name) {
                match tool.execute(input.clone(), &ctx).await {
                    Ok(r) => r,
                    Err(e) => lukan_core::models::tools::ToolResult::error(e.to_string()),
                }
            } else {
                lukan_core::models::tools::ToolResult::error(format!("Tool '{name}' not found"))
            };

            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content: result.content,
                is_error: Some(result.is_error),
                diff: None,
                image: None,
            });
        }
        messages.push(Message {
            role: Role::User,
            content: MessageContent::Blocks(result_blocks),
            tool_call_id: None,
            name: None,
        });
    }

    tracing::info!("Behavior profile update completed");
    Ok(())
}
