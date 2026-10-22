//! Tray (Windows) / menu bar (macOS) icon. On Windows a left click starts a capture, like Joxi/Lightshot;
//! on macOS a click opens the menu, as menu bar apps do.

use crate::donations::DonationStatus;
use crate::i18n::{Lang, tr};
use crate::state::{AppState, HotkeyStatus};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "main";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = make_menu(app)?;
    let icon = app.default_window_icon().cloned().ok_or_else(|| tauri::Error::AssetNotFound("tray icon".into()))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(tr(app.state::<AppState>().lang(), "tray.tooltip"))
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => crate::overlay::begin_capture(app),
            "fix" => crate::windows_ui::open_settings(app, Some("hotkeys")),
            "strip" => crate::windows_ui::sync_strip(app),
            "settings" => crate::windows_ui::open_settings(app, None),
            "support" => crate::windows_ui::open_settings(app, Some("support")),
            "about" => crate::windows_ui::open_settings(app, Some("about")),
            "update" => crate::updates::install_from_tray(app),
            // May first install a waiting update, on a worker thread (updates::quit).
            "quit" => crate::updates::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if cfg!(not(target_os = "macos"))
                && let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event
            {
                crate::overlay::begin_capture(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuilds the menu after a language change, a donation status change, a set change or when the capture
/// hotkey starts or stops working.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    match make_menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(err) => log::error!("tray menu rebuild failed: {err}"),
    }
}

fn make_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let settings = state.settings();
    let has_set = state.session.lock().unwrap().as_ref().is_some_and(|s| !s.is_empty());
    let support_label = if settings.donations.status == DonationStatus::Supporter {
        tr(lang, "tray.supported")
    } else {
        tr(lang, "tray.support")
    };
    let off = capture_off(&state.hotkeys.lock().unwrap());
    let capture_label = capture_label(lang, &settings.capture_hotkey, !off);

    let capture = MenuItem::with_id(app, "capture", capture_label, true, None::<&str>)?;
    let fix = if off {
        let label = tr(lang, "tray.fixHotkey").replace("{hotkey}", &display_hotkey(&settings.capture_hotkey));
        Some(MenuItem::with_id(app, "fix", label, true, None::<&str>)?)
    } else {
        None
    };
    let strip = MenuItem::with_id(app, "strip", tr(lang, "tray.strip"), has_set, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", tr(lang, "tray.settings"), true, None::<&str>)?;
    let support = MenuItem::with_id(app, "support", support_label, true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", tr(lang, "tray.about"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr(lang, "tray.quit"), true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    // Never hold an AppState lock across a menu call: those wait for the main thread, which may be waiting
    // for the same lock (get_update).
    let update_version = state.update.lock().unwrap().offered().map(|info| info.version.clone());
    let update = match update_version {
        Some(version) => {
            let label = tr(lang, "tray.update").replace("{version}", &version);
            Some(MenuItem::with_id(app, "update", label, true, None::<&str>)?)
        }
        None => None,
    };
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&capture];
    if let Some(fix) = &fix {
        items.push(fix);
    }
    items.extend([&strip as &dyn IsMenuItem<Wry>, &sep1, &settings_item, &support, &about]);
    if let Some(update) = &update {
        items.push(update);
    }
    items.extend([&sep2 as &dyn IsMenuItem<Wry>, &quit]);
    Menu::with_items(app, &items)
}

/// The capture hotkey failed to register. False while nothing was tried: before the first registration and
/// while Settings records a new key (the status has no error then).
pub fn capture_off(status: &HotkeyStatus) -> bool {
    !status.capture_registered && status.capture_error.is_some()
}

/// "Take screenshot (PrtSc)", or a label that says the key does not work.
pub fn capture_label(lang: Lang, hotkey: &str, registered: bool) -> String {
    let hotkey = display_hotkey(hotkey);
    if registered {
        format!("{} ({hotkey})", tr(lang, "tray.capture"))
    } else {
        tr(lang, "tray.captureOff").replace("{hotkey}", &hotkey)
    }
}

/// Short human form of an accelerator for menus: "PrintScreen" -> "PrtSc", "Ctrl+Shift+2" stays readable.
pub fn display_hotkey(accel: &str) -> String {
    accel
        .split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "printscreen" => "PrtSc".to_string(),
            "cmd" | "command" | "super" => if cfg!(target_os = "macos") { "⌘".to_string() } else { "Win".to_string() },
            "ctrl" | "control" => "Ctrl".to_string(),
            "alt" | "option" => if cfg!(target_os = "macos") { "⌥".to_string() } else { "Alt".to_string() },
            "shift" => "Shift".to_string(),
            _ => part.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    #[test]
    fn hotkey_display() {
        assert_eq!(super::display_hotkey("PrintScreen"), "PrtSc");
        assert_eq!(super::display_hotkey("Ctrl+Alt+V"), if cfg!(target_os = "macos") { "Ctrl+⌥+V" } else { "Ctrl+Alt+V" });
        assert_eq!(super::display_hotkey("Super+Alt+V"), if cfg!(target_os = "macos") { "⌘+⌥+V" } else { "Win+Alt+V" });
    }

    #[test]
    fn capture_label_says_when_the_key_does_not_work() {
        use super::capture_label;
        use crate::i18n::Lang;
        assert_eq!(capture_label(Lang::En, "PrintScreen", true), "Take screenshot (PrtSc)");
        assert_eq!(capture_label(Lang::Ru, "PrintScreen", true), "Сделать скриншот (PrtSc)");
        assert_eq!(capture_label(Lang::En, "PrintScreen", false), "Take screenshot (PrtSc is not active)");
        assert_eq!(capture_label(Lang::Ru, "Ctrl+PrintScreen", false), "Сделать скриншот (Ctrl+PrtSc не работает)");
    }

    #[test]
    fn capture_is_off_only_after_a_failed_registration() {
        use super::capture_off;
        use crate::state::HotkeyStatus;
        assert!(capture_off(&HotkeyStatus { capture_error: Some("reserved".into()), ..Default::default() }));
        assert!(!capture_off(&HotkeyStatus { capture_registered: true, ..Default::default() }));
        // Before the first registration and while Settings records a key.
        assert!(!capture_off(&HotkeyStatus::default()));
    }
}
