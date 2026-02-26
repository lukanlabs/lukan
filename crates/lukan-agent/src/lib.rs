mod agent_loop;
pub mod message_history;
pub mod permission_matcher;
pub mod session_manager;
pub mod sub_agent;
pub mod whatsapp_channel;
pub mod workers;

pub use agent_loop::{AgentConfig, AgentLoop};
pub use message_history::MessageHistory;
pub use permission_matcher::{PermissionMatcher, ToolVerdict, generate_allow_pattern};
pub use session_manager::SessionManager;
pub use workers::{NotificationWatcher, WorkerNotification, WorkerScheduler};
