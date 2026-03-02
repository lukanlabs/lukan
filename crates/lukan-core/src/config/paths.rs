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

    /// Base data directory: ~/.local/share/lukan/
    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("lukan")
    }

    /// Generic plugin data directory: ~/.local/share/lukan/plugins/{name}/
    /// Used for plugin-specific persistent data (auth credentials, caches, etc.)
    pub fn plugin_data_dir(name: &str) -> PathBuf {
        let new_path = Self::data_dir().join("plugins").join(name);

        // Auto-migrate legacy whatsapp-auth dir → plugins/whatsapp/
        if name == "whatsapp" && !new_path.exists() {
            let old_path = Self::data_dir().join("whatsapp-auth");
            if old_path.exists() {
                if let Some(parent) = new_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::rename(&old_path, &new_path);
            }
        }

        new_path
    }

    /// Plugins directory: ~/.config/lukan/plugins/
    pub fn plugins_dir() -> PathBuf {
        Self::config_dir().join("plugins")
    }

    /// Plugin directory: ~/.config/lukan/plugins/<name>/
    pub fn plugin_dir(name: &str) -> PathBuf {
        Self::plugins_dir().join(name)
    }

    /// Plugin manifest: ~/.config/lukan/plugins/<name>/plugin.toml
    pub fn plugin_manifest(name: &str) -> PathBuf {
        Self::plugin_dir(name).join("plugin.toml")
    }

    /// Plugin config: ~/.config/lukan/plugins/<name>/config.json
    pub fn plugin_config(name: &str) -> PathBuf {
        Self::plugin_dir(name).join("config.json")
    }

    /// Plugin log file: ~/.config/lukan/plugins/<name>/plugin.log
    pub fn plugin_log(name: &str) -> PathBuf {
        Self::plugin_dir(name).join("plugin.log")
    }

    /// Plugin PID file: ~/.config/lukan/plugins/<name>/plugin.pid
    pub fn plugin_pid(name: &str) -> PathBuf {
        Self::plugin_dir(name).join("plugin.pid")
    }

    /// Events directory: ~/.config/lukan/events/
    pub fn events_dir() -> PathBuf {
        Self::config_dir().join("events")
    }

    /// Pending events file: ~/.config/lukan/events/pending.jsonl
    pub fn pending_events_file() -> PathBuf {
        Self::events_dir().join("pending.jsonl")
    }

    /// Event history file: ~/.config/lukan/events/history.jsonl
    pub fn events_history_file() -> PathBuf {
        Self::events_dir().join("history.jsonl")
    }

    /// Views directory: ~/.config/lukan/views/
    pub fn views_dir() -> PathBuf {
        Self::config_dir().join("views")
    }

    /// View data file: ~/.config/lukan/views/<plugin>/<view_id>.json
    pub fn plugin_view_file(plugin: &str, view_id: &str) -> PathBuf {
        Self::views_dir()
            .join(plugin)
            .join(format!("{view_id}.json"))
    }

    /// Worker daemon PID file: ~/.config/lukan/daemon.pid
    pub fn daemon_pid_file() -> PathBuf {
        Self::config_dir().join("daemon.pid")
    }

    /// Worker daemon log file: ~/.config/lukan/daemon.log
    pub fn daemon_log_file() -> PathBuf {
        Self::config_dir().join("daemon.log")
    }

    /// Worker notification file: ~/.config/lukan/worker_notifications.jsonl
    /// The daemon appends one JSON line per worker run completion.
    pub fn worker_notifications_file() -> PathBuf {
        Self::config_dir().join("worker_notifications.jsonl")
    }

    /// Workers definition file: ~/.config/lukan/workers.json
    pub fn workers_file() -> PathBuf {
        Self::config_dir().join("workers.json")
    }

    /// Workers runs directory: ~/.config/lukan/workers/
    pub fn workers_runs_dir() -> PathBuf {
        Self::config_dir().join("workers")
    }

    /// Runs directory for a specific worker: ~/.config/lukan/workers/{id}/
    pub fn worker_runs_dir(id: &str) -> PathBuf {
        Self::workers_runs_dir().join(id)
    }

    /// Run file for a specific worker run: ~/.config/lukan/workers/{id}/{run_id}.json
    pub fn worker_run_file(id: &str, run_id: &str) -> PathBuf {
        Self::worker_runs_dir(id).join(format!("{run_id}.json"))
    }

    /// Ensure all required directories exist
    pub async fn ensure_dirs() -> std::io::Result<()> {
        tokio::fs::create_dir_all(Self::config_dir()).await?;
        tokio::fs::create_dir_all(Self::sessions_dir()).await?;
        tokio::fs::create_dir_all(Self::plugins_dir()).await?;
        tokio::fs::create_dir_all(Self::events_dir()).await?;
        tokio::fs::create_dir_all(Self::workers_runs_dir()).await?;
        Ok(())
    }
}
