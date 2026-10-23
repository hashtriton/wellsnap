//! A low-level keyboard hook (WH_KEYBOARD_LL) on its own thread with a message loop, installed only while
//! Ctrl+V is watched. A global hotkey would not do: it takes every Ctrl+V, also when the press has to reach the
//! program, and it catches smart paste's own synthetic Ctrl+V. The hook sees whether a press is synthetic
//! (LLKHF_INJECTED) and lets everything it does not take go on.

use std::sync::mpsc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYUP,
};

/// The id of the hook thread while the hook is installed.
static THREAD: Mutex<Option<u32>> = Mutex::new(None);

pub fn set_watching(on: bool) {
    let mut thread = THREAD.lock().unwrap_or_else(PoisonError::into_inner);
    if on && thread.is_none() {
        *thread = start();
    } else if !on && let Some(id) = thread.take() {
        // The thread unhooks once its message loop ends.
        if let Err(err) = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) } {
            log::warn!("Ctrl+V hook thread did not get WM_QUIT: {err}");
        }
    }
}

pub fn watching() -> bool {
    THREAD.lock().unwrap_or_else(PoisonError::into_inner).is_some()
}

fn start() -> Option<u32> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("ctrl-v-hook".into()).spawn(move || {
        let mut msg = MSG::default();
        // Creates the message queue first, so a WM_QUIT posted right after start is not lost.
        let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
        let module = unsafe { GetModuleHandleW(None) }.ok().map(|m| HINSTANCE(m.0));
        let hook = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) } {
            Ok(hook) => hook,
            Err(err) => {
                log::warn!("Ctrl+V hook not installed: {err}");
                let _ = tx.send(None);
                return;
            }
        };
        let _ = tx.send(Some(unsafe { GetCurrentThreadId() }));
        // 0 is WM_QUIT, -1 an error; the hook procedure runs inside this wait.
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
        if let Err(err) = unsafe { UnhookWindowsHookEx(hook) } {
            log::warn!("Ctrl+V hook not removed: {err}");
        }
    });
    if let Err(err) = spawned {
        log::warn!("Ctrl+V hook thread not started: {err}");
        return None;
    }
    rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lParam points at the KBDLLHOOKSTRUCT of this event.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let synthetic = info.flags.0 & LLKHF_INJECTED.0 != 0;
        if info.vkCode == u32::from(VK_V.0) && !synthetic {
            match wparam.0 as u32 {
                WM_KEYDOWN if only_ctrl_held() && super::key_down() => return LRESULT(1),
                // WM_SYSKEYUP when Alt is down by then.
                WM_KEYUP | WM_SYSKEYUP => super::key_up(),
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Ctrl and nothing else: Ctrl+Shift+V (paste as plain text) and AltGr (Ctrl+Alt) keep their meaning.
fn only_ctrl_held() -> bool {
    let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0;
    down(VK_CONTROL) && ![VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].into_iter().any(down)
}
