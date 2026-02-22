use std::path::PathBuf;

/// XDG-compliant paths for lukan configuration and data
pub struct LukanPaths;

impl LukanPaths {
    /// Base config directory: ~/.config/lukan/
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("lukan")
    }

    /// Main config file: ~/.config/lukan/config.json
    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    /// Credentials file: ~/.config/lukan/credentials.json
    pub fn credentials_file() -> PathBuf {
        Self::config_dir().join("credentials.json")
    }

    /// Sessions directory: ~/.config/lukan/sessions/
    pub fn sessions_dir() -> PathBuf {
        Self::config_dir().join("sessions")
    }

    /// Session file for a given ID
    pub fn session_file(id: &str) -> PathBuf {
        Self::sessions_dir().join(format!("{id}.json"))
    }

    /// Reminders file: ~/.config/lukan/reminders.md
    pub fn reminders_file() -> PathBuf {
        Self::config_dir().join("reminders.md")
    }

    /// Symbol index file: ~/.config/lukan/symbol-index.json
    pub fn symbol_index_file() -> PathBuf {
        Self::config_dir().join("symbol-index.json")
    }

    /// Global memory file: ~/.config/lukan/MEMORY.md
    pub fn global_memory_file() -> PathBuf {
        Self::config_dir().join("MEMORY.md")
    }

    /// Project memory directory: .lukan/memories/
    pub fn project_memory_dir() -> PathBuf {
        PathBuf::from(".lukan/memories")
    }

    /// Project memory file: .lukan/memories/MEMORY.md
    pub fn project_memory_file() -> PathBuf {
        Self::project_memory_dir().join("MEMORY.md")
    }

    /// Project memory active marker: .lukan/memories/.active
    pub fn project_memory_active_file() -> PathBuf {
        Self::project_memory_dir().join(".active")
    }

    /// WhatsApp auth directory: ~/.local/share/lukan/whatsapp-auth/
    pub fn whatsapp_auth_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("lukan")
            .join("whatsapp-auth")
    }

    /// WhatsApp daemon PID file: ~/.config/lukan/whatsapp.pid
    pub fn whatsapp_pid_file() -> PathBuf {
        Self::config_dir().join("whatsapp.pid")
    }

    /// WhatsApp connector PID file: ~/.config/lukan/whatsapp-connector.pid
    pub fn whatsapp_connector_pid_file() -> PathBuf {
        Self::config_dir().join("whatsapp-connector.pid")
    }

    /// WhatsApp log file: ~/.config/lukan/whatsapp.log
    pub fn whatsapp_log_file() -> PathBuf {
        Self::config_dir().join("whatsapp.log")
    }

    /// Ensure all required directories exist
    pub async fn ensure_dirs() -> std::io::Result<()> {
        tokio::fs::create_dir_all(Self::config_dir()).await?;
        tokio::fs::create_dir_all(Self::sessions_dir()).await?;
        Ok(())
    }
}
