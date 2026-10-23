//! Ctrl+V (Cmd+V on macOS) runs smart paste while WellSnap's own copy is still on the clipboard: a single
//! screenshot with separate comment text, or a set copied with "Copy all" that has comments. One native paste
//! would lose that text in Telegram and in Chromium chats (see clipboard/mod.rs).
//!
//! The key is watched only while the setting is on and smart paste has something to paste, like its own
//! hotkey (hotkeys.rs). A press is taken only when the clipboard still holds what WellSnap last put there for
//! the source smart paste would use now. Everything else passes through untouched: a clipboard written by any
//! program since, a terminal (smart paste never types into one), WellSnap's own windows, and synthetic presses,
//! smart paste's own among them.

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
pub use platform::MARK;

use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use tauri::{AppHandle, Manager};

/// What smart paste takes its pictures and text from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// The single copy of this generation.
    Copy(u64),
    /// The set with this id and these snapshots.
    Set(String, Vec<char>),
}

/// The source WellSnap last put on the clipboard with its text, and the clipboard's change count right after.
static ARMED: Mutex<Option<(Source, u64)>> = Mutex::new(None);
/// A taken key is still down: its auto-repeat presses are taken too, or they would paste the clipboard as is.
static HELD: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Starts or stops watching the key. Called with every hotkey change, on the main thread.
pub fn sync(app: &AppHandle, on: bool) {
    let _ = APP.set(app.clone());
    let before = watching();
    #[cfg(any(windows, target_os = "macos"))]
    platform::set_watching(on);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = on;
    let after = watching();
    // A key held while the watch stopped sends its release to nobody.
    if !after {
        key_up();
    }
    if before != after {
        log::info!("paste key {}", if after { "watched" } else { "released" });
    }
}

/// Ctrl+V is watched now (the macOS tap needs the Accessibility permission).
pub fn watching() -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    return platform::watching();
    #[cfg(not(any(windows, target_os = "macos")))]
    return false;
}

/// Right after WellSnap put smart paste's current source on the clipboard, or after a smart paste: Ctrl+V then
/// pastes it with its text. A source without text is not armed; a plain paste loses nothing there.
pub fn arm(app: &AppHandle) {
    let state = app.state::<AppState>();
    let source = current_source(&state).filter(|source| has_text(&state, source));
    let armed = source.zip(crate::clipboard::change_count());
    *ARMED.lock().unwrap_or_else(PoisonError::into_inner) = armed;
}

fn current_source(state: &AppState) -> Option<Source> {
    if let Some((generation, _)) = state.last_copy.lock().unwrap().as_ref() {
        return Some(Source::Copy(*generation));
    }
    let session = state.session.lock().unwrap();
    session.as_ref().filter(|s| !s.is_empty()).map(|s| Source::Set(s.id.clone(), s.letters()))
}

/// A single copy exists only with text.
fn has_text(state: &AppState, source: &Source) -> bool {
    match source {
        Source::Copy(_) => true,
        Source::Set(..) => {
            let lang = state.lang();
            state.session.lock().unwrap().as_ref().is_some_and(|s| !s.text_block(lang).is_empty())
        }
    }
}

/// Never waits for a lock: the keyboard hook must answer fast (Windows drops a slow low-level hook), and a
/// set being saved holds the session lock while it writes a file. Busy means a plain paste.
fn is_armed(state: &AppState) -> bool {
    let armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let Some((source, count)) = armed else { return false };
    if crate::clipboard::change_count() != Some(count) {
        return false;
    }
    let Ok(last_copy) = state.last_copy.try_lock() else { return false };
    let now = match last_copy.as_ref() {
        Some((generation, _)) => Source::Copy(*generation),
        None => {
            let Ok(session) = state.session.try_lock() else { return false };
            match session.as_ref().filter(|s| !s.is_empty()) {
                Some(s) => Source::Set(s.id.clone(), s.letters()),
                None => return false,
            }
        }
    };
    now == source
}

/// The platform saw Ctrl+V (Cmd+V) pressed by the user, with no other modifier. True: the press is taken and
/// must not reach the program. Runs inside the keyboard hook, so it only decides and leaves the paste to
/// smart paste's own thread.
fn key_down() -> bool {
    held_or(take_press)
}

/// A repeat of a taken press is taken without asking `take` again.
fn held_or(take: impl FnOnce() -> bool) -> bool {
    if HELD.load(Ordering::SeqCst) {
        return true;
    }
    let taken = take();
    HELD.store(taken, Ordering::SeqCst);
    taken
}

fn key_up() {
    HELD.store(false, Ordering::SeqCst);
}

fn take_press() -> bool {
    let Some(app) = APP.get() else { return false };
    if crate::actions::smart_paste_busy() || !is_armed(&app.state::<AppState>()) {
        return false;
    }
    let foreground = crate::paste::foreground();
    if foreground.ours || !crate::actions::types_into(foreground.info.as_ref()) {
        return false;
    }
    log::info!("paste key taken for smart paste");
    crate::actions::smart_paste(app);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_the_same_only_with_the_same_copy_or_snapshots() {
        assert_eq!(Source::Copy(3), Source::Copy(3));
        assert_ne!(Source::Copy(3), Source::Copy(4), "a newer single copy");
        let set = |letters: &str| Source::Set("s1".into(), letters.chars().collect());
        assert_eq!(set("AB"), set("AB"));
        assert_ne!(set("AB"), set("ABC"), "a snapshot added after the copy");
        assert_ne!(set("AB"), Source::Set("s2".into(), vec!['A', 'B']), "another set");
    }

    #[test]
    fn a_held_key_stays_taken_until_it_is_released() {
        key_up();
        assert!(!held_or(|| false), "a press that is not taken");
        assert!(held_or(|| true));
        assert!(held_or(|| panic!("asked again")), "auto-repeat of a taken press");
        key_up();
        assert!(!held_or(|| false));
    }
}
