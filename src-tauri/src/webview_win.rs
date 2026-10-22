//! Fast frame hand-off to overlays on Windows: WebView2 shared memory (PostSharedBufferToScript).
//! Pushing a 33 MB 4K frame through a custom protocol costs ~250 ms in WebView2; a shared buffer is a
//! single memcpy. The page receives it in a `sharedbufferreceived` event together with the prepare payload.

use crate::capture::Frame;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, ICoreWebView2_2, ICoreWebView2_17, ICoreWebView2Controller,
    ICoreWebView2Environment12, ICoreWebView2SharedBuffer,
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
