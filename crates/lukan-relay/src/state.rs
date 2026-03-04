use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

/// Sender half for writing to a WebSocket connection.
pub type WsSender = mpsc::UnboundedSender<String>;

/// A connected daemon (user's local machine).
pub struct DaemonConnection {
    pub user_id: String,
    pub device_name: String,
    pub tx: WsSender,
}

/// A connected browser client.
pub struct BrowserConnection {
    pub user_id: String,
    pub tx: WsSender,
}

/// Pending REST tunnel request waiting for daemon response.
pub struct PendingRestRequest {
    pub tx: tokio::sync::oneshot::Sender<RestTunnelResponse>,
}

/// Response from a tunneled REST request.
pub struct RestTunnelResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Shared relay server state.
pub struct RelayState {
    /// user_id → daemon connection (one daemon per user for now)
    pub daemon_connections: DashMap<String, DaemonConnection>,
    /// connection_id → browser connection
    pub browser_connections: DashMap<String, BrowserConnection>,
    /// request_id → pending REST tunnel oneshot
    pub pending_rest: DashMap<String, PendingRestRequest>,
    /// JWT signing key
    pub jwt_secret: String,
    /// Google OAuth client ID
    pub google_client_id: String,
    /// Google OAuth client secret
    pub google_client_secret: String,
    /// Public URL of this relay server (for OAuth redirects)
    pub public_url: String,
    /// Dev mode: enables /auth/dev endpoints for testing without Google OAuth
    pub dev_mode: bool,
    /// Dev secret: required to access /auth/dev endpoints (prevents unauthorized access)
    pub dev_secret: Option<String>,
    /// Random ID generated on each boot — browser tokens signed with this become
    /// invalid when the relay restarts, forcing re-authentication.
    pub boot_id: String,
}

impl RelayState {
    pub fn new() -> Self {
        let jwt_secret =
            std::env::var("RELAY_JWT_SECRET").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
        let google_client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
        let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
        let public_url =
            std::env::var("RELAY_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let dev_mode = std::env::var("RELAY_DEV_MODE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let dev_secret = std::env::var("RELAY_DEV_SECRET").ok();
        let boot_id = uuid::Uuid::new_v4().to_string();

        Self {
            daemon_connections: DashMap::new(),
            browser_connections: DashMap::new(),
            pending_rest: DashMap::new(),
            jwt_secret,
            google_client_id,
            google_client_secret,
            public_url,
            dev_mode,
            dev_secret,
            boot_id,
        }
    }

    /// Send a JSON message to the daemon for the given user.
    /// Returns false if no daemon is connected.
    pub fn send_to_daemon(&self, user_id: &str, message: &str) -> bool {
        if let Some(daemon) = self.daemon_connections.get(user_id) {
            daemon.tx.send(message.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Send a JSON message to a specific browser connection.
    /// Returns false if the connection doesn't exist.
    pub fn send_to_browser(&self, connection_id: &str, message: &str) -> bool {
        if let Some(browser) = self.browser_connections.get(connection_id) {
            browser.tx.send(message.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Send a JSON message to a browser connection, but only if it belongs to the given user.
    /// Prevents a daemon from sending messages to another user's browser.
    pub fn send_to_browser_if_owned(
        &self,
        connection_id: &str,
        user_id: &str,
        message: &str,
    ) -> bool {
        if let Some(browser) = self.browser_connections.get(connection_id) {
            if browser.user_id == user_id {
                browser.tx.send(message.to_string()).is_ok()
            } else {
                false // connection belongs to a different user
            }
        } else {
            false
        }
    }

    /// Check if a daemon is connected for the given user.
    pub fn has_daemon(&self, user_id: &str) -> bool {
        self.daemon_connections.contains_key(user_id)
    }

    /// JWT secret for browser tokens. Includes boot_id so all browser
    /// tokens are invalidated when the relay restarts.
    pub fn browser_jwt_secret(&self) -> String {
        format!("{}.{}", self.jwt_secret, self.boot_id)
    }
}

/// Convenience type for sharing state via Axum.
pub type SharedState = Arc<RelayState>;
