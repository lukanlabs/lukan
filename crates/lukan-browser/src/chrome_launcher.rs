//! Chrome process launcher — finds and spawns a headless Chrome instance.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::{debug, info};

/// Chrome profile strategy.
#[derive(Debug, Clone, Default)]
pub enum ProfileMode {
    /// Ephemeral profile in /tmp, deleted on exit.
    #[default]
    Temp,
    /// Persistent profile in ~/.config/lukan/chrome-profile.
    Persistent,
    /// Use the system Chrome profile (~/.config/google-chrome or macOS equivalent).
    System,
    /// Use a custom path as the profile directory.
    Custom(PathBuf),
}

/// Options for launching Chrome.
#[derive(Debug, Clone)]
pub struct ChromeOptions {
    /// Which profile to use.
    pub profile: ProfileMode,
    /// Run in visible (headed) mode instead of headless.
    pub visible: bool,
    /// Remote debugging port.
    pub port: u16,
}

impl Default for ChromeOptions {
    fn default() -> Self {
        Self {
            profile: ProfileMode::default(),
            visible: false,
            port: 9222,
        }
    }
}

/// A launched Chrome process with its CDP URL.
pub struct LaunchedChrome {
    pub cdp_url: String,
    child: tokio::process::Child,
    _temp_dir: Option<PathBuf>,
}

impl LaunchedChrome {
    /// Kill the Chrome process.
    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl Drop for LaunchedChrome {
    fn drop(&mut self) {
        self.kill();
        // Clean up temp profile dir
        if let Some(ref dir) = self._temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Find a Chrome/Chromium binary on the system.
pub fn find_chrome() -> Result<PathBuf> {
    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ]
    } else {
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
    };

    for path in &candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // Fallback: check PATH
    for name in &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if let Some(path) = which(name) {
            return Ok(path);
        }
    }

    bail!(
        "Chrome/Chromium not found. Install Chrome or provide a CDP URL with --browser-cdp.\n\
         Searched: {}",
        candidates.join(", ")
    )
}

/// Launch Chrome with remote debugging enabled.
pub async fn launch_chrome(opts: &ChromeOptions) -> Result<LaunchedChrome> {
    let chrome_path = find_chrome()?;
    info!(path = %chrome_path.display(), "Found Chrome");

    let profile_dir;
    let temp_dir;

    match &opts.profile {
        ProfileMode::Temp => {
            let pid = std::process::id();
            let dir = PathBuf::from(format!("/tmp/lukan-chrome-{pid}"));
            std::fs::create_dir_all(&dir).ok();
            profile_dir = dir.clone();
            temp_dir = Some(dir);
        }
        ProfileMode::Persistent => {
            let dir = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("lukan")
                .join("chrome-profile");
            std::fs::create_dir_all(&dir).ok();
            profile_dir = dir;
            temp_dir = None;
        }
        ProfileMode::System => {
            let dir = if cfg!(target_os = "macos") {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join("Library/Application Support/Google/Chrome")
            } else {
                dirs::config_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join("google-chrome")
            };
            info!(path = %dir.display(), "Using system Chrome profile");
            profile_dir = dir;
            temp_dir = None;
        }
        ProfileMode::Custom(path) => {
            std::fs::create_dir_all(path).ok();
            profile_dir = path.clone();
            temp_dir = None;
        }
    }

    let port_str = opts.port.to_string();

    let mut args = vec![
        format!("--remote-debugging-port={port_str}"),
        format!("--user-data-dir={}", profile_dir.display()),
        "--no-first-run".to_string(),
        "--disable-default-apps".to_string(),
        "--disable-extensions".to_string(),
        "--disable-sync".to_string(),
        "--disable-translate".to_string(),
    ];

    // --no-sandbox is needed in containers and restricted environments
    args.push("--no-sandbox".to_string());

    if !opts.visible {
        args.push("--headless=new".to_string());
        args.push("--disable-gpu".to_string());
    }

    args.push("about:blank".to_string());

    debug!(
        chrome = %chrome_path.display(),
        args = ?args,
        "Launching Chrome"
    );

    let child = tokio::process::Command::new(&chrome_path)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to launch Chrome: {}", chrome_path.display()))?;

    // Poll until Chrome's debugging endpoint is ready
    let cdp_url = format!("http://127.0.0.1:{port_str}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    loop {
        if tokio::time::Instant::now() > deadline {
            bail!("Chrome did not start within 10 seconds");
        }

        match client.get(format!("{cdp_url}/json/version")).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("Chrome debugging endpoint ready at {cdp_url}");
                break;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }

    Ok(LaunchedChrome {
        cdp_url,
        child,
        _temp_dir: temp_dir,
    })
}

/// Simple `which` — find an executable in PATH.
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() { Some(full) } else { None }
        })
    })
}
