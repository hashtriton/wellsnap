//! "One picture" mode: the strip webview composes every snapshot of the set into one PNG sheet on request.
//! Rust sends `sheet:request` to the strip; the strip answers with `sheet_put` (or `sheet_failed`), and a
//! strip that was still loading asks for the open request with `sheet_pending`.
//!
//! Contract only: the request slot, the wait and the cache arrive with WP1.
#![allow(dead_code)] // Until WP1 adds the bodies and the callers (actions.rs, smart paste).

use crate::session::Snapshot;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;

/// Event sent to the strip window with a `SheetRequest` payload.
pub const REQUEST_EVENT: &str = "sheet:request";

/// A composed sheet, cached under `<set dir>/sheets/<requestId>/`.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub png: Arc<Vec<u8>>,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Too many snapshots to keep the text readable at the size caps.
    pub cramped: bool,
}

/// One request in flight at a time, plus a small cache of finished sheets for the current set.
#[derive(Default)]
pub struct SheetJobs {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetRequest {
    pub request_id: u64,
    pub session_id: String,
    /// Heading word ("Снимок" / "Screenshot"), the same one the text block uses.
    pub title: String,
    /// Draw the comment lines under each snapshot (never for smart paste, which sends the text separately).
    pub with_comments: bool,
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPutMeta {
    pub request_id: u64,
    pub width: u32,
    pub height: u32,
    pub cramped: bool,
}

/// The sheet for `letters` (the whole set when None), built by the strip or taken from the cache. Blocks until
/// the strip answers or the timeout passes; never call it on the main thread.
pub fn ensure(_app: &AppHandle, _letters: Option<&[char]>, _with_comments: bool) -> Result<Sheet, String> {
    Err("not implemented".into())
}

/// Builds the sheets the next copy will need, without waiting. Only in one-picture mode.
pub fn request_prewarm(_app: &AppHandle) {}

/// A sheet from the strip (`sheet_put`).
pub fn accept(_app: &AppHandle, _meta: SheetPutMeta, _png: Vec<u8>) -> Result<(), String> {
    Ok(())
}

/// The strip could not compose the request (`sheet_failed`).
pub fn fail(_app: &AppHandle, _request_id: u64, _reason: String) {}

/// The request in flight, for a strip that loaded after it was sent (`sheet_pending`).
pub fn pending(_app: &AppHandle) -> Option<SheetRequest> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_put_meta_match_ipc() {
        let snapshot = Snapshot {
            letter: 'A',
            path: PathBuf::from("A.png"),
            width: 30,
            height: 20,
            marks: vec![],
            comments_on_image: false,
            revision: 0,
            scale: 1.5,
        };
        let request = SheetRequest {
            request_id: 7,
            session_id: "s1".into(),
            title: "Screenshot".into(),
            with_comments: true,
            snapshots: vec![snapshot],
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "requestId": 7,
                "sessionId": "s1",
                "title": "Screenshot",
                "withComments": true,
                "snapshots": [{"letter": "A", "width": 30, "height": 20, "marks": [], "commentsOnImage": false, "revision": 0, "scale": 1.5}],
            })
        );
        // The strip sends JSON.stringify(meta), URI-encoded in the x-meta header.
        let header = urlencoding::encode(r#"{"requestId":7,"width":1920,"height":1080,"cramped":true}"#).into_owned();
        let meta: SheetPutMeta = serde_json::from_str(&urlencoding::decode(&header).unwrap()).unwrap();
        assert_eq!((meta.request_id, meta.width, meta.height, meta.cramped), (7, 1920, 1080, true));
    }
}
