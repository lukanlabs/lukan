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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_config() {
        let err = LukanError::Config("bad value".into());
        assert_eq!(err.to_string(), "Configuration error: bad value");
    }

    #[test]
    fn test_error_display_provider() {
        let err = LukanError::Provider("timeout".into());
        assert_eq!(err.to_string(), "Provider error: timeout");
    }

    #[test]
    fn test_error_display_missing_api_key() {
        let err = LukanError::MissingApiKey {
            provider: "anthropic".into(),
            env_var: "ANTHROPIC_API_KEY".into(),
        };
        assert_eq!(
            err.to_string(),
            "Missing API key for provider 'anthropic'. Set it via `lukan setup` or environment variable ANTHROPIC_API_KEY"
        );
    }

    #[test]
    fn test_error_display_tool_execution() {
        let err = LukanError::ToolExecution {
            tool: "Bash".into(),
            message: "command not found".into(),
        };
        assert_eq!(
            err.to_string(),
            "Tool execution error in 'Bash': command not found"
        );
    }

    #[test]
    fn test_error_display_session() {
        let err = LukanError::Session("not found".into());
        assert_eq!(err.to_string(), "Session error: not found");
    }

    #[test]
    fn test_error_display_permission_denied() {
        let err = LukanError::PermissionDenied("write to /etc".into());
        assert_eq!(err.to_string(), "Permission denied: write to /etc");
    }

    #[test]
    fn test_error_display_stream() {
        let err = LukanError::Stream("connection reset".into());
        assert_eq!(err.to_string(), "Stream error: connection reset");
    }

    #[test]
    fn test_error_display_http() {
        let err = LukanError::Http("502 Bad Gateway".into());
        assert_eq!(err.to_string(), "HTTP error: 502 Bad Gateway");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = LukanError::from(io_err);
        assert!(matches!(err, LukanError::Io(_)));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = LukanError::from(json_err);
        assert!(matches!(err, LukanError::Json(_)));
    }
}
