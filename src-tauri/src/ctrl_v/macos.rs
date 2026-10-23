//! An active event tap (CGEventTap) for key presses on its own thread with a CFRunLoop, created only while
//! Cmd+V is watched. It needs the Accessibility permission, the one smart paste asks for to post Cmd+V; without
//! it the tap is not created and Cmd+V stays plain. Smart paste marks its own events (`MARK`), and the tap
//! lets them through.

use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CGPreflightPostEventAccess,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// kCGEventSourceUserData of the Cmd+V that smart paste posts (paste/macos.rs).
pub const MARK: i64 = 0x5745_4c4c_534e_4150;

/// CF objects of the tap thread. CFRunLoopStop and CGEventTapEnable may be called from any thread.
struct Tap {
    run_loop: CFRetained<CFRunLoop>,
    port: CFRetained<CFMachPort>,
}

// SAFETY: only the thread-safe calls named above touch these from other threads.
unsafe impl Send for Tap {}

static TAP: Mutex<Option<Tap>> = Mutex::new(None);
/// Key code of the press taken last, until its key is released.
static TAKEN_KEY: AtomicI64 = AtomicI64::new(NO_KEY);
const NO_KEY: i64 = -1;
/// kVK_ANSI_V.
const ANSI_V: i64 = 9;

pub fn set_watching(on: bool) {
    let mut tap = TAP.lock().unwrap_or_else(PoisonError::into_inner);
    if on && tap.is_none() {
        *tap = start();
    } else if !on && let Some(tap) = tap.take() {
        CGEvent::tap_enable(&tap.port, false);
        tap.run_loop.stop();
    }
}

pub fn watching() -> bool {
    TAP.lock().unwrap_or_else(PoisonError::into_inner).is_some()
}

fn start() -> Option<Tap> {
    // Without the permission CGEventTapCreate fails; the next hotkey change tries again.
    if !CGPreflightPostEventAccess() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("cmd-v-tap".into()).spawn(move || {
        let mask = (1u64 << CGEventType::KeyDown.0) | (1u64 << CGEventType::KeyUp.0);
        // SAFETY: `tap_callback` matches CGEventTapCallBack and uses no user info.
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(tap_callback),
                std::ptr::null_mut(),
            )
        };
        let (Some(port), Some(run_loop)) = (port, CFRunLoop::current()) else {
            log::warn!("Cmd+V event tap not created");
            let _ = tx.send(None);
            return;
        };
        let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
            log::warn!("Cmd+V event tap has no run loop source");
            let _ = tx.send(None);
            return;
        };
        // SAFETY: a CoreFoundation constant.
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
        CGEvent::tap_enable(&port, true);
        let _ = tx.send(Some(Tap { run_loop: run_loop.clone(), port: port.clone() }));
        CFRunLoop::run();
    });
    if let Err(err) = spawned {
        log::warn!("Cmd+V tap thread not started: {err}");
        return None;
    }
    rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
}

unsafe extern "C-unwind" fn tap_callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: the event is valid for the duration of the callback.
    let event_ref = unsafe { event.as_ref() };
    match kind {
        // macOS turns off a tap that answered too slowly; turn it back on.
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            if let Some(tap) = TAP.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                CGEvent::tap_enable(&tap.port, true);
            }
        }
        CGEventType::KeyDown if is_users_cmd_v(event_ref) && super::key_down() => {
            TAKEN_KEY.store(keycode(event_ref), Ordering::SeqCst);
            return std::ptr::null_mut();
        }
        // By key code, not as Cmd+V: Command is often released before the V key.
        CGEventType::KeyUp if keycode(event_ref) == TAKEN_KEY.load(Ordering::SeqCst) => {
            TAKEN_KEY.store(NO_KEY, Ordering::SeqCst);
            super::key_up();
        }
        _ => {}
    }
    event.as_ptr()
}

fn keycode(event: &CGEvent) -> i64 {
    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode)
}

/// Command and no other modifier, a key that types "v", not posted by smart paste. With a non-Latin layout the
/// key may type its own letter even with Command; Cocoa then takes the V key of the ANSI layout as Cmd+V.
fn is_users_cmd_v(event: &CGEvent) -> bool {
    if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData) == MARK {
        return false;
    }
    let flags = CGEvent::flags(Some(event));
    let others = CGEventFlags::MaskShift | CGEventFlags::MaskAlternate | CGEventFlags::MaskControl;
    if !flags.contains(CGEventFlags::MaskCommand) || flags.intersects(others) {
        return false;
    }
    let mut chars = [0u16; 4];
    let mut len = 0;
    // SAFETY: the out pointers are valid locals of the given size.
    unsafe { CGEvent::keyboard_get_unicode_string(Some(event), chars.len() as _, &mut len, chars.as_mut_ptr()) };
    let len = usize::try_from(len).unwrap_or(0).min(chars.len());
    let typed = String::from_utf16_lossy(&chars[..len]);
    typed.eq_ignore_ascii_case("v") || (!typed.is_ascii() && keycode(event) == ANSI_V)
}
