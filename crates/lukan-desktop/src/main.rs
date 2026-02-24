#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

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
            // Memory
            commands::memory::get_global_memory,
            commands::memory::save_global_memory,
            commands::memory::get_project_memory,
            commands::memory::save_project_memory,
            commands::memory::is_project_memory_active,
            commands::memory::toggle_project_memory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
