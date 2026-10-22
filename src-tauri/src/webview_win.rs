//! Fast frame hand-off to overlays on Windows: WebView2 shared memory (PostSharedBufferToScript).
//! Pushing a 33 MB 4K frame through a custom protocol costs ~250 ms in WebView2; a shared buffer is a
//! single memcpy. The page receives it in a `sharedbufferreceived` event together with the prepare payload.
//! Also the ProcessFailed watch that reports failed webview processes (telemetry).

use crate::capture::Frame;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, ICoreWebView2_2, ICoreWebView2_17, ICoreWebView2Controller,
    ICoreWebView2Environment12, ICoreWebView2SharedBuffer,
};
#[cfg(feature = "telemetry")]
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED, COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
    COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED, COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED, COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    ICoreWebView2ProcessFailedEventArgs, ICoreWebView2ProcessFailedEventArgs2,
};
use windows::core::{HSTRING, Interface};

thread_local! {
    // WebView2 objects live on the UI thread. One buffer per overlay is kept until the next capture so the
    // page can read it; the page releases its own view with chrome.webview.releaseBuffer.
    static BUFFERS: RefCell<HashMap<String, ICoreWebView2SharedBuffer>> = RefCell::new(HashMap::new());
}

/// Posts `frame` (RGBA) with `payload_json` as additional data. Errors are sent back through `on_error`
/// because the work runs later on the UI thread.
pub fn post_frame(
    win: &WebviewWindow,
    frame: Arc<Frame>,
    payload_json: String,
    on_error: impl FnOnce(String) + Send + 'static,
) {
    let label = win.label().to_string();
    let scheduled = win.with_webview(move |webview| {
        let result = unsafe { post_on_ui_thread(webview.controller(), &label, &frame, &payload_json) };
        if let Err(err) = result {
            on_error(err.to_string());
        }
    });
    if let Err(err) = scheduled {
        log::error!("with_webview failed: {err}");
    }
}

unsafe fn post_on_ui_thread(
    controller: ICoreWebView2Controller,
    label: &str,
    frame: &Frame,
    payload_json: &str,
) -> windows::core::Result<()> {
    unsafe {
        let core = controller.CoreWebView2()?;
        let environment: ICoreWebView2Environment12 = core.cast::<ICoreWebView2_2>()?.Environment()?.cast()?;
        let buffer = environment.CreateSharedBuffer(frame.rgba.len() as u64)?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buffer.Buffer(&mut ptr)?;
        std::ptr::copy_nonoverlapping(frame.rgba.as_ptr(), ptr, frame.rgba.len());
        core.cast::<ICoreWebView2_17>()?.PostSharedBufferToScript(
            &buffer,
            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
            &HSTRING::from(payload_json),
        )?;
        BUFFERS.with(|buffers| {
            if let Some(old) = buffers.borrow_mut().insert(label.to_string(), buffer) {
                let _ = old.Close();
            }
        });
        Ok(())
    }
}

/// Frees the shared memory of all overlays (called on the UI thread after the overlays are hidden).
pub fn release_all(app: &tauri::AppHandle) {
    let _ = app.run_on_main_thread(|| {
        BUFFERS.with(|buffers| {
            for (_, buffer) in buffers.borrow_mut().drain() {
                unsafe {
                    let _ = buffer.Close();
                }
            }
        });
    });
}

/// What ended, as WebView2 reports it in ProcessFailed.
#[cfg(feature = "telemetry")]
pub struct ProcessFailure {
    pub kind: &'static str,
    pub reason: &'static str,
    pub exit_code: Option<i32>,
}

/// Calls `on_failed` (on the UI thread) whenever a process of this webview fails: the renderer crashed or was
/// killed, the GPU process died, the whole browser process exited.
#[cfg(feature = "telemetry")]
pub fn watch_process_failed(webview: &tauri::Webview, on_failed: impl Fn(ProcessFailure) + Send + 'static) {
    let label = webview.label().to_string();
    let scheduled = webview.with_webview(move |platform| {
        let attached = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
                let handler = webview2_com::ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(args) = args {
                        on_failed(describe_failure(&args));
                    }
                    Ok(())
                }));
                let mut token = 0i64;
                core.add_ProcessFailed(&handler, &mut token)
            })
        };
        if let Err(err) = attached {
            log::warn!("no ProcessFailed watch on {label}: {err}");
        }
    });
    if let Err(err) = scheduled {
        log::warn!("no ProcessFailed watch: {err}");
    }
}

#[cfg(feature = "telemetry")]
fn describe_failure(args: &ICoreWebView2ProcessFailedEventArgs) -> ProcessFailure {
    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
    let _ = unsafe { args.ProcessFailedKind(&mut kind) };
    // Reason and exit code need the second args interface; older runtimes only give the kind.
    let (mut reason, mut exit_code) = (None, None);
    if let Ok(args2) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
        let mut value = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
        if unsafe { args2.Reason(&mut value) }.is_ok() {
            reason = Some(value);
        }
        let mut code = 0i32;
        if unsafe { args2.ExitCode(&mut code) }.is_ok() {
            exit_code = Some(code);
        }
    }
    ProcessFailure { kind: kind_name(kind), reason: reason.map_or("unknown", reason_name), exit_code }
}

#[cfg(feature = "telemetry")]
fn kind_name(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> &'static str {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => "browser_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => "render_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => "render_unresponsive",
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => "frame_render_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED => "utility_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED => "sandbox_helper_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => "gpu_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED => "ppapi_plugin_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED => "ppapi_broker_exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED => "unknown_exited",
        _ => "other",
    }
}

#[cfg(feature = "telemetry")]
fn reason_name(reason: COREWEBVIEW2_PROCESS_FAILED_REASON) -> &'static str {
    match reason {
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED => "unexpected",
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE => "unresponsive",
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED => "terminated",
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED => "crashed",
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED => "launch_failed",
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY => "out_of_memory",
        COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED => "profile_deleted",
        _ => "other",
    }
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;

    #[test]
    fn process_failure_names() {
        assert_eq!(kind_name(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED), "render_exited");
        assert_eq!(kind_name(COREWEBVIEW2_PROCESS_FAILED_KIND(99)), "other");
        assert_eq!(reason_name(COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED), "crashed");
        assert_eq!(reason_name(COREWEBVIEW2_PROCESS_FAILED_REASON(99)), "other");
    }
}
