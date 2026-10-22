//! Start with the system: the Run value of the current user on Windows, a LaunchAgent on macOS.
//! Only release builds touch login items: a dev build would register the debug binary.

use crate::settings::Settings;
use crate::state::AppState;
use tauri::{AppHandle, Manager};

/// Turns the login item on or off and checks that the system kept the change.
pub fn apply(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    if enabled && running_from_temporary_location() {
        log::info!("not registering autostart from a temporary location (disk image or App Translocation)");
        return Ok(());
    }
    // Both calls are idempotent; enable also rewrites the entry with the current app path.
    let changed = if enabled { enable(app) } else { disable(app) };
    let result = changed.and_then(|()| match is_enabled(app)? {
        now if now == enabled => Ok(()),
        // A Run value written by an elevated run, or a policy, outweighs ours.
        _ => Err(format!("the system kept autostart {}", if enabled { "off" } else { "on" })),
    });
    result.inspect_err(|err| log::error!("autostart change failed: {err}"))
}

/// Startup. A missing login item is registered: a manual reinstall deletes it unless the old uninstaller runs
/// with /UPDATE. One that exists but is turned off in Task Manager or Windows Settings is the user's choice,
/// and the switch shows it instead of overriding it. On the first run the onboarding switch decides, applied
/// when onboarding closes.
pub fn sync(app: &AppHandle, settings: &Settings) {
    if cfg!(debug_assertions) {
        return;
    }
    let actual = match is_enabled(app) {
        Ok(actual) => actual,
        Err(err) => {
            log::warn!("could not read the autostart state: {err}");
            return;
        }
    };
    match decide(settings.autostart, actual, login_item_present(app), settings.onboarding_done) {
        Sync::Keep => {
            // The LaunchAgent check only sees that the plist exists; rewrite it so it points at the app as
            // it is now, which may have moved since.
            #[cfg(target_os = "macos")]
            if settings.autostart && actual {
                let _ = apply(app, true);
            }
        }
        Sync::Register => {
            log::info!("autostart entry is missing, registering it again");
            if apply(app, true).is_err() {
                // The switch shows what the system does; turning it on again shows the error.
                app.state::<AppState>().update_settings(|s| s.autostart = false);
            }
        }
        Sync::Adopt(actual) => {
            log::info!("autostart is {actual} in the system, adopting it");
            app.state::<AppState>().update_settings(|s| s.autostart = actual);
        }
    }
}

#[derive(Debug, PartialEq)]
enum Sync {
    Keep,
    Register,
    Adopt(bool),
}

fn decide(wanted: bool, actual: bool, present: bool, onboarding_done: bool) -> Sync {
    if wanted == actual || (wanted && !present && !onboarding_done) {
        Sync::Keep
    } else if wanted && !present {
        Sync::Register
    } else {
        Sync::Adopt(actual)
    }
}

/// Windows: own launcher in CurrentUser mode. The plugin's launcher uses the Dynamic mode, which writes the
/// machine-wide HKLM Run key (every account on the PC) whenever WellSnap happens to run elevated.
#[cfg(windows)]
fn launcher(app: &AppHandle) -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(&app.package_info().name)
        .set_app_path(&exe.display().to_string())
        .set_args(&["--autostart"])
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn enable(app: &AppHandle) -> Result<(), String> {
    launcher(app)?.enable().map_err(|e| e.to_string())
}

/// Also removes an HKLM value left by an elevated run, when this run is elevated too.
#[cfg(windows)]
fn disable(app: &AppHandle) -> Result<(), String> {
    launcher(app)?.disable().map_err(|e| e.to_string())
}

/// Registered in HKCU or HKLM and not turned off under StartupApproved.
#[cfg(windows)]
fn is_enabled(app: &AppHandle) -> Result<bool, String> {
    launcher(app)?.is_enabled().map_err(|e| e.to_string())
}

/// Whether the Run value exists at all, enabled or not: Task Manager keeps it when the user turns the app off
/// there and records that under StartupApproved instead.
#[cfg(windows)]
fn login_item_present(app: &AppHandle) -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_ANY, RegGetValueW};
    use windows::core::HSTRING;
    let key = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = HSTRING::from(app.package_info().name.as_str());
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .any(|root| unsafe { RegGetValueW(root, &key, &name, RRF_RT_ANY, None, None, None) } == ERROR_SUCCESS)
}

#[cfg(not(windows))]
fn enable(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().enable().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn disable(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().disable().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn is_enabled(app: &AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// macOS LaunchAgent: the plist exists or not; turning the app off in Login Items cannot be read back.
#[cfg(not(windows))]
fn login_item_present(app: &AppHandle) -> bool {
    is_enabled(app).unwrap_or(false)
}

/// Running from a mounted disk image or a Gatekeeper translocation folder: a login item written now would
/// point at a path that is gone after the next eject or reboot.
#[cfg(target_os = "macos")]
fn running_from_temporary_location() -> bool {
    std::env::current_exe().and_then(|p| p.canonicalize()).is_ok_and(|p| is_temporary_path(&p.to_string_lossy()))
}

#[cfg(any(target_os = "macos", test))]
fn is_temporary_path(path: &str) -> bool {
    path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_entry_is_registered_and_a_turned_off_one_is_adopted() {
        assert_eq!(decide(true, true, true, true), Sync::Keep);
        assert_eq!(decide(false, false, false, true), Sync::Keep);
        // A reinstall whose old uninstaller deleted the Run value.
        assert_eq!(decide(true, false, false, true), Sync::Register);
        // Turned off in Task Manager: the value stays, marked disabled.
        assert_eq!(decide(true, false, true, true), Sync::Adopt(false));
        assert_eq!(decide(false, true, true, true), Sync::Adopt(true));
    }

    #[test]
    fn first_run_waits_for_the_onboarding_switch() {
        assert_eq!(decide(true, false, false, false), Sync::Keep);
        // Turned off in Task Manager before onboarding ended still counts.
        assert_eq!(decide(true, false, true, false), Sync::Adopt(false));
    }

    #[test]
    fn disk_images_and_translocation_are_temporary() {
        assert!(is_temporary_path("/Volumes/WellSnap/WellSnap.app/Contents/MacOS/WellSnap"));
        assert!(is_temporary_path("/private/var/folders/x/T/AppTranslocation/ABC/d/WellSnap.app/Contents/MacOS/WellSnap"));
        assert!(!is_temporary_path("/Applications/WellSnap.app/Contents/MacOS/WellSnap"));
    }
}
