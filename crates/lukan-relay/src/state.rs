use std::net::IpAddr;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Sender half for writing to a WebSocket connection.
pub type WsSender = mpsc::UnboundedSender<String>;

// ── Device Code Flow ─────────────────────────────────────────────────

/// Status of a device code authorization request.
pub enum DeviceCodeStatus {
    Pending,
    Authorized {
        token: String,
        user_id: String,
        email: String,
    },
}

/// A pending device code entry (for headless login flow).
pub struct DeviceCodeEntry {
    pub user_code: String,
    pub device_code: String,
    pub expires_at: Instant,
    pub status: DeviceCodeStatus,
}

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

// ── OAuth CSRF State ────────────────────────────────────────────────

/// A pending OAuth state nonce (CSRF protection).
pub struct OAuthStateEntry {
    pub flow_info: String,
    pub expires_at: Instant,
}

// ── Auth Code Exchange ──────────────────────────────────────────────

/// A short-lived auth code that can be exchanged for a JWT (replaces token-in-URL).
pub struct AuthCodeEntry {
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub expires_at: Instant,
}

// ── Rate Limiter ────────────────────────────────────────────────────

/// Simple per-IP sliding-window rate limiter.
pub struct RateLimiter {
    /// IP → (request count, window start)
    windows: DashMap<IpAddr, (u32, Instant)>,
    /// Maximum requests per window
    max_requests: u32,
    /// Window duration
    window_duration: std::time::Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            windows: DashMap::new(),
            max_requests,
            window_duration: std::time::Duration::from_secs(window_secs),
        }
    }

    /// Returns true if the request is allowed, false if rate-limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut entry = self.windows.entry(ip).or_insert((0, now));
        let (count, window_start) = entry.value_mut();

        if now.duration_since(*window_start) >= self.window_duration {
            // Reset window
            *count = 1;
            *window_start = now;
            true
        } else if *count < self.max_requests {
            *count += 1;
            true
        } else {
            false
        }
    }
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
    /// device_code → DeviceCodeEntry (for headless device code login flow)
    pub device_codes: DashMap<String, DeviceCodeEntry>,
    /// OAuth CSRF state nonces (nonce → OAuthStateEntry)
    pub oauth_states: DashMap<String, OAuthStateEntry>,
    /// Short-lived auth codes for CLI code exchange (code → AuthCodeEntry)
    pub auth_codes: DashMap<String, AuthCodeEntry>,
    /// Rate limiters for various endpoints
    pub rate_device_start: RateLimiter,
    pub rate_device_verify: RateLimiter,
    pub rate_device_poll: RateLimiter,
    pub rate_dev_token: RateLimiter,
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
            device_codes: DashMap::new(),
            oauth_states: DashMap::new(),
            auth_codes: DashMap::new(),
            rate_device_start: RateLimiter::new(10, 60),
            rate_device_verify: RateLimiter::new(5, 60),
            rate_device_poll: RateLimiter::new(30, 60),
            rate_dev_token: RateLimiter::new(5, 60),
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

    /// JWT secret for browser tokens.
    /// Uses the same stable secret so sessions survive relay restarts.
    pub fn browser_jwt_secret(&self) -> String {
        self.jwt_secret.clone()
    }

    // ── OAuth CSRF helpers ──────────────────────────────────────────

    /// Create a CSRF-protected OAuth state parameter.
    /// Returns a string like "{nonce}:{flow_info}" with a 10-minute TTL.
    pub fn create_oauth_state(&self, flow_info: &str) -> String {
        let nonce = uuid::Uuid::new_v4().to_string();
        let entry = OAuthStateEntry {
            flow_info: flow_info.to_string(),
            expires_at: Instant::now() + std::time::Duration::from_secs(10 * 60),
        };
        self.oauth_states.insert(nonce.clone(), entry);
        format!("{nonce}:{flow_info}")
    }

    /// Verify and consume an OAuth state parameter (single-use).
    /// Returns the flow_info if valid, None otherwise.
    pub fn verify_oauth_state(&self, state_param: &str) -> Option<String> {
        let nonce = state_param.split(':').next()?;
        let (_, entry) = self.oauth_states.remove(nonce)?;
        if entry.expires_at > Instant::now() {
            Some(entry.flow_info)
        } else {
            None
        }
    }

    // ── Auth Code Exchange helpers ──────────────────────────────────

    /// Create a short-lived auth code that can be exchanged for credentials.
    /// Returns the code string (60-second TTL, single-use).
    pub fn create_auth_code(&self, token: String, user_id: String, email: String) -> String {
        let code = uuid::Uuid::new_v4().to_string();
        let entry = AuthCodeEntry {
            token,
            user_id,
            email,
            expires_at: Instant::now() + std::time::Duration::from_secs(60),
        };
        self.auth_codes.insert(code.clone(), entry);
        code
    }

    /// Exchange an auth code for credentials (single-use).
    pub fn exchange_auth_code(&self, code: &str) -> Option<AuthCodeEntry> {
        let (_, entry) = self.auth_codes.remove(code)?;
        if entry.expires_at > Instant::now() {
            Some(entry)
        } else {
            None
        }
    }

    // ── Device Code helpers ──────────────────────────────────────────

    /// Charset for device user codes: A-Z 0-9 (36 chars).
    const USER_CODE_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

    /// Create a new device code entry with a 15-minute TTL.
    /// Returns (device_code, user_code) where user_code is 8 alphanumeric chars (XXXX-XXXX).
    pub fn create_device_code(&self) -> (String, String) {
        use rand::Rng;
        let device_code = uuid::Uuid::new_v4().to_string();
        let user_code = {
            let mut rng = rand::rng();
            let chars: String = (0..8)
                .map(|_| {
                    let idx = rng.random_range(0..Self::USER_CODE_CHARSET.len());
                    Self::USER_CODE_CHARSET[idx] as char
                })
                .collect();
            format!("{}-{}", &chars[..4], &chars[4..])
        };
        let entry = DeviceCodeEntry {
            user_code: user_code.clone(),
            device_code: device_code.clone(),
            expires_at: Instant::now() + std::time::Duration::from_secs(15 * 60),
            status: DeviceCodeStatus::Pending,
        };
        self.device_codes.insert(device_code.clone(), entry);
        (device_code, user_code)
    }

    /// Find a device code entry by its user-facing code. Returns the device_code key if found.
    pub fn find_by_user_code(&self, user_code: &str) -> Option<String> {
        // Normalize: uppercase and strip non-alphanumeric, then re-insert dash
        let normalized: String = user_code
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_uppercase();
        let formatted = if normalized.len() == 8 {
            format!("{}-{}", &normalized[..4], &normalized[4..])
        } else {
            normalized
        };

        for entry in self.device_codes.iter() {
            if entry.user_code == formatted && entry.expires_at > Instant::now() {
                return Some(entry.device_code.clone());
            }
        }
        None
    }

    /// Poll a device code. Returns None if not found/expired.
    pub fn poll_device_code(&self, device_code: &str) -> Option<DeviceCodePollResult> {
        let entry = self.device_codes.get(device_code)?;
        if entry.expires_at <= Instant::now() {
            drop(entry);
            self.device_codes.remove(device_code);
            return Some(DeviceCodePollResult::Expired);
        }
        match &entry.status {
            DeviceCodeStatus::Pending => Some(DeviceCodePollResult::Pending),
            DeviceCodeStatus::Authorized {
                token,
                user_id,
                email,
            } => {
                let result = DeviceCodePollResult::Authorized {
                    token: token.clone(),
                    user_id: user_id.clone(),
                    email: email.clone(),
                };
                drop(entry);
                self.device_codes.remove(device_code);
                Some(result)
            }
        }
    }

    /// Mark a device code as authorized (called after user authenticates in browser).
    pub fn complete_device_code(
        &self,
        device_code: &str,
        token: String,
        user_id: String,
        email: String,
    ) -> bool {
        if let Some(mut entry) = self.device_codes.get_mut(device_code) {
            if entry.expires_at <= Instant::now() {
                return false;
            }
            entry.status = DeviceCodeStatus::Authorized {
                token,
                user_id,
                email,
            };
            true
        } else {
            false
        }
    }
}

/// Result of polling a device code.
pub enum DeviceCodePollResult {
    Pending,
    Expired,
    Authorized {
        token: String,
        user_id: String,
        email: String,
    },
}

/// Convenience type for sharing state via Axum.
pub type SharedState = Arc<RelayState>;
