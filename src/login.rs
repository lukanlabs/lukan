use anyhow::{Context, Result};
use tracing::info;

use lukan_core::relay::RelayConfig;

/// Run the `lukan login` command.
///
/// 1. Opens browser to relay's Google OAuth page
/// 2. Starts a local HTTP server to receive the callback
/// 3. Saves the JWT token to `~/.config/lukan/relay.json`
pub async fn run_login(relay_url: Option<&str>) -> Result<()> {
    let relay_url = relay_url.unwrap_or("https://app.lukan.ai");

    // Find a free port for the local callback server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);

    // Open browser to the relay's login page with cli_port parameter.
    // The login page handles auth method selection (Google, dev mode, etc.)
    // and calls back to our local server after successful authentication.
    let auth_url = format!("{relay_url}/?cli_port={port}");
    println!("  Opening browser for login...");
    println!("  If the browser doesn't open, visit: {auth_url}");
    let _ = open::that(&auth_url);

    // Start local HTTP server to receive the callback
    let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<(String, String, String)>(1);

    let callback_server = {
        let token_tx = token_tx.clone();
        async move {
            use axum::extract::Query;
            use axum::response::Html;
            use axum::routing::get;

            #[derive(serde::Deserialize)]
            struct CallbackParams {
                token: String,
                user_id: String,
                email: String,
            }

            let app = axum::Router::new().route(
                "/callback",
                get(
                    move |Query(params): Query<CallbackParams>| {
                        let tx = token_tx.clone();
                        async move {
                            let _ = tx.send((params.token, params.user_id, params.email)).await;
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

    // Wait for the token callback
    println!("  Waiting for authentication...");
    let (jwt_token, user_id, email) = token_rx
        .recv()
        .await
        .context("Login timed out or was cancelled")?;

    // Save relay config
    let config = RelayConfig {
        relay_url: relay_url.to_string(),
        jwt_token,
        user_id: user_id.clone(),
        email: email.clone(),
    };
    config.save().await?;

    // Abort the callback server
    server_handle.abort();

    println!("  Logged in as {email}");
    info!(user_id = %user_id, email = %email, "Login successful");

    // Restart daemon so it picks up the new relay config
    if crate::daemon::is_daemon_running() {
        println!("  Restarting daemon...");
        let _ = crate::daemon::stop_daemon();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        crate::daemon::ensure_daemon_running();
        println!("  Daemon restarted with new relay config.");
    } else {
        println!("  Relay config saved. Run `lukan daemon start` to connect.");
    }

    Ok(())
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
            println!("  Relay: connected");
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
