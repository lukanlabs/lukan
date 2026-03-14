use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Messages sent from the daemon to the relay server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DaemonToRelay {
    /// Register this daemon for a user after connecting.
    Register {
        user_id: String,
        device_name: String,
        /// Operating system and architecture (e.g. "Linux x86_64", "macOS arm64")
        #[serde(default)]
        os: Option<String>,
        /// Daemon binary version
        #[serde(default)]
        version: Option<String>,
    },
    /// Forward a server message back to a specific browser connection.
    Forward {
        connection_id: String,
        /// The inner ServerMessage, serialized as JSON value to stay transport-agnostic.
        message: serde_json::Value,
    },
    /// Response to a tunneled REST request from the relay.
    RestResponse {
        request_id: String,
        status: u16,
        headers: HashMap<String, String>,
        #[serde(with = "base64_bytes")]
        body: Vec<u8>,
    },
    /// Heartbeat to keep the connection alive.
    Ping,
}

/// Messages sent from the relay server to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RelayToDaemon {
    /// Forward a browser client message to the daemon for processing.
    Forward {
        connection_id: String,
        /// The inner ClientMessage, serialized as JSON value.
        message: serde_json::Value,
    },
    /// Tunnel a REST request from the browser through to the daemon's local HTTP.
    RestRequest {
        request_id: String,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        #[serde(with = "base64_bytes")]
        body: Vec<u8>,
    },
    /// Notify daemon that a new browser connection was opened for this user.
    ConnectionOpened { connection_id: String },
    /// Notify daemon that a browser connection was closed.
    ConnectionClosed { connection_id: String },
    /// Heartbeat response.
    Pong,
}

/// Relay connection configuration stored at `~/.config/lukan/relay.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConfig {
    pub relay_url: String,
    pub jwt_token: String,
    pub user_id: String,
    pub email: String,
    /// Whether the relay connection is enabled. Defaults to true for backward compat.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl RelayConfig {
    /// Load relay config from the standard path, returning None if it doesn't exist.
    pub async fn load() -> Option<Self> {
        let path = crate::config::LukanPaths::config_dir().join("relay.json");
        let data = tokio::fs::read_to_string(&path).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Load relay config only if it exists AND is enabled.
    pub async fn load_if_enabled() -> Option<Self> {
        let config = Self::load().await?;
        if config.enabled { Some(config) } else { None }
    }

    /// Set the enabled flag and save.
    pub async fn set_enabled(enabled: bool) -> anyhow::Result<()> {
        let mut config = Self::load().await.ok_or_else(|| {
            anyhow::anyhow!("No relay credentials found. Run `lukan login` first.")
        })?;
        config.enabled = enabled;
        config.save().await
    }

    /// Save relay config to the standard path.
    pub async fn save(&self) -> anyhow::Result<()> {
        let path = crate::config::LukanPaths::config_dir().join("relay.json");
        let data = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    /// Remove the relay config file (logout).
    pub async fn remove() -> anyhow::Result<()> {
        let path = crate::config::LukanPaths::config_dir().join("relay.json");
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }
        Ok(())
    }
}

/// Base64 encoding/decoding for binary body fields in JSON.
mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_to_relay_register_serialization() {
        let msg = DaemonToRelay::Register {
            user_id: "user123".into(),
            device_name: "laptop".into(),
            os: Some("Linux x86_64".into()),
            version: Some("0.1.0".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"register""#));
        assert!(json.contains(r#""userId":"user123""#));
        assert!(json.contains(r#""deviceName":"laptop""#));
        assert!(json.contains(r#""os":"Linux x86_64""#));
        assert!(json.contains(r#""version":"0.1.0""#));

        let deserialized: DaemonToRelay = serde_json::from_str(&json).unwrap();
        match deserialized {
            DaemonToRelay::Register {
                user_id,
                device_name,
                os,
                version,
            } => {
                assert_eq!(user_id, "user123");
                assert_eq!(device_name, "laptop");
                assert_eq!(os.unwrap(), "Linux x86_64");
                assert_eq!(version.unwrap(), "0.1.0");
            }
            _ => panic!("Expected Register variant"),
        }
    }

    #[test]
    fn test_register_backwards_compatible() {
        // Old daemons without os/version fields should still deserialize
        let json = r#"{"type":"register","userId":"u1","deviceName":"old-daemon"}"#;
        let msg: DaemonToRelay = serde_json::from_str(json).unwrap();
        match msg {
            DaemonToRelay::Register { os, version, .. } => {
                assert!(os.is_none());
                assert!(version.is_none());
            }
            _ => panic!("Expected Register variant"),
        }
    }

    #[test]
    fn test_relay_to_daemon_forward_serialization() {
        let inner = serde_json::json!({"type": "send_message", "content": "hello"});
        let msg = RelayToDaemon::Forward {
            connection_id: "conn-1".into(),
            message: inner.clone(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"forward""#));
        assert!(json.contains(r#""connectionId":"conn-1""#));

        let deserialized: RelayToDaemon = serde_json::from_str(&json).unwrap();
        match deserialized {
            RelayToDaemon::Forward {
                connection_id,
                message,
            } => {
                assert_eq!(connection_id, "conn-1");
                assert_eq!(message, inner);
            }
            _ => panic!("Expected Forward variant"),
        }
    }

    #[test]
    fn test_rest_request_body_base64() {
        let msg = RelayToDaemon::RestRequest {
            request_id: "req-1".into(),
            method: "POST".into(),
            path: "/api/config".into(),
            headers: HashMap::new(),
            body: b"hello world".to_vec(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        // Body should be base64-encoded
        assert!(json.contains(r#""aGVsbG8gd29ybGQ=""#));

        let deserialized: RelayToDaemon = serde_json::from_str(&json).unwrap();
        match deserialized {
            RelayToDaemon::RestRequest { body, .. } => {
                assert_eq!(body, b"hello world");
            }
            _ => panic!("Expected RestRequest variant"),
        }
    }

    #[test]
    fn test_relay_config_serde() {
        let config = RelayConfig {
            relay_url: "wss://app.lukan.ai".into(),
            jwt_token: "token123".into(),
            user_id: "user1".into(),
            email: "test@example.com".into(),
            enabled: true,
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains(r#""relayUrl""#));
        assert!(json.contains(r#""jwtToken""#));
        assert!(json.contains(r#""userId""#));

        let deserialized: RelayConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.relay_url, "wss://app.lukan.ai");
        assert_eq!(deserialized.email, "test@example.com");
    }
}
