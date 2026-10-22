//! Synthetic paste for "smart paste": the app pastes the text first and then the files, because one native
//! paste cannot deliver both (see clipboard/mod.rs). Platform parts are implemented in the platform tasks.

pub mod plan;
pub mod target;

use std::time::Duration;

/// Waits until Ctrl/Alt/Shift/Win (Cmd/Option/Control/Shift on macOS) are physically released, so the
/// synthetic Ctrl+V is not combined with the keys of the hotkey that triggered it. False on timeout.
pub fn wait_modifiers_released(timeout: Duration) -> bool {
    platform::wait_modifiers_released(timeout)
}

/// Sends Ctrl+V (Windows) or Cmd+V (macOS) to the focused application.
pub fn send_paste() -> Result<(), String> {
    platform::send_paste()
}

/// Executable name (Windows, lowercase, e.g. "telegram.exe") or bundle id (macOS) of the foreground app.
pub fn foreground_app() -> Option<String> {
    platform::foreground_app()
}

/// The foreground program for `target::detect`: the executable (bundle id on macOS) and, for browsers only, the
/// window title. The title must never be logged or stored.
#[allow(dead_code)] // Called by smart paste from WP4b on.
pub fn foreground_info() -> Option<target::ForegroundInfo> {
    platform::foreground_info()
}

/// Identifies the foreground window (Windows) or app (macOS), to notice a focus change between two pastes. It
/// cannot see a chat switched inside the same window, so it narrows the risk rather than removing it.
pub fn foreground_id() -> Option<String> {
    #[cfg(windows)]
    return platform::foreground_window().map(|hwnd| format!("{hwnd:x}"));
    #[cfg(not(windows))]
    return platform::foreground_app();
}

/// The foreground window belongs to a process with a higher integrity level (runs as administrator while
/// WellSnap does not). Windows drops synthetic input to it silently. Always false outside Windows.
pub fn foreground_is_elevated() -> bool {
    #[cfg(windows)]
    return platform::foreground_is_elevated();
    #[cfg(not(windows))]
    return false;
}

/// macOS needs the Accessibility (post events) permission for synthetic input; always true on Windows.
pub fn can_send_input() -> bool {
    platform::can_send_input()
}

pub fn request_input_permission() -> bool {
    platform::request_input_permission()
}

pub fn is_telegram(app: &str) -> bool {
    let app = app.to_ascii_lowercase();
    app.contains("telegram") || app == "ru.keepcoder.telegram" || app == "org.telegram.desktop"
}

#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::time::Duration;
    pub fn wait_modifiers_released(_timeout: Duration) -> bool {
        true
    }
    pub fn send_paste() -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn foreground_app() -> Option<String> {
        None
    }
    pub fn foreground_info() -> Option<super::target::ForegroundInfo> {
        None
    }
    pub fn can_send_input() -> bool {
        false
    }
    pub fn request_input_permission() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn telegram_detection() {
        assert!(super::is_telegram("telegram.exe"));
        assert!(super::is_telegram("Telegram.exe"));
        assert!(super::is_telegram("ru.keepcoder.Telegram"));
        assert!(!super::is_telegram("chrome.exe"));
    }
}
