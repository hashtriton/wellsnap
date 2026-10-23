mod actions;
mod autostart;
mod capture;
mod clipboard;
mod commands;
mod conflicts;
mod ctrl_v;
mod donations;
mod export;
mod hotkeys;
mod i18n;
mod install_privacy;
#[cfg(target_os = "macos")]
mod macos;
mod notice;
mod overlay;
mod paste;
mod printscreen;
mod protocol;
mod session;
mod settings;
mod sheet;
mod state;
mod telemetry;
mod tray;
mod updates;
#[cfg(windows)]
mod webview_win;
mod windows_ui;

use state::AppState;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    let _telemetry = telemetry::early_init();
    let builder = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running instance instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows_ui::open_settings(app, None);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Dispatch(
                    telemetry::log_dispatch(),
                )))
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("wsnap", protocol::handle);
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(telemetry::web_content_terminated);
    builder
        .on_window_event(|window, event| {
            let app = window.app_handle();
            let label = window.label();
            // Closing onboarding with the title bar button counts as finishing it. The login item is registered
            // only now, after the user has seen the switch for it.
            if label == "onboarding" && matches!(event, tauri::WindowEvent::Destroyed) {
                let settings = app.state::<AppState>().update_settings(|s| s.onboarding_done = true);
                let _ = autostart::apply(app, settings.autostart);
                telemetry::onboarding_finished(app);
            }
            // Settings closed while the hotkey recorder had them suspended: the page's own resume never runs
            // when the webview is torn down.
            if label == "settings"
                && matches!(event, tauri::WindowEvent::Destroyed)
                && app.state::<AppState>().hotkeys_suspended.load(std::sync::atomic::Ordering::SeqCst)
            {
                hotkeys::resume(app);
            }
            if overlay::is_overlay_label(label) {
                overlay::on_window_event(app, label, event);
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let paste_root = app.path().app_cache_dir()?.join("paste");
            app.manage(AppState::new(settings::settings_path(&config_dir), paste_root.clone()));
            session::cleanup_old(&paste_root, None, session::MAX_FOLDER_AGE);

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::build(&handle)?;
            hotkeys::apply(&handle);
            conflicts::start_startup_check(&handle);
            let monitors = capture::monitors(&handle);
            overlay::ensure_windows(&handle, &monitors);
            // After the windows exist: the shareable-content query must list WellSnap to exclude its windows.
            #[cfg(target_os = "macos")]
            std::thread::spawn(capture::prewarm);
            overlay::start_idle_release(&handle);
            actions::start_cache_cleanup(&handle);

            let state = handle.state::<AppState>();
            // Before onboarding shows the switches and telemetry::start reads them.
            install_privacy::take(&handle);
            let settings = state.update_settings(|s| s.donations.ensure_initialised(chrono::Utc::now().timestamp()));
            autostart::sync(&handle, &settings);
            // Startup bookkeeping (the "updated" notice) and, in release builds, the update thread.
            updates::start(&handle);
            telemetry::start(&handle);
            if !settings.onboarding_done {
                windows_ui::open_onboarding(&handle);
            }
            #[cfg(windows)]
            if wants_print_screen(&settings) {
                let handle = handle.clone();
                // Off the main thread: the status reads the registry and scans the OneDrive settings folder.
                std::thread::spawn(move || restore_print_screen_take_over(&handle));
            }
            #[cfg(any(debug_assertions, feature = "bench"))]
            start_latency_bench(&handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::overlay_page_loaded,
            commands::overlay_ready,
            commands::overlay_cancel,
            commands::overlay_finish,
            commands::log_metric,
            commands::get_settings,
            commands::update_settings,
            commands::get_app_info,
            commands::hotkeys_suspend,
            commands::hotkeys_resume,
            commands::validate_hotkey,
            commands::hotkey_conflicts,
            commands::hotkeys_recheck,
            commands::get_set,
            commands::set_copy_all,
            commands::set_save_all,
            commands::sheet_put,
            commands::sheet_failed,
            commands::sheet_pending,
            commands::set_clear,
            commands::set_remove,
            commands::start_capture,
            commands::open_settings,
            commands::open_external,
            commands::reveal_path,
            commands::print_screen_status,
            commands::print_screen_take_over,
            commands::print_screen_restore,
            commands::donation_outcome,
            commands::donation_set_status,
            commands::donation_reset_counters,
            commands::notice_close,
            commands::notice_ready,
            commands::onboarding_complete,
            commands::request_screen_permission,
            commands::request_input_permission,
            commands::open_system_settings,
            commands::get_update,
            commands::check_update,
            commands::install_update,
            commands::hide_update_notes,
            commands::relaunch,
            commands::report_js_error,
            commands::telemetry_status,
            commands::telemetry_history,
            commands::crash_prompt_answer,
            commands::telemetry_reset_ids,
        ])
        .build(tauri::generate_context!())
        .expect("error while building WellSnap")
        .run(|_app, event| match event {
            // A tray app keeps running when its last window closes; only an explicit exit code quits.
            RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
            RunEvent::Exit => telemetry::shutdown(),
            _ => {}
        });
}

/// The user approved the Print Screen take-over and still uses Print Screen for captures.
#[cfg(any(windows, test))]
fn wants_print_screen(settings: &settings::Settings) -> bool {
    settings.onboarding_done
        && settings.print_screen_taken_over
        && settings.capture_hotkey.trim().eq_ignore_ascii_case("PrintScreen")
}

/// A reinstall that runs the old uninstaller without /UPDATE puts Snipping Tool back on Print Screen and
/// deletes WellSnap's registry marker, while the settings file survives. The user agreed once, so the
/// take-over is applied again silently.
#[cfg(any(windows, test))]
fn take_over_lost(status: &printscreen::PrintScreenStatus) -> bool {
    status.applicable && status.snipping_enabled && !status.changed_by_us
}

#[cfg(windows)]
fn restore_print_screen_take_over(app: &tauri::AppHandle) {
    if !take_over_lost(&printscreen::status()) {
        return;
    }
    match printscreen::take_over() {
        Ok(()) => {
            log::info!("Print Screen take-over applied again (lost after a reinstall)");
            hotkeys::apply(app);
        }
        Err(err) => log::warn!("could not apply the Print Screen take-over again: {err}"),
    }
}

/// Latency benchmark for debug builds and `--features bench`: `WELLSNAP_BENCH=5` runs five captures
/// (hotkey path without the key) and closes each overlay shortly after it appears; timings go to the log.
#[cfg(any(debug_assertions, feature = "bench"))]
fn start_latency_bench(app: &tauri::AppHandle) {
    use std::time::Duration;
    let Some(runs) = std::env::var("WELLSNAP_BENCH").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        for _ in 0..runs {
            overlay::begin_capture(&app);
            std::thread::sleep(Duration::from_millis(1500));
            overlay::hide_all(&app);
            std::thread::sleep(Duration::from_millis(1500));
        }
        log::info!("[bench] done");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use printscreen::PrintScreenStatus;

    #[test]
    fn print_screen_take_over_is_restored_only_when_agreed_and_lost() {
        let agreed = settings::Settings {
            onboarding_done: true,
            print_screen_taken_over: true,
            capture_hotkey: "printscreen".into(),
            ..Default::default()
        };
        assert!(wants_print_screen(&agreed));
        assert!(!wants_print_screen(&settings::Settings { onboarding_done: false, ..agreed.clone() }));
        assert!(!wants_print_screen(&settings::Settings { print_screen_taken_over: false, ..agreed.clone() }));
        assert!(!wants_print_screen(&settings::Settings { capture_hotkey: "Ctrl+PrintScreen".into(), ..agreed }));

        let lost = PrintScreenStatus { applicable: true, snipping_enabled: true, ..Default::default() };
        assert!(take_over_lost(&lost));
        assert!(!take_over_lost(&PrintScreenStatus { changed_by_us: true, ..lost.clone() }));
        assert!(!take_over_lost(&PrintScreenStatus { snipping_enabled: false, ..lost.clone() }));
        assert!(!take_over_lost(&PrintScreenStatus { applicable: false, ..lost }));
    }
}

