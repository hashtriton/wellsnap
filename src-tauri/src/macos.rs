//! macOS window and system integration.
//!
//! Overlays stay tao's own NSWindow subclass instead of becoming an NSPanel (tauri-nspanel). tao's TaoWindow
//! answers canBecomeKeyWindow from its `focusable` flag (true by default), borderless or not, so keyboard
//! shortcuts work; and with ActivationPolicy::Accessory plus canJoinAllSpaces | fullScreenAuxiliary an
//! ordinary window already shows over full-screen apps. tauri-nspanel would turn on tauri's
//! macos-private-api feature and swap the window class at runtime. The cost of a regular window is that
//! taking keyboard focus activates WellSnap, so `overlays_hidden` hands activation back to the app that was
//! frontmost before the capture.

use objc2::rc::autoreleasepool;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSFloatingWindowLevel, NSRunningApplication, NSScreenSaverWindowLevel, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSString, NSURL};
use std::sync::atomic::{AtomicI32, Ordering};
use tauri::{AppHandle, WebviewWindow};

/// Process id of the app that was frontmost when an overlay appeared; 0 when none is pending.
static PREVIOUS_APP: AtomicI32 = AtomicI32::new(0);

const PRIVACY_FALLBACK: &str = "x-apple.systempreferences:com.apple.preference.security";

/// Makes an overlay float above the menu bar, the Dock and full-screen apps on every Space.
pub fn configure_overlay(win: &WebviewWindow) {
    with_ns_window(win, |window| {
        window.setLevel(NSScreenSaverWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setHidesOnDeactivate(false);
        // The frozen frame must appear at once, without the zoom-in of ordering a window front.
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    });
}

/// Same for the snapshot strip and notices: visible on all Spaces, above normal windows.
pub fn configure_floating(win: &WebviewWindow) {
    with_ns_window(win, |window| {
        window.setLevel(NSFloatingWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setHidesOnDeactivate(false);
    });
}

/// Fades a whole pin window (pins.rs): the webview of an opaque window cannot show what is under it.
pub fn set_alpha(win: &WebviewWindow, alpha: f64) {
    with_ns_window(win, move |window| window.setAlphaValue(alpha));
}

/// Call right after an overlay is shown, instead of `set_always_on_top`: tao's always-on-top is the floating
/// level, below the menu bar and the Dock. Also remembers the frontmost app before WellSnap takes focus.
pub fn overlay_shown(win: &WebviewWindow) {
    let own_pid = std::process::id() as i32;
    with_ns_window(win, move |window| {
        if PREVIOUS_APP.load(Ordering::SeqCst) == 0 {
            let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
            if let Some(pid) = frontmost.filter(|&pid| pid > 0 && pid != own_pid) {
                PREVIOUS_APP.store(pid, Ordering::SeqCst);
            }
        }
        window.setLevel(NSScreenSaverWindowLevel);
    });
}

/// Call after the overlays are hidden: an accessory app without visible windows keeps activation, so the app
/// the user was in (the chat to paste into) would not get keyboard focus back on its own.
pub fn overlays_hidden(app: &AppHandle) {
    // Queued on the main thread after the hide requests, so activation moves only once the overlays are gone.
    let result = app.run_on_main_thread(|| {
        let pid = PREVIOUS_APP.swap(0, Ordering::SeqCst);
        if pid == 0 {
            return;
        }
        autoreleasepool(|_| {
            // The user may have switched apps by clicking elsewhere meanwhile; do not take that back.
            if !NSRunningApplication::currentApplication().isActive() {
                return;
            }
            if let Some(previous) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                previous.activateWithOptions(NSApplicationActivationOptions::empty());
            }
        });
    });
    if let Err(err) = result {
        log::warn!("could not restore the previous app: {err}");
    }
}

/// Opens System Settings at the privacy pane: "screen" (Screen Recording) or "accessibility".
pub fn open_privacy_settings(kind: &str) -> Result<(), String> {
    let url = privacy_url(kind).ok_or_else(|| format!("unknown settings page: {kind}"))?;
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        // The anchor URLs are not documented by Apple; if one stops resolving, at least open Privacy & Security.
        for candidate in [url, PRIVACY_FALLBACK] {
            if let Some(ns_url) = NSURL::URLWithString(&NSString::from_str(candidate))
                && workspace.openURL(&ns_url)
            {
                return Ok(());
            }
        }
        Err(format!("could not open {url}"))
    })
}

fn privacy_url(kind: &str) -> Option<&'static str> {
    match kind {
        "screen" => Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"),
        "accessibility" => Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
        _ => None,
    }
}

/// Runs `f` on the main thread (inline when already there), as AppKit requires for NSWindow. Callers include
/// worker threads (strip, notices, overlays rebuilt during a capture).
fn with_ns_window(win: &WebviewWindow, f: impl FnOnce(&NSWindow) + Send + 'static) {
    let handle = win.clone();
    let result = win.run_on_main_thread(move || {
        let ptr = match handle.ns_window() {
            Ok(ptr) => ptr,
            Err(err) => {
                log::warn!("no NSWindow for {}: {err}", handle.label());
                return;
            }
        };
        // SAFETY: tauri returns the live NSWindow of this window, and this closure runs on the main thread.
        let Some(window) = (unsafe { ptr.cast::<NSWindow>().as_ref() }) else { return };
        autoreleasepool(|_| f(window));
    });
    if let Err(err) = result {
        log::warn!("could not configure {}: {err}", win.label());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_panes() {
        assert!(privacy_url("screen").is_some_and(|u| u.ends_with("?Privacy_ScreenCapture")));
        assert!(privacy_url("accessibility").is_some_and(|u| u.ends_with("?Privacy_Accessibility")));
        assert_eq!(privacy_url("keyboard"), None);
    }
}
