#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

mod commands;
mod state;
mod terminal_state;

mod cli_install {
    use std::path::{Path, PathBuf};

    /// Search order for the bundled CLI binary relative to the desktop executable dir.
    const BUNDLE_RESOURCE_PATHS: &[&str] = &[
        "../Resources/lukan",         // macOS .app: Contents/MacOS/../Resources/
        "../lib/Lukan Desktop/lukan", // Linux .deb: /usr/bin/../lib/Lukan Desktop/
    ];

    /// Check if `lukan` is already available somewhere in the given PATH dirs.
    pub fn cli_in_path(path_dirs: &[PathBuf]) -> bool {
        path_dirs.iter().any(|dir| {
            let candidate = dir.join("lukan");
            candidate.is_file() || candidate.is_symlink()
        })
    }

    /// Find the bundled CLI binary relative to the executable directory.
    ///
    /// Returns `None` if:
    /// - No bundled CLI exists (dev build without sibling binary)
    /// - The CLI is next to the exe AND that dir is already in PATH (curl install)
    pub fn find_bundled_cli(exe_dir: &Path, path_dirs: &[PathBuf]) -> Option<PathBuf> {
        // 1. Next to our binary
        let beside = exe_dir.join("lukan");
        if beside.exists() {
            // If exe_dir is in PATH, user already has access (curl install) — skip
            if path_dirs.iter().any(|p| p == exe_dir) {
                return None;
            }
            return Some(beside);
        }

        // 2. Tauri bundle resource paths
        for relative in BUNDLE_RESOURCE_PATHS {
            let candidate = exe_dir.join(relative);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }

    /// Determine the directory where the CLI symlink should be created.
    pub fn symlink_dir(home: Option<&Path>) -> PathBuf {
        if cfg!(target_os = "macos") {
            PathBuf::from("/usr/local/bin")
        } else {
            home.map(|h| h.join(".local/bin"))
                .unwrap_or_else(|| PathBuf::from("/usr/local/bin"))
        }
    }

    /// Whether we're running inside an AppImage (temporary mount, paths vanish on exit).
    fn is_appimage() -> bool {
        std::env::var_os("APPIMAGE").is_some()
    }

    /// Top-level: find bundled CLI, check PATH, install if needed.
    /// AppImage: copies the binary (symlinks would break when mount disappears).
    /// .deb / .app: creates a symlink.
    pub fn install_cli_symlink() {
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(_) => return,
        };
        let exe_dir = match exe.parent() {
            Some(d) => d,
            None => return,
        };

        let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();

        if cli_in_path(&path_dirs) {
            // For AppImage, verify the existing binary isn't a stale symlink
            if is_appimage() {
                let existing = path_dirs.iter().find_map(|dir| {
                    let candidate = dir.join("lukan");
                    if candidate.is_symlink() && !candidate.exists() {
                        Some(candidate)
                    } else {
                        None
                    }
                });
                if let Some(stale) = existing {
                    let _ = std::fs::remove_file(&stale);
                } else {
                    return;
                }
            } else {
                return;
            }
        }

        let cli_bin = match find_bundled_cli(exe_dir, &path_dirs) {
            Some(p) => p,
            None => return,
        };

        let home = std::env::var_os("HOME").map(PathBuf::from);
        let link_dir = symlink_dir(home.as_deref());

        if std::fs::create_dir_all(&link_dir).is_err() {
            return;
        }

        let dest_path = link_dir.join("lukan");

        if is_appimage() {
            // Copy the binary so it persists after AppImage unmounts
            let _ = std::fs::copy(&cli_bin, &dest_path);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o755));
            }
        } else {
            // Symlink for persistent installs (.deb, .app)
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(&cli_bin, &dest_path);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::sync::atomic::{AtomicU32, Ordering};

        static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

        fn make_temp(name: &str) -> PathBuf {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "lukan-cli-test-{}-{}-{}",
                std::process::id(),
                id,
                name
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn cli_in_path_finds_existing_binary() {
            let tmp = make_temp("in-path-found");
            fs::write(tmp.join("lukan"), "fake").unwrap();
            assert!(cli_in_path(&[tmp.clone()]));
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn cli_in_path_returns_false_when_missing() {
            let tmp = make_temp("in-path-missing");
            assert!(!cli_in_path(&[tmp.clone()]));
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[cfg(unix)]
        #[test]
        fn cli_in_path_detects_symlink() {
            let tmp = make_temp("in-path-symlink");
            let real = tmp.join("lukan-real");
            fs::write(&real, "fake").unwrap();
            std::os::unix::fs::symlink(&real, tmp.join("lukan")).unwrap();

            assert!(cli_in_path(&[tmp.clone()]));
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn find_bundled_beside_exe_not_in_path() {
            let tmp = make_temp("beside-not-path");
            fs::write(tmp.join("lukan"), "fake").unwrap();

            let result = find_bundled_cli(&tmp, &[PathBuf::from("/some/other/dir")]);
            assert_eq!(result, Some(tmp.join("lukan")));
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn find_bundled_beside_exe_in_path_returns_none() {
            let tmp = make_temp("beside-in-path");
            fs::write(tmp.join("lukan"), "fake").unwrap();

            let result = find_bundled_cli(&tmp, &[tmp.clone()]);
            assert!(result.is_none());
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn find_bundled_in_macos_resources() {
            let tmp = make_temp("macos-resources");
            let macos_dir = tmp.join("Contents/MacOS");
            let resources_dir = tmp.join("Contents/Resources");
            fs::create_dir_all(&macos_dir).unwrap();
            fs::create_dir_all(&resources_dir).unwrap();
            fs::write(resources_dir.join("lukan"), "fake").unwrap();

            let result = find_bundled_cli(&macos_dir, &[]);
            assert!(result.is_some());
            assert!(result.unwrap().ends_with("Resources/lukan"));
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn find_bundled_in_deb_lib_dir() {
            // .deb layout: exe at /usr/bin/lukan-desktop
            //              cli at /usr/lib/Lukan Desktop/lukan
            // relative from /usr/bin: ../lib/Lukan Desktop/lukan
            let tmp = make_temp("deb-lib");
            let bin_dir = tmp.join("usr/bin");
            let lib_dir = tmp.join("usr/lib/Lukan Desktop");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::create_dir_all(&lib_dir).unwrap();
            fs::write(lib_dir.join("lukan"), "fake").unwrap();

            let result = find_bundled_cli(&bin_dir, &[]);
            assert!(result.is_some());
            assert!(
                result.unwrap().ends_with("Lukan Desktop/lukan"),
                "should find CLI in lib dir"
            );
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn find_bundled_returns_none_when_missing() {
            let tmp = make_temp("nothing-exists");
            let result = find_bundled_cli(&tmp, &[]);
            assert!(result.is_none());
            fs::remove_dir_all(&tmp).unwrap();
        }

        #[test]
        fn symlink_dir_linux_with_home() {
            if cfg!(target_os = "macos") {
                return;
            }
            let home = PathBuf::from("/home/testuser");
            assert_eq!(
                symlink_dir(Some(&home)),
                PathBuf::from("/home/testuser/.local/bin")
            );
        }

        #[test]
        fn symlink_dir_without_home_fallback() {
            if cfg!(target_os = "macos") {
                return;
            }
            assert_eq!(symlink_dir(None), PathBuf::from("/usr/local/bin"));
        }

        #[cfg(unix)]
        #[test]
        fn full_symlink_creation() {
            let tmp = make_temp("full-symlink");
            let exe_dir = tmp.join("app");
            let link_dir = tmp.join("bin");
            fs::create_dir_all(&exe_dir).unwrap();
            fs::create_dir_all(&link_dir).unwrap();
            fs::write(exe_dir.join("lukan"), "fake-cli").unwrap();

            let cli_bin = find_bundled_cli(&exe_dir, &[]).unwrap();
            let link_path = link_dir.join("lukan");
            std::os::unix::fs::symlink(&cli_bin, &link_path).unwrap();

            assert!(link_path.is_symlink());
            assert_eq!(fs::read_to_string(&link_path).unwrap(), "fake-cli");
            fs::remove_dir_all(&tmp).unwrap();
        }
    }
}

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
            // Install CLI symlink if bundled binary exists but isn't in PATH
            cli_install::install_cli_symlink();

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
            commands::config::list_tools,
            // Credentials
            commands::credentials::get_credentials,
            commands::credentials::save_credentials,
            commands::credentials::get_provider_status,
            commands::credentials::test_provider,
            // Plugins
            commands::plugins::list_plugins,
            commands::plugins::get_plugin_view_data,
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
            commands::plugins::get_plugin_auth_qr,
            commands::plugins::check_plugin_auth,
            commands::plugins::get_plugin_manifest_info,
            commands::plugins::get_plugin_commands,
            commands::plugins::run_plugin_command,
            commands::plugins::get_plugin_manifest_tools,
            commands::plugins::get_web_ui_status,
            commands::plugins::start_web_ui,
            commands::plugins::stop_web_ui,
            commands::plugins::check_transcription_status,
            commands::plugins::transcribe_audio,
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
            commands::chat::delete_session,
            commands::chat::delete_all_sessions,
            commands::chat::load_session,
            commands::chat::new_session,
            commands::chat::set_permission_mode,
            commands::chat::list_tasks,
            commands::chat::get_daemon_port,
            commands::chat::create_agent_tab,
            commands::chat::destroy_agent_tab,
            commands::chat::rename_agent_tab,
            commands::chat::load_agent_tabs,
            commands::chat::save_agent_tabs,
            // Terminal
            commands::terminal::terminal_create,
            commands::terminal::terminal_input,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_destroy,
            commands::terminal::terminal_list,
            commands::terminal::terminal_rename,
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
            // Background processes
            commands::bg_processes::list_bg_processes,
            commands::bg_processes::get_bg_process_log,
            commands::bg_processes::kill_bg_process,
            commands::bg_processes::clear_bg_processes,
            commands::bg_processes::send_to_background,
            // Events
            commands::events::consume_pending_events,
            commands::events::get_event_history,
            commands::events::clear_event_history,
            // Audio recording
            commands::audio::start_recording,
            commands::audio::stop_recording,
            commands::audio::cancel_recording,
            commands::audio::is_recording,
            commands::audio::list_audio_devices,
            // Files
            commands::files::list_directory,
            commands::files::read_file,
            commands::files::write_file,
            commands::files::open_in_editor,
            commands::files::get_cwd,
            commands::files::open_url,
            // Project
            commands::files::set_project_cwd,
            commands::files::get_recent_projects,
            commands::files::add_recent_project,
            commands::files::pick_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
