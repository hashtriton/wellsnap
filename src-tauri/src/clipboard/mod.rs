//! Multi-format clipboard writes in ONE clipboard transaction.
//!
//! Why custom code: the Tauri clipboard plugin cannot write file lists, arboard writes one format per call,
//! and clipboard-rs on Windows silently drops other formats when an image is included. What targets read:
//! - Chromium (Chrome, Edge, Electron, WebView2) and Telegram: if files are present, ONLY files are used;
//!   plain text and HTML are ignored. Files (CF_HDROP / public.file-url) arrive in order.
//! - A bitmap plus text: Chromium pages get both; Telegram shows the picture.
//!   See the project memory note "clipboard-one-paste-limit".

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct ClipPayload {
    /// Existing files, in paste order (Windows CF_HDROP + "Preferred DropEffect" = copy; macOS one
    /// public.file-url pasteboard item per file).
    pub files: Vec<PathBuf>,
    /// Plain text (CF_UNICODETEXT / public.utf8-plain-text). On macOS it must be on exactly one item.
    pub text: Option<String>,
    /// HTML fragment; the platform layer adds the CF_HTML header on Windows.
    pub html: Option<String>,
    /// Encoded PNG; written as a bitmap (CF_DIBV5 + registered "PNG" format / public.png).
    pub image_png: Option<Vec<u8>>,
    /// Keep out of clipboard history and cloud sync (smart paste's intermediate text step).
    pub transient: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[cfg_attr(not(windows), allow(dead_code))]
    #[error("clipboard is busy")]
    Busy,
    #[error("clipboard write failed: {0}")]
    Failed(String),
}

pub fn write(payload: &ClipPayload) -> Result<(), ClipboardError> {
    #[cfg(windows)]
    return windows::write(payload);
    #[cfg(target_os = "macos")]
    return macos::write(payload);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = payload;
        Err(ClipboardError::Failed("unsupported platform".into()))
    }
}

/// One screenshot: bitmap + optional text (comments). No file list, so web chats also get the text.
pub fn copy_image(png: Vec<u8>, text: Option<String>) -> Result<(), ClipboardError> {
    write(&ClipPayload { image_png: Some(png), text: text.filter(|t| !t.is_empty()), ..Default::default() })
}

/// A snapshot set: files A.png..Z.png (one paste = album / attachments) plus text and HTML for targets that
/// read them (text editors, Word, Outlook). No bitmap: Chromium would hide it anyway and Telegram would
/// treat the paste ambiguously.
pub fn copy_files(files: Vec<PathBuf>, text: Option<String>, html: Option<String>) -> Result<(), ClipboardError> {
    write(&ClipPayload {
        files,
        text: text.filter(|t| !t.is_empty()),
        html: html.filter(|h| !h.is_empty()),
        ..Default::default()
    })
}

pub fn copy_text(text: String, transient: bool) -> Result<(), ClipboardError> {
    write(&ClipPayload { text: Some(text), transient, ..Default::default() })
}
