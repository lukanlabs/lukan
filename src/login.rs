use anyhow::{Context, Result};
use tracing::info;

use lukan_core::relay::RelayConfig;

/// Run the `lukan login` command.
///
/// - Without `--remote`: opens browser + localhost callback (default relay)
/// - With `--remote <url>`: asks user to choose browser or device code flow
pub async fn run_login(relay_url: Option<&str>) -> Result<()> {
    let relay_url = relay_url.unwrap_or("https://remote.lukan.ai");

    let selection = dialoguer::Select::new()
        .with_prompt("How would you like to authenticate?")
        .items(&[
            "Browser (opens login page)",
            "Device code (for headless/SSH)",
        ])
        .default(0)
        .interact()?;

    match selection {
        0 => run_local_login(relay_url).await,
        1 => run_device_code_login(relay_url).await,
        _ => unreachable!(),
    }
}

/// Device code login flow — works without a local browser or port forwarding.
/// 1. POST /auth/device → get device_code + user_code
/// 2. Print verification URL + code for user
/// 3. Poll /auth/device/poll until authorized
/// 4. Save relay.json + restart daemon
async fn run_device_code_login(relay_url: &str) -> Result<()> {
    let client = reqwest::Client::new();

    // Step 1: Request a device code
    let resp = client
        .post(format!("{relay_url}/auth/device"))
        .send()
        .await
        .context("Failed to reach relay server")?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to start device login: {text}");
    }

    let data: serde_json::Value = resp.json().await?;
    let device_code = data["deviceCode"]
        .as_str()
        .context("Missing deviceCode in response")?;
    let user_code = data["userCode"]
        .as_str()
        .context("Missing userCode in response")?;
    let interval = data["interval"].as_u64().unwrap_or(5);

    // Step 2: Show the code to the user
    // Use the relay_url we already know instead of the server's verificationUrl
    // (which may be localhost if RELAY_PUBLIC_URL isn't configured)
    println!();
    println!("  Visit: {relay_url}/device");
    println!("  Enter code: {user_code}");
    println!();
    println!("  Waiting for authorization...");

    // Step 3: Poll until authorized or expired
    let poll_body = serde_json::json!({ "deviceCode": device_code });

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let resp = client
            .post(format!("{relay_url}/auth/device/poll"))
            .json(&poll_body)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                info!(error = %e, "Poll request failed, retrying...");
                continue;
            }
        };

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(_) => continue,
        };

        match data["status"].as_str() {
            Some("pending") => continue,
            Some("authorized") => {
                let token = data["token"].as_str().context("Missing token")?.to_string();
                let user_id = data["userId"]
                    .as_str()
                    .context("Missing userId")?
                    .to_string();
                let email = data["email"].as_str().context("Missing email")?.to_string();

                // Save relay config
                let config = RelayConfig {
                    relay_url: relay_url.to_string(),
                    jwt_token: token,
                    user_id: user_id.clone(),
                    email: email.clone(),
                    enabled: true,
                };
                config.save().await?;

                println!("  Logged in as {email}");
                info!(user_id = %user_id, email = %email, "Device code login successful");

                restart_daemon_if_running().await;
                return Ok(());
            }
            Some("expired") => {
                anyhow::bail!("Device code expired. Please run `lukan login --remote` again.");
            }
            other => {
                anyhow::bail!("Unexpected poll status: {other:?}");
            }
        }
    }
}

/// Original localhost callback login flow (requires local browser access).
async fn run_local_login(relay_url: &str) -> Result<()> {
    // Find a free port for the local callback server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);

    // Open browser to the relay's login page with cli_port parameter.
    let auth_url = format!("{relay_url}/?cli_port={port}");
    println!("  Opening browser for login...");
    println!("  If the browser doesn't open, visit: {auth_url}");
    let _ = open::that(&auth_url);

    // Start local HTTP server to receive the callback (now receives a code, not a token)
    let (code_tx, mut code_rx) = tokio::sync::mpsc::channel::<(String, String, String)>(1);

    let callback_server = {
        let code_tx = code_tx.clone();
        async move {
            use axum::extract::Query;
            use axum::response::Html;
            use axum::routing::get;

            #[derive(serde::Deserialize)]
            struct CallbackParams {
                code: String,
                user_id: String,
                email: String,
            }

            let app = axum::Router::new().route(
                "/callback",
                get(
                    move |Query(params): Query<CallbackParams>| {
                        let tx = code_tx.clone();
                        async move {
                            let _ = tx.send((params.code, params.user_id, params.email)).await;
                            Html(
                                "<html><body style='font-family: system-ui; text-align: center; padding-top: 100px'>\
                                 <h1>Logged in to lukan</h1>\
                                 <p>You can close this window and return to the terminal.</p>\
                                 </body></html>"
                            )
                        }
                    },
                ),
            );

            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
                .await
                .expect("Failed to bind callback port");

            // Serve with a timeout
            tokio::time::timeout(
                std::time::Duration::from_secs(300), // 5 minute timeout
                axum::serve(listener, app).into_future(),
            )
            .await
        }
    };

    // Run callback server in background
    let server_handle = tokio::spawn(callback_server);

    // Wait for the auth code callback
    println!("  Waiting for authentication...");
    let (auth_code, _user_id_hint, _email_hint) = code_rx
        .recv()
        .await
        .context("Login timed out or was cancelled")?;

    // Exchange the short-lived auth code for the actual JWT token
    let client = reqwest::Client::new();
    let exchange_resp = client
        .post(format!("{relay_url}/auth/exchange"))
        .json(&serde_json::json!({ "code": auth_code }))
        .send()
        .await
        .context("Failed to exchange auth code")?;

    if !exchange_resp.status().is_success() {
        let text = exchange_resp.text().await.unwrap_or_default();
        anyhow::bail!("Auth code exchange failed: {text}");
    }

    let data: serde_json::Value = exchange_resp.json().await?;
    let jwt_token = data["token"]
        .as_str()
        .context("Missing token in exchange response")?
        .to_string();
    let user_id = data["userId"]
        .as_str()
        .context("Missing userId in exchange response")?
        .to_string();
    let email = data["email"]
        .as_str()
        .context("Missing email in exchange response")?
        .to_string();

    // Save relay config
    let config = RelayConfig {
        relay_url: relay_url.to_string(),
        jwt_token,
        user_id: user_id.clone(),
        email: email.clone(),
        enabled: true,
    };
    config.save().await?;

    // Abort the callback server
    server_handle.abort();

    println!("  Logged in as {email}");
    info!(user_id = %user_id, email = %email, "Login successful");

    restart_daemon_if_running().await;

    Ok(())
}

/// Restart the daemon if it's running so it picks up new relay config.
async fn restart_daemon_if_running() {
    if crate::daemon::is_daemon_running() {
        println!("  Restarting daemon...");
        let _ = crate::daemon::stop_daemon();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = crate::daemon::ensure_daemon_running();
        println!("  Daemon restarted with new relay config.");
    } else {
        println!("  Relay config saved. Run `lukan daemon start` to connect.");
    }
}

/// Run the `lukan logout` command.
pub async fn run_logout() -> Result<()> {
    RelayConfig::remove().await?;
    println!("  Logged out. Relay config removed.");
    println!("  Restart the daemon to disconnect from the relay.");
    Ok(())
}

/// Show relay connection status.
pub async fn show_relay_status() -> Result<()> {
    match RelayConfig::load().await {
        Some(config) => {
            let status = if config.enabled {
                "enabled"
            } else {
                "disabled"
            };
            println!("  Relay: configured ({status})");
            println!("  URL: {}", config.relay_url);
            println!("  Email: {}", config.email);
            println!("  User ID: {}", config.user_id);
        }
        None => {
            println!("  Relay: not configured");
            println!("  Run `lukan login` to connect to the relay.");
        }
    }
    Ok(())
}
