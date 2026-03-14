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

/// Save host terminal state and install a signal handler that restores it.
///
/// tmux subprocesses can corrupt the host TTY even with `setsid()`.
/// By saving termios at startup and restoring on SIGINT/SIGTERM, we
/// guarantee the user's terminal is never left in a broken state.
#[cfg(unix)]
fn install_terminal_guard() {
    use std::sync::OnceLock;
    static SAVED_TERMIOS: OnceLock<libc::termios> = OnceLock::new();

    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
            SAVED_TERMIOS.get_or_init(|| termios);
        }
    }

    // Install signal handlers that restore termios before exiting
    unsafe {
        extern "C" fn restore_and_exit(sig: libc::c_int) {
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
                    // Restore canonical mode, echo, and sane settings
                    termios.c_lflag |= libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN;
                    termios.c_iflag |= libc::ICRNL;
                    termios.c_oflag |= libc::OPOST;
                    let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios);
                }
                // Write a newline so the shell prompt appears on a clean line
                let _ = libc::write(libc::STDOUT_FILENO, b"\n" as *const u8 as _, 1);
                // Re-raise with default handler to get the correct exit status
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
        }

        libc::signal(
            libc::SIGINT,
            restore_and_exit as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            restore_and_exit as *const () as libc::sighandler_t,
        );
    }
}

/// Start the web server with embedded React UI (interactive mode — opens browser, blocks)
pub async fn start_web_server(resolved: ResolvedConfig, port: u16) -> Result<()> {
    #[cfg(unix)]
    install_terminal_guard();

    let (actual_port, handle) = start_daemon_server(resolved, port).await?;

    println!("\n  \x1b[1m\x1b[36mlukan web\x1b[0m");
    println!("  \x1b[2mWeb UI running at\x1b[0m \x1b[4mhttp://localhost:{actual_port}\x1b[0m\n");

    // Try to open browser
    let _ = open::that(format!("http://localhost:{actual_port}"));

    // Block until server exits
    handle.await??;

    Ok(())
}

/// Start the web server in the background without opening a browser.
/// Returns `(actual_port, join_handle)`. Useful for embedding in the daemon.
pub async fn start_daemon_server(
    resolved: ResolvedConfig,
    port: u16,
) -> Result<(u16, tokio::task::JoinHandle<Result<(), std::io::Error>>)> {
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
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let actual_port = listener.local_addr()?.port();
    tracing::info!("Web server listening on 0.0.0.0:{actual_port}");

    let handle = tokio::spawn(async move { axum::serve(listener, router).await });

    Ok((actual_port, handle))
}
