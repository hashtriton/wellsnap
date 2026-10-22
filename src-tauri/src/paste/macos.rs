//! macOS synthetic paste (CGEventPost Cmd+V) and frontmost app detection. Posting events needs the
//! Accessibility permission (CGPreflightPostEventAccess), which System Settings lists under Accessibility.

use super::Foreground;
use super::target::{ForegroundInfo, is_browser};
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGKeyCode,
    CGPreflightPostEventAccess, CGRequestPostEventAccess, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowLayer, kCGWindowName, kCGWindowOwnerPID,
};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// kVK_ANSI_V: the V key position on ANSI keyboards. Tried first, and used when the layout lookup fails.
const KEY_V: CGKeyCode = 9;
/// The layout lookup runs on the main thread; a busy main thread must not hold up the paste for long.
const LAYOUT_TIMEOUT: Duration = Duration::from_millis(300);
/// NX_DEVICELCMDKEYMASK: a real keyboard also sets the device-dependent left-Command bit, and some apps check
/// it before treating the event as a shortcut.
const DEVICE_LEFT_COMMAND: u64 = 0x08;
const POLL: Duration = Duration::from_millis(10);

fn modifiers() -> CGEventFlags {
    CGEventFlags::MaskCommand | CGEventFlags::MaskAlternate | CGEventFlags::MaskControl | CGEventFlags::MaskShift
}

fn any_held(flags: CGEventFlags) -> bool {
    flags.intersects(modifiers())
}

pub fn wait_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !any_held(CGEventSource::flags_state(CGEventSourceStateID::HIDSystemState)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// The key code that types "v" with Command held.
#[derive(Debug, Clone, Copy)]
pub struct PasteKey(CGKeyCode);

pub fn paste_key() -> PasteKey {
    PasteKey(paste_keycode())
}

pub fn send_paste(PasteKey(key): PasteKey) -> Result<(), String> {
    if !CGPreflightPostEventAccess() {
        return Err("accessibility permission is missing".into());
    }
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    let down = CGEvent::new_keyboard_event(source.as_deref(), key, true);
    let up = CGEvent::new_keyboard_event(source.as_deref(), key, false);
    let (Some(down), Some(up)) = (down, up) else {
        return Err("CGEventCreateKeyboardEvent failed".into());
    };
    let flags = CGEventFlags(CGEventFlags::MaskCommand.0 | DEVICE_LEFT_COMMAND);
    CGEvent::set_flags(Some(&down), flags);
    CGEvent::set_flags(Some(&up), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

/// The key that types "v" with Command held in the current keyboard layout. On plain Dvorak, whose Command
/// layer is Dvorak too, key 9 would send Cmd+. (Cancel) instead of pasting.
fn paste_keycode() -> CGKeyCode {
    let found = if MainThreadMarker::new().is_some() {
        layout::command_v_keycode()
    } else {
        let (tx, rx) = mpsc::channel();
        DispatchQueue::main().exec_async(move || {
            let _ = tx.send(layout::command_v_keycode());
        });
        rx.recv_timeout(LAYOUT_TIMEOUT).unwrap_or_else(|_| {
            log::warn!("keyboard layout lookup timed out; pasting with the ANSI V key");
            None
        })
    };
    found.unwrap_or(KEY_V)
}

/// `KEY_V` first, so layouts with V in the usual place keep it, then every other virtual key code.
fn find_keycode(mut typed: impl FnMut(CGKeyCode) -> Option<String>) -> Option<CGKeyCode> {
    std::iter::once(KEY_V)
        .chain((0..128).filter(|&code| code != KEY_V))
        .find(|&code| typed(code).is_some_and(|text| text.eq_ignore_ascii_case("v")))
}

/// Text Input Sources and UCKeyTranslate (Carbon). On macOS 14 and later the TIS calls assert that they run on
/// the main thread, so `command_v_keycode` must only be called there.
mod layout {
    use super::{CGKeyCode, find_keycode};
    use objc2_core_foundation::{CFData, CFRetained, CFString, CFType};
    use std::ffi::{c_ulong, c_void};
    use std::ptr::NonNull;

    /// kUCKeyActionDown.
    const KEY_ACTION_DOWN: u16 = 0;
    /// (cmdKey >> 8) & 0xFF, the modifier state UCKeyTranslate expects for Command.
    const COMMAND_MODIFIER_STATE: u32 = 1;
    /// kUCKeyTranslateNoDeadKeysMask.
    const NO_DEAD_KEYS: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        #[allow(non_upper_case_globals)]
        static kTISPropertyUnicodeKeyLayoutData: *const CFString;
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(source: *mut c_void, key: *const CFString) -> *const c_void;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const c_void,
            key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_length: c_ulong,
            actual_length: *mut c_ulong,
            unicode: *mut u16,
        ) -> i32;
    }

    /// Looks in the current layout, then in the current ASCII-capable one, which Cocoa matches Command
    /// shortcuts against while a non-Latin layout is active.
    pub fn command_v_keycode() -> Option<CGKeyCode> {
        let copies: [unsafe extern "C" fn() -> *mut c_void; 2] =
            [TISCopyCurrentKeyboardLayoutInputSource, TISCopyCurrentASCIICapableKeyboardLayoutInputSource];
        copies.into_iter().find_map(|copy| {
            // SAFETY: a TISCopy function returns a +1 reference or null; CFRetained releases it.
            let source = unsafe { CFRetained::from_raw(NonNull::new(copy())?.cast::<CFType>()) };
            source_keycode(&source)
        })
    }

    fn source_keycode(source: &CFType) -> Option<CGKeyCode> {
        let source = (source as *const CFType).cast_mut().cast::<c_void>();
        // SAFETY: get rule: the CFData belongs to the input source, which outlives this function. Layouts
        // without Unicode data (rare legacy ones) return null.
        let data = unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) };
        let data = unsafe { data.cast::<CFData>().as_ref() }?;
        let layout = data.byte_ptr().cast::<c_void>();
        let keyboard_type = u32::from(unsafe { LMGetKbdType() });
        find_keycode(|code| {
            let mut dead_keys = 0u32;
            let mut chars = [0u16; 4];
            let mut len: c_ulong = 0;
            // SAFETY: `layout` points at the UCKeyboardLayout in `data`; the out pointers are valid locals.
            let status = unsafe {
                UCKeyTranslate(
                    layout,
                    code,
                    KEY_ACTION_DOWN,
                    COMMAND_MODIFIER_STATE,
                    keyboard_type,
                    NO_DEAD_KEYS,
                    &mut dead_keys,
                    chars.len() as c_ulong,
                    &mut len,
                    chars.as_mut_ptr(),
                )
            };
            let len = usize::try_from(len).ok()?.min(chars.len());
            (status == 0).then(|| String::from_utf16_lossy(&chars[..len]))
        })
    }
}

/// Bundle id of the frontmost application, e.g. "ru.keepcoder.Telegram".
pub fn foreground_id() -> Option<String> {
    autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(app.bundleIdentifier()?.to_string())
    })
}

/// The id is the bundle id, read with the program in the same call; input is never refused for elevation here.
pub fn foreground() -> Foreground {
    let info = foreground_info();
    let id = info.as_ref().map(|info| info.app.clone());
    Foreground { info, id, elevated: false }
}

/// The bundle id of the frontmost application and, for a browser, the name of its front window.
fn foreground_info() -> Option<ForegroundInfo> {
    let (app, pid) = autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some((app.bundleIdentifier()?.to_string(), app.processIdentifier()))
    })?;
    let title = if is_browser(&app, "") { window_title(pid) } else { None };
    Some(ForegroundInfo { app, path: String::new(), title })
}

/// macOS fills in kCGWindowName only for apps that hold Screen Recording, which capture needs anyway; reading
/// it shows no prompt. Without the permission the name is missing and detection uses the bundle id alone.
fn window_title(pid: i32) -> Option<String> {
    let options = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID)?;
    // SAFETY: every element of the window list is a dictionary with CFString keys (CGWindow.h).
    let windows = unsafe { list.cast_unchecked::<CFDictionary<CFString, CFType>>() };
    let number = |window: &CFDictionary<CFString, CFType>, key: &CFString| {
        window.get(key)?.downcast_ref::<CFNumber>()?.as_i64()
    };
    // SAFETY: the keys are immutable CFString constants of CoreGraphics.
    let (owner, layer, name) = unsafe { (kCGWindowOwnerPID, kCGWindowLayer, kCGWindowName) };
    // The list runs from the front window to the back; layer 0 holds normal windows, not menus or panels.
    let front = windows
        .iter()
        .find(|window| number(window, owner) == Some(i64::from(pid)) && number(window, layer) == Some(0))?;
    let title = front.get(name)?;
    Some(title.downcast_ref::<CFString>()?.to_string())
}

pub fn can_send_input() -> bool {
    CGPreflightPostEventAccess()
}

/// Shows the system prompt that leads to Accessibility settings (only the first time) and returns at once.
pub fn request_input_permission() -> bool {
    CGRequestPostEventAccess()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_detection() {
        assert!(!any_held(CGEventFlags::empty()));
        assert!(!any_held(CGEventFlags::MaskAlphaShift));
        assert!(any_held(CGEventFlags::MaskCommand));
        assert!(any_held(CGEventFlags::MaskAlternate | CGEventFlags::MaskNonCoalesced));
        assert!(any_held(CGEventFlags::MaskControl));
        assert!(any_held(CGEventFlags::MaskShift));
    }

    #[test]
    fn qwerty_keeps_the_ansi_v_key() {
        let qwerty = |code: CGKeyCode| Some(if code == 9 { "v" } else { "x" }.to_string());
        assert_eq!(find_keycode(qwerty), Some(9));
    }

    #[test]
    fn dvorak_command_layer_uses_the_moved_v_key() {
        // Plain Dvorak: key 9 types "." and kVK_ANSI_Period (47) types "v", also with Command held.
        let dvorak = |code: CGKeyCode| match code {
            9 => Some(".".to_string()),
            47 => Some("V".to_string()),
            _ => Some(String::new()),
        };
        assert_eq!(find_keycode(dvorak), Some(47));
    }

    #[test]
    fn no_v_in_the_layout_finds_nothing() {
        assert_eq!(find_keycode(|_| None), None);
        assert_eq!(find_keycode(|_| Some("\u{43c}".to_string())), None);
    }
}
