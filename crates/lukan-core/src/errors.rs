use thiserror::Error;

#[derive(Error, Debug)]
pub enum LukanError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error(
        "Missing API key for provider '{provider}'. Set it via `lukan setup` or environment variable {env_var}"
    )]
    MissingApiKey { provider: String, env_var: String },

    #[error("Tool execution error in '{tool}': {message}")]
    ToolExecution { tool: String, message: String },

    #[error("Session error: {0}")]
    Session(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(String),
}
