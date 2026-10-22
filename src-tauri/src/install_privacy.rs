//! The privacy page of the Windows installer (windows/hooks.nsh) and the app share two switches through
//! HKCU\Software\WellSnap:
//! - `CrashReports`, `UsageStats` (DWORD 0 or 1): the app's current choice, so the page starts from it.
//! - `InstallerCrashReports`, `InstallerUsageStats`: a switch the user changed on the page, applied once at
//!   the next start and deleted.
//!
//! macOS has no installer page; there these are no-ops.

#[cfg(any(windows, test))]
use crate::settings::CrashMode;
use crate::settings::Settings;
use tauri::AppHandle;

/// The installer's choice. A ticked crash switch keeps "ask"; only "never" turns into "always".
#[cfg(any(windows, test))]
fn apply(settings: &mut Settings, crash: Option<u32>, usage: Option<u32>) {
    match crash {
        Some(0) => settings.crash_reports = CrashMode::Never,
        Some(_) if settings.crash_reports == CrashMode::Never => settings.crash_reports = CrashMode::Always,
        _ => {}
    }
    if let Some(on) = usage {
        settings.usage_stats = on != 0;
    }
}

/// What the installer page shows for these settings.
#[cfg(any(windows, test))]
fn switches(settings: &Settings) -> (u32, u32) {
    (u32::from(settings.crash_reports != CrashMode::Never), u32::from(settings.usage_stats))
}

/// At start, before onboarding and telemetry::start: applies a choice the installer left, then records the
/// current one for the next install.
#[cfg(windows)]
pub fn take(app: &AppHandle) {
    use crate::printscreen::{OUR_KEY, delete_value, read_dword};
    use tauri::Manager;
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;
    use windows::core::w;

    let (crash_value, usage_value) = (w!("InstallerCrashReports"), w!("InstallerUsageStats"));
    let read = |value| {
        read_dword(HKEY_CURRENT_USER, OUR_KEY, value).unwrap_or_else(|err| {
            log::warn!("{err}");
            None
        })
    };
    let (crash, usage) = (read(crash_value), read(usage_value));
    let state = app.state::<crate::state::AppState>();
    if crash.is_some() || usage.is_some() {
        let before = state.settings();
        let after = state.update_settings(|s| apply(s, crash, usage));
        log::info!("installer privacy choice: crash reports {:?}, statistics {}", after.crash_reports, after.usage_stats);
        crate::telemetry::settings_changed(app, &before, &after);
        for value in [crash_value, usage_value] {
            if let Err(err) = delete_value(HKEY_CURRENT_USER, OUR_KEY, value) {
                log::warn!("{err}");
            }
        }
    }
    record(&state.settings());
}

#[cfg(not(windows))]
pub fn take(_app: &AppHandle) {}

/// Keeps the installer page in step with a changed choice.
#[cfg(windows)]
pub fn record(settings: &Settings) {
    use crate::printscreen::{OUR_KEY, write_dword};
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;
    use windows::core::w;

    let (crash, usage) = switches(settings);
    for (value, data) in [(w!("CrashReports"), crash), (w!("UsageStats"), usage)] {
        if let Err(err) = write_dword(HKEY_CURRENT_USER, OUR_KEY, value, data) {
            log::warn!("{err}");
        }
    }
}

#[cfg(not(windows))]
pub fn record(_settings: &Settings) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(crash_reports: CrashMode, usage_stats: bool) -> Settings {
        Settings { crash_reports, usage_stats, ..Default::default() }
    }

    fn applied(settings: Settings, crash: Option<u32>, usage: Option<u32>) -> (CrashMode, bool) {
        let mut settings = settings;
        apply(&mut settings, crash, usage);
        (settings.crash_reports, settings.usage_stats)
    }

    #[test]
    fn unticked_switches_turn_telemetry_off() {
        assert_eq!(applied(with(CrashMode::Always, true), Some(0), Some(0)), (CrashMode::Never, false));
        assert_eq!(applied(with(CrashMode::Ask, true), Some(0), None), (CrashMode::Never, true));
    }

    #[test]
    fn ticked_switches_turn_it_back_on_and_keep_ask() {
        assert_eq!(applied(with(CrashMode::Never, false), Some(1), Some(1)), (CrashMode::Always, true));
        assert_eq!(applied(with(CrashMode::Ask, false), Some(1), None), (CrashMode::Ask, false));
    }

    #[test]
    fn a_switch_the_user_did_not_change_is_left_alone() {
        for settings in [with(CrashMode::Never, false), with(CrashMode::Ask, true), with(CrashMode::Always, true)] {
            let expected = (settings.crash_reports, settings.usage_stats);
            assert_eq!(applied(settings, None, None), expected);
        }
    }

    #[test]
    fn the_page_shows_ask_as_on() {
        assert_eq!(switches(&with(CrashMode::Ask, false)), (1, 0));
        assert_eq!(switches(&with(CrashMode::Never, true)), (0, 1));
        assert_eq!(switches(&Settings::default()), (1, 1), "a new install starts with both on");
    }
}
