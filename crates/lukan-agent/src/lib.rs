mod agent_loop;
pub mod git_worktree_roots;
pub(crate) mod memory_helpers;
pub mod message_history;
pub mod permission_matcher;
pub mod pipelines;
pub mod session_manager;
pub mod sub_agent;
pub mod subagent_worktree_tools;
pub mod subagent_worktrees;
pub mod vision_preprocessor;
pub mod workers;

pub use agent_loop::{AgentConfig, AgentLoop};
pub use message_history::MessageHistory;
pub use permission_matcher::{PermissionMatcher, ToolVerdict, generate_allow_pattern};
pub use pipelines::{PipelineNotification, PipelineNotificationWatcher, PipelineScheduler};
pub use session_manager::SessionManager;
pub use workers::{NotificationWatcher, WorkerNotification, WorkerScheduler};
