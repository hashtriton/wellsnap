//! Windows synthetic paste (SendInput) and foreground app detection.

use super::target::{ForegroundInfo, is_browser};
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, E_ACCESSDENIED, HANDLE, HWND};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC,
    MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
use windows::core::PWSTR;

const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];
const POLL: Duration = Duration::from_millis(10);
/// SECURITY_MANDATORY_HIGH_RID: the integrity level of a process running as administrator.
const HIGH_INTEGRITY_RID: u32 = 0x3000;
/// Longest window title read, in UTF-16 units; the site names that detection looks for are near the end of
/// much shorter titles.
const TITLE_MAX: usize = 1024;

pub fn wait_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        // The most significant bit of GetAsyncKeyState is set while the key is down.
        if !MODIFIERS.iter().any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// SendInput gives no sign when User Interface Privilege Isolation drops the events (the target runs as
/// administrator and WellSnap does not): it still reports them as inserted. Only a partial insert, for
/// example when another thread blocks input, comes back as an error here.
pub fn send_paste() -> Result<(), String> {
    let inputs = paste_inputs();
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        return Ok(());
    }
    let err = windows::core::Error::from_thread();
    if sent > 0 {
        // Do not leave Ctrl logically pressed if only the first events went through.
        let release = [key(VK_V, true), key(VK_CONTROL, true)];
        unsafe { SendInput(&release, size_of::<INPUT>() as i32) };
    }
    Err(format!(
        "SendInput inserted {sent} of {} key events ({err}); input may be blocked by another app or by a window \
         running as administrator",
        inputs.len()
    ))
}

fn paste_inputs() -> [INPUT; 4] {
    [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                // Some apps look at the scan code as well as the virtual key.
                wScan: unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// The executable of the foreground window, its lowercase path and, for a browser, the window title. One
/// GetForegroundWindow call, so the title belongs to the same window as the executable. Store (UWP) apps report
/// their frame host, "applicationframehost.exe".
pub fn foreground_info() -> Option<ForegroundInfo> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let path = process_path(window_pid(hwnd)?)?;
    let app = exe_name(&path)?;
    let path = path.to_lowercase();
    let title = if is_browser(&app, &path) { window_title(hwnd) } else { None };
    Some(ForegroundInfo { app, path, title })
}

/// The title of the foreground window, whatever program owns it: smart paste's focus check compares it.
pub fn foreground_title() -> Option<String> {
    window_title(unsafe { GetForegroundWindow() })
}

/// For a window of another process GetWindowTextW returns the caption Windows stores, without sending the
/// window a message, so a hung target cannot block the paste.
fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; TITLE_MAX + 1];
    let len = usize::try_from(unsafe { GetWindowTextW(hwnd, &mut buf) }).ok()?.min(TITLE_MAX);
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        let queried = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        queried.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The foreground window as an integer (HWND is not Send).
pub fn foreground_window() -> Option<usize> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd.0 as usize)
}

fn foreground_pid() -> Option<u32> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    window_pid(hwnd)
}

fn window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// User Interface Privilege Isolation drops our synthetic keys to such a window without any error.
pub fn foreground_is_elevated() -> bool {
    let Some(pid) = foreground_pid() else { return false };
    let Ok(own) = process_integrity(unsafe { GetCurrentProcess() }) else { return false };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let target = process_integrity(process);
    let _ = unsafe { CloseHandle(process) };
    is_higher(own, target)
}

fn is_higher(own: u32, target: windows::core::Result<u32>) -> bool {
    match target {
        Ok(level) => level > own,
        // An elevated process does not let a normal one open its token.
        Err(err) => err.code() == E_ACCESSDENIED && own < HIGH_INTEGRITY_RID,
    }
}

/// Integrity level RID of the process token (0x2000 medium, 0x3000 high).
fn process_integrity(process: HANDLE) -> windows::core::Result<u32> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
        let level = token_integrity(token);
        let _ = CloseHandle(token);
        level
    }
}

unsafe fn token_integrity(token: HANDLE) -> windows::core::Result<u32> {
    unsafe {
        let mut len = 0u32;
        // The first call only reports the size (and fails with ERROR_INSUFFICIENT_BUFFER).
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut len);
        if len == 0 {
            return Err(windows::core::Error::from_thread());
        }
        // u64 elements keep the TOKEN_MANDATORY_LABEL properly aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        GetTokenInformation(token, TokenIntegrityLevel, Some(buf.as_mut_ptr().cast()), len, &mut len)?;
        let sid = (*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return Err(windows::core::Error::from_hresult(E_ACCESSDENIED));
        }
        Ok(*GetSidSubAuthority(sid, u32::from(count - 1)))
    }
}

fn exe_name(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_str()?;
    Some(name.to_lowercase())
}

pub fn can_send_input() -> bool {
    true
}

pub fn request_input_permission() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_from_full_path() {
        assert_eq!(
            exe_name(r"C:\Users\me\AppData\Roaming\Telegram Desktop\Telegram.exe").as_deref(),
            Some("telegram.exe")
        );
        assert_eq!(exe_name(r"\\?\C:\Program Files\App\APP.EXE").as_deref(), Some("app.exe"));
        assert_eq!(exe_name(""), None);
    }

    #[test]
    fn foreground_info_is_consistent() {
        assert_eq!(window_title(HWND::default()), None, "no window, no title");
        // A test run may have no foreground window at all (CI); when there is one, check its shape.
        if let Some(info) = foreground_info() {
            assert_eq!(info.path, info.path.to_lowercase());
            assert!(info.path.ends_with(&info.app), "{} / {}", info.app, info.path);
            assert!(info.title.is_none() || is_browser(&info.app, &info.path), "titles of browsers only");
        }
    }

    #[test]
    fn paste_inputs_are_ctrl_v_down_then_up() {
        let keys: Vec<(u16, bool)> = paste_inputs()
            .iter()
            .map(|i| {
                assert_eq!(i.r#type, INPUT_KEYBOARD);
                let ki = unsafe { i.Anonymous.ki };
                (ki.wVk.0, ki.dwFlags.contains(KEYEVENTF_KEYUP))
            })
            .collect();
        assert_eq!(keys, vec![(VK_CONTROL.0, false), (VK_V.0, false), (VK_V.0, true), (VK_CONTROL.0, true)]);
    }

    #[test]
    fn higher_integrity_detection() {
        let denied = || Err(windows::core::Error::from_hresult(E_ACCESSDENIED));
        assert!(is_higher(0x2000, Ok(0x3000)));
        assert!(!is_higher(0x2000, Ok(0x2000)));
        assert!(!is_higher(0x2000, Ok(0x1000)), "AppContainer apps are lower");
        assert!(!is_higher(0x3000, Ok(0x3000)));
        assert!(is_higher(0x2000, denied()));
        assert!(!is_higher(0x3000, denied()), "an elevated WellSnap can paste anywhere it can open");
        assert!(!is_higher(0x2000, Err(windows::core::Error::from_hresult(windows::core::HRESULT(-1)))));
    }

    #[test]
    fn own_integrity_level_is_readable() {
        let level = process_integrity(unsafe { GetCurrentProcess() }).unwrap();
        assert!((0x1000..=0x4000).contains(&level), "{level:#x}");
    }

    #[test]
    fn modifiers_wait_returns_without_keys_held() {

        // Nobody holds a modifier while the tests run; with a zero timeout the call must not block.
        let started = Instant::now();
        let _ = wait_modifiers_released(Duration::ZERO);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
