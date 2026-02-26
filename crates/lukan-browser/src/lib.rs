//! `lukan-browser` — Chrome DevTools Protocol integration for lukan.
//!
//! Provides a CDP WebSocket client, Chrome launcher, accessibility tree
//! parser, and a `BrowserManager` singleton that coordinates them.

#![allow(dead_code)]

pub mod ax_tree;
pub mod cdp_client;
pub mod chrome_launcher;
pub mod url_guard;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use cdp_client::CdpClient;
use chrome_launcher::{ChromeOptions, LaunchedChrome};

// ── Browser configuration ──────────────────────────────────────────────

// Re-export ProfileMode so callers can use it directly.
pub use chrome_launcher::ProfileMode;

/// Configuration for the browser manager.
#[derive(Debug, Clone, Default)]
pub struct BrowserConfig {
    /// Direct CDP WebSocket/HTTP URL (skips auto-launch).
    pub cdp_url: Option<String>,
    /// Allow navigation to internal/private IPs.
    pub allow_internal: bool,
    /// Chrome profile strategy.
    pub profile: ProfileMode,
    /// Run Chrome in visible (headed) mode.
    pub visible: bool,
    /// Directory for browser downloads (default: ~/Downloads/lukan/).
    pub download_dir: Option<std::path::PathBuf>,
    /// Browser name: "auto", "chrome", "edge", "chromium".
    pub browser_name: String,
}

// ── BrowserManager singleton ───────────────────────────────────────────

static BROWSER_MANAGER: OnceLock<Arc<BrowserManager>> = OnceLock::new();

pub struct BrowserManager {
    config: BrowserConfig,
    state: Mutex<BrowserState>,
}

struct BrowserState {
    client: Option<CdpClient>,
    chrome: Option<LaunchedChrome>,
    http_base: Option<String>,
    /// Set to `false` by `disconnect()` to prevent `ensure_connected()` from
    /// auto-launching Chrome.  Re-set to `true` by `reactivate()`.
    active: bool,
}

impl BrowserManager {
    /// Initialize the global BrowserManager singleton.
    pub fn init(config: BrowserConfig) {
        if config.allow_internal {
            url_guard::set_allow_internal(true);
        }

        let manager = Arc::new(BrowserManager {
            config,
            state: Mutex::new(BrowserState {
                client: None,
                chrome: None,
                http_base: None,
                active: true,
            }),
        });

        let _ = BROWSER_MANAGER.set(manager);
        info!("BrowserManager initialized");
    }

    /// Get the global BrowserManager, if initialized.
    pub fn get() -> Option<Arc<BrowserManager>> {
        BROWSER_MANAGER.get().cloned()
    }

    /// Get a connected CDP client, connecting/launching as needed.
    pub async fn get_cdp_client(&self) -> Result<&CdpClient> {
        // We need to return a reference into the Mutex, so we use a different pattern:
        // ensure connected, then the caller uses send_cdp() instead.
        bail!("Use send_cdp() / call methods directly instead");
    }

    /// Check if the browser is active (not shut down) without auto-connecting.
    pub async fn is_active(&self) -> bool {
        let state = self.state.lock().await;
        state.active && (state.chrome.is_some() || state.client.is_some())
    }

    /// Re-activate after a `disconnect()` so the next `ensure_connected()`
    /// will auto-launch again.
    pub async fn reactivate(&self) {
        let mut state = self.state.lock().await;
        state.active = true;
    }

    /// Ensure we have a connected CDP client, connecting/launching as needed.
    async fn ensure_connected(&self) -> Result<()> {
        let mut state = self.state.lock().await;

        // Don't auto-launch if explicitly shut down
        if !state.active {
            bail!("Browser was shut down. Call reactivate() or re-launch.");
        }

        // Check if already connected
        if let Some(ref client) = state.client {
            if client.is_connected().await {
                return Ok(());
            }
            warn!("CDP client disconnected, reconnecting...");
            state.client = None;
        }

        // Connect or launch
        let cdp_url = if let Some(ref url) = self.config.cdp_url {
            url.clone()
        } else if let Some(ref chrome) = state.chrome {
            chrome.cdp_url.clone()
        } else {
            // Auto-launch browser
            info!("Auto-launching browser...");
            let opts = ChromeOptions {
                profile: self.config.profile.clone(),
                visible: self.config.visible,
                browser_name: self.config.browser_name.clone(),
                ..Default::default()
            };
            let chrome = chrome_launcher::launch_chrome(&opts).await?;
            let url = chrome.cdp_url.clone();
            state.chrome = Some(chrome);
            url
        };

        // Derive HTTP base URL
        let http_base = derive_http_base(&cdp_url);
        state.http_base = Some(http_base);

        // Connect CDP client
        let client = CdpClient::connect(&cdp_url, Duration::from_secs(10)).await?;

        // Enable required domains
        let dl_dir = self.download_dir();
        enable_domains(&client, &dl_dir).await?;

        state.client = Some(client);
        Ok(())
    }

    /// Send a CDP command (auto-connects if needed).
    pub async fn send_cdp(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.ensure_connected().await?;
        let state = self.state.lock().await;
        let client = state.client.as_ref().context("CDP client not connected")?;
        client.send(method, params).await
    }

    /// Wait for a CDP event (auto-connects if needed).
    pub async fn wait_for_event(
        &self,
        event: &str,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        self.ensure_connected().await?;
        let state = self.state.lock().await;
        let client = state.client.as_ref().context("CDP client not connected")?;
        client.wait_for_event(event, timeout).await
    }

    /// Subscribe to a CDP event stream.
    pub async fn on_event(
        &self,
        event: &str,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>> {
        self.ensure_connected().await?;
        let state = self.state.lock().await;
        let client = state.client.as_ref().context("CDP client not connected")?;
        Ok(client.on(event).await)
    }

    /// Switch to a different tab by its WebSocket URL.
    pub async fn switch_to_tab(&self, ws_url: &str) -> Result<()> {
        let mut state = self.state.lock().await;

        // Disconnect current client
        if let Some(ref client) = state.client {
            client.disconnect().await;
        }
        state.client = None;

        // Connect to new target
        let client = CdpClient::connect(ws_url, Duration::from_secs(10)).await?;
        let dl_dir = self.download_dir();
        enable_domains(&client, &dl_dir).await?;
        state.client = Some(client);

        debug!(ws_url = %ws_url, "Switched to tab");
        Ok(())
    }

    /// Get the HTTP base URL for the debugging endpoint.
    pub async fn http_base(&self) -> Result<String> {
        self.ensure_connected().await?;
        let state = self.state.lock().await;
        state.http_base.clone().context("HTTP base not available")
    }

    /// Take a quick JPEG screenshot (quality 50).
    pub async fn quick_screenshot(&self) -> Result<String> {
        let result = self
            .send_cdp(
                "Page.captureScreenshot",
                json!({
                    "format": "jpeg",
                    "quality": 50,
                }),
            )
            .await?;

        let data = result
            .get("data")
            .and_then(|d| d.as_str())
            .context("No screenshot data")?;

        Ok(format!("data:image/jpeg;base64,{data}"))
    }

    /// Get the accessibility snapshot of the current page.
    /// When `compact` is true, only interactive elements are returned.
    pub async fn snapshot(&self, compact: bool) -> Result<String> {
        self.ensure_connected().await?;
        let state = self.state.lock().await;
        let client = state.client.as_ref().context("CDP client not connected")?;
        ax_tree::get_accessibility_snapshot(client, compact).await
    }

    /// Get the download directory (creates it if needed).
    pub fn download_dir(&self) -> std::path::PathBuf {
        if let Some(ref dir) = self.config.download_dir {
            dir.clone()
        } else {
            dirs::download_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Downloads"))
                .join("lukan")
        }
    }

    /// Save raw bytes to the download directory, returning the full path.
    pub fn save_to_downloads(&self, filename: &str, data: &[u8]) -> Result<std::path::PathBuf> {
        let dir = self.download_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create download dir: {}", dir.display()))?;
        let path = dir.join(filename);
        std::fs::write(&path, data)
            .with_context(|| format!("Failed to write file: {}", path.display()))?;
        Ok(path)
    }

    /// Disconnect from Chrome and kill the process if we launched it.
    /// Sets `active = false` to prevent `ensure_connected()` from auto-relaunching.
    pub async fn disconnect(&self) {
        let mut state = self.state.lock().await;
        if let Some(ref client) = state.client {
            client.disconnect().await;
        }
        state.client = None;
        if let Some(ref mut chrome) = state.chrome {
            chrome.kill();
        }
        state.chrome = None;
        state.active = false;
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Enable the CDP domains we need.
async fn enable_domains(client: &CdpClient, download_dir: &std::path::Path) -> Result<()> {
    // Ensure download dir exists
    std::fs::create_dir_all(download_dir).ok();

    // Fire all enables concurrently
    let (r1, r2, r3, r4, _r5) = tokio::join!(
        client.send("Page.enable", json!({})),
        client.send("DOM.enable", json!({})),
        client.send("Accessibility.enable", json!({})),
        client.send("Runtime.enable", json!({})),
        // Auto-accept downloads to our directory (avoids native Save dialog)
        client.send(
            "Browser.setDownloadBehavior",
            json!({
                "behavior": "allowAndName",
                "downloadPath": download_dir.to_string_lossy(),
                "eventsEnabled": true,
            }),
        ),
    );
    r1.context("Page.enable failed")?;
    r2.context("DOM.enable failed")?;
    r3.context("Accessibility.enable failed")?;
    r4.context("Runtime.enable failed")?;
    // Download behavior is best-effort — not all Chrome versions support it
    Ok(())
}

/// Derive an HTTP base URL from a WebSocket or HTTP CDP URL.
fn derive_http_base(url: &str) -> String {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        let http = url
            .replacen("ws://", "http://", 1)
            .replacen("wss://", "https://", 1);
        // Strip the path (e.g. /devtools/page/xxx)
        if let Ok(parsed) = url::Url::parse(&http) {
            return format!(
                "{}://{}:{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("127.0.0.1"),
                parsed.port().unwrap_or(9222)
            );
        }
        http
    } else {
        url.trim_end_matches('/').to_string()
    }
}
