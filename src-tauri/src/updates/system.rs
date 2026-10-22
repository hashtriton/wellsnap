//! What the updater asks the OS: how long the user has been away, whether something runs full screen, and
//! whether this copy may replace itself.

#[cfg(target_os = "macos")]
use std::path::Path;
use std::time::Duration;

/// Time since the last keyboard or mouse input. Hardware input only on macOS: WellSnap's own synthetic paste
/// goes through another event source.
pub fn idle() -> Option<Duration> {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return None;
        }
        let now = unsafe { GetTickCount() };
        Some(super::policy::idle_from_ticks(now, info.dwTime))
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
        // kCGAnyInputEventType is ~0.
        let secs = CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::HIDSystemState, CGEventType(u32::MAX));
        (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    None
}

/// Windows: a full-screen game or video, a presentation, or "busy" (the state Focus assist also uses).
pub fn fullscreen_busy() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
        };
        match unsafe { SHQueryUserNotificationState() } {
            Ok(state) => [QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE].contains(&state),
            Err(err) => {
                log::warn!("SHQueryUserNotificationState: {err}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    false
}

/// An installed copy that can replace itself without asking: not a dev build or a benchmark run; on Windows
/// the installer's uninstall.exe sits next to the exe; on macOS the bundle is not translocated, not on a disk
/// image and its folder is writable (otherwise the updater would ask for an admin password).
pub fn installed_copy() -> bool {
    if cfg!(feature = "bench") || std::env::var_os("WELLSNAP_BENCH").is_some() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    #[cfg(windows)]
    {
        exe.parent().is_some_and(|dir| dir.join("uninstall.exe").is_file())
    }
    #[cfg(target_os = "macos")]
    {
        let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) else { return false };
        mac_bundle_location_ok(&bundle.to_string_lossy()) && bundle.parent().is_some_and(writable)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = exe;
        false
    }
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn mac_bundle_location_ok(bundle: &str) -> bool {
    !bundle.contains("/AppTranslocation/") && !bundle.starts_with("/Volumes/")
}

#[cfg(target_os = "macos")]
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".wellsnap-update-probe-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The updater plugin keeps its "WellSnap-<version>-updater-*" folders in %TEMP% on purpose (the installer
/// runs from there after the app exits); old ones are removed at the next start.
#[cfg(windows)]
pub fn remove_old_installer_dirs(app_name: &str, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_installer_dir(&name, app_name) {
            continue;
        }
        let old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_dir())
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if old && let Err(err) = std::fs::remove_dir_all(entry.path()) {
            log::warn!("could not remove {}: {err}", entry.path().display());
        }
    }
}

#[cfg(any(windows, test))]
fn is_installer_dir(name: &str, app_name: &str) -> bool {
    name.strip_prefix(app_name).and_then(|rest| rest.strip_prefix('-')).is_some_and(|rest| rest.contains("-updater-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_dirs_are_recognized() {
        assert!(is_installer_dir("WellSnap-0.2.0-updater-a1B2c3", "WellSnap"));
        assert!(!is_installer_dir("WellSnap-0.2.0-installer.exe", "WellSnap"));
        assert!(!is_installer_dir("Other-0.2.0-updater-x", "WellSnap"));
        assert!(!is_installer_dir("WellSnapX-0.2.0-updater-x", "WellSnap"));
    }

    #[test]
    fn mac_bundles_on_images_or_translocated_are_not_updated() {
        assert!(mac_bundle_location_ok("/Applications/WellSnap.app"));
        assert!(mac_bundle_location_ok("/Users/a/Applications/WellSnap.app"));
        assert!(!mac_bundle_location_ok("/Volumes/WellSnap/WellSnap.app"));
        assert!(!mac_bundle_location_ok("/private/var/folders/x/AppTranslocation/ABC/d/WellSnap.app"));
    }
}
