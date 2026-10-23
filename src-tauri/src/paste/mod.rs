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

/// The key of a paste: Ctrl+V on Windows; on macOS the key that types "v" with Command in the current layout.
pub use platform::PasteKey;

/// Looked up once per smart paste, before its focus checks: on macOS the layout lookup runs on the main thread
/// and may take a while.
pub fn paste_key() -> PasteKey {
    platform::paste_key()
}

/// Sends Ctrl+V (Windows) or Cmd+V (macOS) to the focused application.
pub fn send_paste(key: PasteKey) -> Result<(), String> {
    platform::send_paste(key)
}

/// The foreground window when a smart paste starts. Read from one window, so the program, the id and the
/// elevation check cannot describe two different windows.
#[derive(Debug, Default)]
pub struct Foreground {
    /// For `target::detect`: the executable (bundle id on macOS) and, for browsers only, the window title. The
    /// title must never be logged or stored.
    pub info: Option<target::ForegroundInfo>,
    /// The same value `foreground_id` returns for this window.
    pub id: Option<String>,
    /// The window belongs to a process with a higher integrity level (runs as administrator while WellSnap does
    /// not). Windows drops synthetic input to it silently. Always false outside Windows.
    pub elevated: bool,
    /// The window belongs to WellSnap itself (a comment field of the editor, Settings).
    pub ours: bool,
}

pub fn foreground() -> Foreground {
    platform::foreground()
}

/// Identifies the foreground window (Windows) or app (macOS), to notice a focus change between two pastes. It
/// cannot see a chat switched inside the same window, so it narrows the risk rather than removing it.
pub fn foreground_id() -> Option<String> {
    platform::foreground_id()
}

/// The foreground window title without unread counters (Windows; None elsewhere), whatever program owns it: a
/// chat or tab switched inside the same window changes it, a counter that appears, changes or goes away does
/// not. Compared in memory only, never logged or stored.
pub fn foreground_fingerprint() -> Option<String> {
    #[cfg(windows)]
    return platform::foreground_title().map(|title| without_counters(&title));
    #[cfg(not(windows))]
    return None;
}

/// Removes every "(digits)" group with one space next to it: "(3) Inbox" and "Inbox (3) - Gmail" become "Inbox"
/// and "Inbox - Gmail".
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn without_counters(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(open) = rest.find('(') {
        let inside = &rest[open + 1..];
        let digits = inside.len() - inside.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 || !inside[digits..].starts_with(')') {
            out.push_str(&rest[..=open]);
            rest = inside;
            continue;
        }
        let after = &inside[digits + 1..];
        match rest[..open].strip_suffix(' ') {
            Some(before) => {
                out.push_str(before);
                rest = after;
            }
            None => {
                out.push_str(&rest[..open]);
                rest = after.strip_prefix(' ').unwrap_or(after);
            }
        }
    }
    out.push_str(rest);
    out
}

/// macOS needs the Accessibility (post events) permission for synthetic input; always true on Windows.
pub fn can_send_input() -> bool {
    platform::can_send_input()
}

pub fn request_input_permission() -> bool {
    platform::request_input_permission()
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
    #[derive(Debug, Clone, Copy)]
    pub struct PasteKey;
    pub fn paste_key() -> PasteKey {
        PasteKey
    }
    pub fn send_paste(_key: PasteKey) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn foreground_id() -> Option<String> {
        None
    }
    pub fn foreground() -> super::Foreground {
        super::Foreground::default()
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
    fn the_fingerprint_ignores_unread_counters() {
        use super::without_counters as f;
        assert_eq!(f("(1) Inbox"), "Inbox");
        assert_eq!(f("Inbox (3) - X"), "Inbox - X");
        assert_eq!(f("Inbox (12)"), "Inbox");
        // A counter that appears, changes or goes away.
        assert_eq!(f("Chat"), f("(1) Chat"));
        assert_eq!(f("(3) Chat"), f("(4) Chat"));
        assert_eq!(f("Inbox (12) - Gmail"), f("Inbox - Gmail"));
        assert_eq!(f("a (1) (2) b"), "a b");
        // Other text stays, so another chat or page still differs.
        assert_ne!(f("Chat A"), f("Chat B"));
        assert_ne!(f("Chat 2"), f("Chat 3"));
        assert_eq!(f("(x) Page (1a) ("), "(x) Page (1a) (");
        assert_eq!(f("() Page"), "() Page");
        assert_eq!(f(""), "");
        assert_eq!(f("(7)"), "");
    }
}
