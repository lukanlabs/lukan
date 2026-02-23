mod auth;
pub mod embedded_ui;
mod protocol;
mod server;
mod state;
mod ws_handler;

use std::sync::Arc;

use anyhow::Result;
use lukan_core::config::ResolvedConfig;

use crate::state::AppState;

/// Start the web server with embedded React UI
pub async fn start_web_server(resolved: ResolvedConfig, port: u16) -> Result<()> {
    let state = Arc::new(AppState::new(resolved));

    // Start the worker scheduler
    state.worker_scheduler.start().await;

    let router = server::create_router(Arc::clone(&state));

    let addr = format!("0.0.0.0:{port}");
    println!("\n  \x1b[1m\x1b[36mlukan web\x1b[0m");
    println!("  \x1b[2mWeb UI running at\x1b[0m \x1b[4mhttp://localhost:{port}\x1b[0m\n");

    // Try to open browser
    let _ = open::that(format!("http://localhost:{port}"));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Web server listening on {addr}");
    axum::serve(listener, router).await?;

    // Stop scheduler on shutdown
    state.worker_scheduler.stop();

    Ok(())
}
