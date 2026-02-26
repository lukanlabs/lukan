#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

mod commands;
mod state;
mod terminal_state;

fn main() {
    // Catch GTK/display initialization failures and exit with a friendly message
    // instead of a panic backtrace.
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else if let Some(s) = info.payload().downcast_ref::<&str>() {
            s
        } else {
            "unknown error"
        };

        if msg.contains("gtk") || msg.contains("GTK") || msg.contains("display") {
            eprintln!(
                "Error: No graphical display available.\n\
                 lukan-desktop requires a desktop environment with X11 or Wayland.\n\
                 Use 'lukan chat' for TUI or 'lukan chat --ui web' for browser."
            );
        } else {
            eprintln!("Error: {msg}");
        }

        std::process::exit(1);
    }));

    tauri::Builder::default()
        .manage(state::ChatState::default())
        .manage(terminal_state::TerminalState::default())
        .setup(|app| {
            // Spawn background task to poll worker notifications and emit Tauri events
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use lukan_agent::NotificationWatcher;
                let mut watcher = NotificationWatcher::new();
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    for notif in watcher.poll().await {
                        let payload = serde_json::json!({
                            "workerId": notif.worker_id,
                            "workerName": notif.worker_name,
                            "status": notif.status,
                            "summary": notif.summary,
                        });
                        let _ = handle.emit("worker-notification", payload.to_string());
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Config
            commands::config::get_config,
            commands::config::save_config,
            commands::config::get_config_value,
            commands::config::set_config_value,
            // Credentials
            commands::credentials::get_credentials,
            commands::credentials::save_credentials,
            commands::credentials::get_provider_status,
            commands::credentials::test_provider,
            // Plugins
            commands::plugins::list_plugins,
            commands::plugins::install_plugin,
            commands::plugins::install_remote_plugin,
            commands::plugins::remove_plugin,
            commands::plugins::start_plugin,
            commands::plugins::stop_plugin,
            commands::plugins::restart_plugin,
            commands::plugins::get_plugin_config,
            commands::plugins::set_plugin_config_field,
            commands::plugins::get_plugin_logs,
            commands::plugins::list_remote_plugins,
            commands::plugins::get_whatsapp_qr,
            commands::plugins::check_whatsapp_auth,
            commands::plugins::fetch_whatsapp_groups,
            commands::plugins::get_plugin_commands,
            commands::plugins::run_plugin_command,
            commands::plugins::get_web_ui_status,
            commands::plugins::start_web_ui,
            commands::plugins::stop_web_ui,
            // Providers
            commands::providers::list_providers,
            commands::providers::get_models,
            commands::providers::fetch_provider_models,
            commands::providers::set_active_provider,
            commands::providers::add_model,
            commands::providers::set_provider_models,
            // Chat
            commands::chat::initialize_chat,
            commands::chat::send_message,
            commands::chat::cancel_stream,
            commands::chat::approve_tools,
            commands::chat::always_allow_tools,
            commands::chat::deny_all_tools,
            commands::chat::accept_plan,
            commands::chat::reject_plan,
            commands::chat::answer_question,
            commands::chat::list_sessions,
            commands::chat::load_session,
            commands::chat::new_session,
            commands::chat::set_permission_mode,
            // Terminal
            commands::terminal::terminal_create,
            commands::terminal::terminal_input,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_destroy,
            commands::terminal::terminal_list,
            // Memory
            commands::memory::get_global_memory,
            commands::memory::save_global_memory,
            commands::memory::get_project_memory,
            commands::memory::save_project_memory,
            commands::memory::is_project_memory_active,
            commands::memory::toggle_project_memory,
            // Browser
            commands::browser::browser_launch,
            commands::browser::browser_status,
            commands::browser::browser_navigate,
            commands::browser::browser_screenshot,
            commands::browser::browser_tabs,
            commands::browser::browser_close,
            // Workers
            commands::workers::list_workers,
            commands::workers::create_worker,
            commands::workers::update_worker,
            commands::workers::delete_worker,
            commands::workers::toggle_worker,
            commands::workers::get_worker_detail,
            commands::workers::get_worker_run,
            // Files
            commands::files::list_directory,
            commands::files::open_in_editor,
            commands::files::get_cwd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
