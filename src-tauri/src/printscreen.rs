//! Windows 11 routes the bare PrintScreen key to Snipping Tool by default
//! (HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled). Like ShareX and Greenshot, WellSnap
//! turns it off only after the user clicks a button, remembers the previous value and can restore it.
//! Other platforms report "not applicable".
//!
//! What WellSnap stores in HKCU\Software\WellSnap (the NSIS uninstall hook in windows/hooks.nsh reads the same):
//! - `PrintScreenTakenOver` (DWORD 1): WellSnap changed the setting; nothing to restore without it.
//! - `PrevPrintScreenKeyForSnippingEnabled` (DWORD): the value before the change; missing when the value
//!   itself was missing, so restoring deletes it again. Only meaningful while the marker is set.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintScreenStatus {
    /// False on macOS and Windows 10 without the setting.
    pub applicable: bool,
    /// The OS opens Snipping Tool on PrintScreen (a missing value on Windows 11 counts as enabled).
    pub snipping_enabled: bool,
    /// OneDrive "Automatically save screenshots" also grabs PrintScreen.
    pub onedrive_enabled: bool,
    /// WellSnap changed the setting and stored the previous value.
    pub changed_by_us: bool,
}

#[cfg(windows)]
pub use platform::{restore, status, take_over};
/// Also used for the installer's privacy choice (install_privacy.rs), in the same key.
#[cfg(windows)]
pub(crate) use platform::{OUR_KEY, delete_value, read_dword, write_dword};

#[cfg(not(windows))]
pub fn status() -> PrintScreenStatus {
    PrintScreenStatus::default()
}

/// Writes 0, stores the previous value, broadcasts WM_SETTINGCHANGE. Caller re-registers hotkeys.
#[cfg(not(windows))]
pub fn take_over() -> Result<(), String> {
    Err("not available on this platform".into())
}

/// Puts back the value that was there before `take_over` (or removes it if there was none).
#[cfg(not(windows))]
pub fn restore() -> Result<(), String> {
    Err("not available on this platform".into())
}

/// First Windows 11 build.
#[cfg(any(windows, test))]
const WINDOWS_11_BUILD: u32 = 22000;

/// (applicable, snipping_enabled) from the registry value and the OS build. A missing value means the OS
/// default: on (Windows 11) or a feature the user never turned on (Windows 10).
#[cfg(any(windows, test))]
fn snipping_state(value: Option<u32>, build: Option<u32>) -> (bool, bool) {
    match value {
        Some(v) => (true, v != 0),
        None if build.is_some_and(|b| b >= WINDOWS_11_BUILD) => (true, true),
        None => (false, false),
    }
}

/// What `take_over` remembers: `Some(previous)` on the first take-over, None when WellSnap already owns the
/// setting (storing the current 0 would lose the user's original value).
#[cfg(any(windows, test))]
fn previous_to_store(marker: bool, current: Option<u32>) -> Option<Option<u32>> {
    (!marker).then_some(current)
}

#[cfg_attr(test, derive(Debug, PartialEq))]
#[cfg(any(windows, test))]
enum RestoreAction {
    Nothing,
    Set(u32),
    Delete,
}

#[cfg(any(windows, test))]
fn restore_action(marker: bool, previous: Option<u32>) -> RestoreAction {
    match (marker, previous) {
        (false, _) => RestoreAction::Nothing,
        (true, Some(v)) => RestoreAction::Set(v),
        (true, None) => RestoreAction::Delete,
    }
}

/// HKCU\Software\Microsoft\OneDrive\AutoSaveScreenshots, a DWORD or a string, as Greenshot reads it.
#[cfg_attr(test, derive(Debug))]
#[cfg(any(windows, test))]
enum RegData {
    Dword(u32),
    Text(String),
}

#[cfg(any(windows, test))]
fn onedrive_value_blocking(value: &RegData) -> bool {
    match value {
        RegData::Dword(v) => *v == 1,
        RegData::Text(s) => s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"),
    }
}

/// OneDrive keeps the screenshot setting in %LOCALAPPDATA%\Microsoft\OneDrive\settings\...\*_screenshot.dat;
/// "2" on the second line means it grabs PrintScreen (Greenshot, HotkeySectionViewModel.CheckOneDriveStatus).
#[cfg(any(windows, test))]
fn onedrive_dat_blocking(bytes: &[u8]) -> bool {
    let text = match bytes {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(
            &rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>(),
        ),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    text.lines().nth(1).is_some_and(|line| line.trim() == "2")
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::path::Path;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, LPARAM, WIN32_ERROR, WPARAM};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_DWORD, REG_ROUTINE_FLAGS, REG_SZ, REG_VALUE_TYPE,
        RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    use windows::core::{PCWSTR, w};

    const KEYBOARD_KEY: PCWSTR = w!("Control Panel\\Keyboard");
    const SNIPPING_VALUE: PCWSTR = w!("PrintScreenKeyForSnippingEnabled");
    pub(crate) const OUR_KEY: PCWSTR = w!("Software\\WellSnap");
    const MARKER_VALUE: PCWSTR = w!("PrintScreenTakenOver");
    const PREVIOUS_VALUE: PCWSTR = w!("PrevPrintScreenKeyForSnippingEnabled");

    pub fn status() -> PrintScreenStatus {
        let value = read_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, SNIPPING_VALUE).unwrap_or_else(|e| {
            log::warn!("PrintScreenKeyForSnippingEnabled: {e}");
            None
        });
        let (applicable, snipping_enabled) = snipping_state(value, windows_build());
        PrintScreenStatus {
            applicable,
            snipping_enabled,
            onedrive_enabled: onedrive_enabled(),
            changed_by_us: matches!(read_dword(HKEY_CURRENT_USER, OUR_KEY, MARKER_VALUE), Ok(Some(_))),
        }
    }

    pub fn take_over() -> Result<(), String> {
        let current = read_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, SNIPPING_VALUE)?;
        let marker = read_dword(HKEY_CURRENT_USER, OUR_KEY, MARKER_VALUE)?.is_some();
        // The previous value is saved before anything changes, so a failure here leaves the setting untouched.
        if let Some(previous) = previous_to_store(marker, current) {
            match previous {
                Some(v) => write_dword(HKEY_CURRENT_USER, OUR_KEY, PREVIOUS_VALUE, v)?,
                None => delete_value(HKEY_CURRENT_USER, OUR_KEY, PREVIOUS_VALUE)?,
            }
            write_dword(HKEY_CURRENT_USER, OUR_KEY, MARKER_VALUE, 1)?;
        }
        write_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, SNIPPING_VALUE, 0)?;
        broadcast_keyboard_change();
        Ok(())
    }

    pub fn restore() -> Result<(), String> {
        let marker = read_dword(HKEY_CURRENT_USER, OUR_KEY, MARKER_VALUE)?.is_some();
        let previous = read_dword(HKEY_CURRENT_USER, OUR_KEY, PREVIOUS_VALUE)?;
        match restore_action(marker, previous) {
            RestoreAction::Nothing => return Ok(()),
            RestoreAction::Set(v) => write_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, SNIPPING_VALUE, v)?,
            RestoreAction::Delete => delete_value(HKEY_CURRENT_USER, KEYBOARD_KEY, SNIPPING_VALUE)?,
        }
        broadcast_keyboard_change();
        // Marker first: a marker left without the previous value would make a later restore delete the setting.
        // A previous value left without the marker is ignored, and take_over rewrites it before setting the marker.
        delete_value(HKEY_CURRENT_USER, OUR_KEY, MARKER_VALUE)?;
        if let Err(err) = delete_value(HKEY_CURRENT_USER, OUR_KEY, PREVIOUS_VALUE) {
            log::warn!("{err}");
        }
        Ok(())
    }

    fn windows_build() -> Option<u32> {
        let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        match read_value(HKEY_LOCAL_MACHINE, key, w!("CurrentBuildNumber"), RRF_RT_REG_SZ) {
            Ok(Some(RegData::Text(s))) => s.trim().parse().ok(),
            _ => None,
        }
    }

    fn onedrive_enabled() -> bool {
        let value = read_value(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\OneDrive"),
            w!("AutoSaveScreenshots"),
            RRF_RT_REG_DWORD | RRF_RT_REG_SZ,
        );
        if matches!(value, Ok(Some(ref v)) if onedrive_value_blocking(v)) {
            return true;
        }
        std::env::var_os("LOCALAPPDATA")
            .is_some_and(|dir| any_screenshot_dat_blocking(&Path::new(&dir).join("Microsoft\\OneDrive\\settings"), 4))
    }

    fn any_screenshot_dat_blocking(dir: &Path, depth: u32) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else { return false };
        entries.flatten().any(|entry| {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { return false };
            if kind.is_dir() {
                depth > 0 && any_screenshot_dat_blocking(&path, depth - 1)
            } else {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().ends_with("_screenshot.dat"))
                    && std::fs::read(&path).is_ok_and(|bytes| onedrive_dat_blocking(&bytes))
            }
        })
    }

    pub(crate) fn read_dword(root: HKEY, key: PCWSTR, value: PCWSTR) -> Result<Option<u32>, String> {
        match read_value(root, key, value, RRF_RT_REG_DWORD)? {
            Some(RegData::Dword(v)) => Ok(Some(v)),
            Some(RegData::Text(_)) | None => Ok(None),
        }
    }

    /// A DWORD or a string of up to 255 characters; Ok(None) when the key or the value does not exist.
    fn read_value(root: HKEY, key: PCWSTR, value: PCWSTR, flags: REG_ROUTINE_FLAGS) -> Result<Option<RegData>, String> {
        let mut kind = REG_VALUE_TYPE(0);
        let mut buf = [0u16; 256];
        let mut size = size_of_val(&buf) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                key,
                value,
                flags,
                Some(&mut kind),
                Some(buf.as_mut_ptr().cast::<c_void>()),
                Some(&mut size),
            )
        };
        match status {
            ERROR_SUCCESS if kind == REG_DWORD => {
                Ok(Some(RegData::Dword(u32::from(buf[0]) | (u32::from(buf[1]) << 16))))
            }
            ERROR_SUCCESS if kind == REG_SZ => {
                let units = &buf[..(size as usize / 2).min(buf.len())];
                let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
                Ok(Some(RegData::Text(String::from_utf16_lossy(&units[..end]))))
            }
            ERROR_SUCCESS => Ok(None),
            ERROR_FILE_NOT_FOUND => Ok(None),
            err => Err(format!("registry read {}: {}", unsafe { value.display() }, win32_message(err))),
        }
    }

    pub(crate) fn write_dword(root: HKEY, key: PCWSTR, value: PCWSTR, data: u32) -> Result<(), String> {
        // Creates the key when it does not exist.
        let status = unsafe {
            RegSetKeyValueW(
                root,
                key,
                value,
                REG_DWORD.0,
                Some((&data as *const u32).cast::<c_void>()),
                size_of::<u32>() as u32,
            )
        };
        match status {
            ERROR_SUCCESS => Ok(()),
            err => Err(format!("registry write {}: {}", unsafe { value.display() }, win32_message(err))),
        }
    }

    pub(crate) fn delete_value(root: HKEY, key: PCWSTR, value: PCWSTR) -> Result<(), String> {
        match unsafe { RegDeleteKeyValueW(root, key, value) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            err => Err(format!("registry delete {}: {}", unsafe { value.display() }, win32_message(err))),
        }
    }

    fn win32_message(err: WIN32_ERROR) -> String {
        windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(err.0)).message()
    }

    /// Explorer and the keyboard settings re-read the value on this broadcast, as after a change in Settings.
    fn broadcast_keyboard_change() {
        let area = w!("Control Panel\\Keyboard");
        let delivered = unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(area.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                1000,
                None,
            )
        };
        if delivered.0 == 0 {
            log::warn!("WM_SETTINGCHANGE broadcast: {}", windows::core::Error::from_thread());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN10: Option<u32> = Some(19045);
    const WIN11: Option<u32> = Some(26100);

    #[test]
    fn snipping_value_present_wins_on_both_versions() {
        for build in [WIN10, WIN11, None] {
            assert_eq!(snipping_state(Some(1), build), (true, true));
            assert_eq!(snipping_state(Some(0), build), (true, false));
        }
    }

    #[test]
    fn snipping_value_absent_depends_on_version() {
        assert_eq!(snipping_state(None, WIN11), (true, true));
        assert_eq!(snipping_state(None, Some(WINDOWS_11_BUILD)), (true, true));
        assert_eq!(snipping_state(None, WIN10), (false, false));
        assert_eq!(snipping_state(None, None), (false, false));
    }

    #[test]
    fn take_over_stores_previous_only_once() {
        assert_eq!(previous_to_store(false, Some(1)), Some(Some(1)));
        assert_eq!(previous_to_store(false, Some(0)), Some(Some(0)));
        assert_eq!(previous_to_store(false, None), Some(None));
        // Already ours: the stored original value is kept.
        assert_eq!(previous_to_store(true, Some(0)), None);
        assert_eq!(previous_to_store(true, None), None);
    }

    #[test]
    fn restore_puts_back_or_deletes() {
        assert_eq!(restore_action(true, Some(1)), RestoreAction::Set(1));
        assert_eq!(restore_action(true, Some(0)), RestoreAction::Set(0));
        assert_eq!(restore_action(true, None), RestoreAction::Delete);
        assert_eq!(restore_action(false, Some(1)), RestoreAction::Nothing);
        assert_eq!(restore_action(false, None), RestoreAction::Nothing);
    }

    #[test]
    fn take_over_then_restore_round_trips() {
        for (original, build) in [(Some(1), WIN11), (None, WIN11), (Some(0), WIN10), (None, WIN10)] {
            let stored = previous_to_store(false, original).unwrap();
            let after = match restore_action(true, stored) {
                RestoreAction::Set(v) => Some(v),
                RestoreAction::Delete => None,
                RestoreAction::Nothing => unreachable!(),
            };
            assert_eq!(after, original);
            assert_eq!(snipping_state(after, build), snipping_state(original, build));
        }
    }

    #[test]
    fn onedrive_registry_values() {
        assert!(onedrive_value_blocking(&RegData::Dword(1)));
        assert!(!onedrive_value_blocking(&RegData::Dword(0)));
        assert!(onedrive_value_blocking(&RegData::Text("True".into())));
        assert!(onedrive_value_blocking(&RegData::Text("1".into())));
        assert!(!onedrive_value_blocking(&RegData::Text("false".into())));
    }

    #[test]
    fn onedrive_screenshot_dat() {
        assert!(onedrive_dat_blocking(b"1\r\n2\r\n"));
        assert!(onedrive_dat_blocking(b"\xEF\xBB\xBF0\n2"));
        assert!(!onedrive_dat_blocking(b"1\r\n1\r\n"));
        assert!(!onedrive_dat_blocking(b"2"));
        assert!(!onedrive_dat_blocking(b""));
        let utf16: Vec<u8> = [0xFEFFu16]
            .iter()
            .chain("1\r\n2\r\n".encode_utf16().collect::<Vec<_>>().iter())
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(onedrive_dat_blocking(&utf16));
    }

    /// Reads (never writes) the real registry of this machine. Run with `cargo test -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn status_reads_this_machine() {
        let status = status();
        println!("{status:?}");
        assert!(status.applicable || !status.snipping_enabled);
    }
}
