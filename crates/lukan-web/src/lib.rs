mod auth;
mod auth_middleware;
mod protocol;
mod rest_browser;
mod rest_config;
mod rest_credentials;
mod rest_events;
mod rest_files;
mod rest_memory;
mod rest_plugins;
mod rest_processes;
mod rest_providers;
mod rest_workers;
mod server;
mod state;
mod static_files;
mod terminal;
mod ws_handler;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use lukan_agent::NotificationWatcher;
use lukan_core::config::ResolvedConfig;

use crate::state::AppState;

/// Start the web server with embedded React UI
pub async fn start_web_server(resolved: ResolvedConfig, port: u16) -> Result<()> {
    let state = Arc::new(AppState::new(resolved));

    // Spawn background task to poll notification file and broadcast to WebSocket clients
    let notify_tx = state.notification_tx.clone();
    tokio::spawn(async move {
        let mut watcher = NotificationWatcher::new();
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            for notif in watcher.poll().await {
                let _ = notify_tx.send(notif);
            }
        }
    });

    let router = server::create_router(Arc::clone(&state));

    let addr = format!("0.0.0.0:{port}");
    println!("\n  \x1b[1m\x1b[36mlukan web\x1b[0m");
    println!("  \x1b[2mWeb UI running at\x1b[0m \x1b[4mhttp://localhost:{port}\x1b[0m\n");

    // Try to open browser
    let _ = open::that(format!("http://localhost:{port}"));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Web server listening on {addr}");
    axum::serve(listener, router).await?;

    Ok(())
}
