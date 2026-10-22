//! macOS pasteboard writer: one clearContents + writeObjects, so readers never see a half-written pasteboard.
//!
//! Item layout (see mod.rs): text and HTML on the first item only, because -[NSPasteboard stringForType:]
//! joins the strings of every item that has the type; then the image (PNG plus TIFF for readers that only
//! take TIFF); then one item per file with nothing but public.file-url, in paste order, which is what
//! Chromium and Telegram iterate over.
//!
//! Threading: NSPasteboard and NSPasteboardItem are not main-thread-only (AppKit marks NSWindow and other UI
//! classes with NS_SWIFT_UI_ACTOR, not these), and the pasteboard is a cross-process service, so writes run on
//! the calling thread. A lock keeps our own writers from interleaving clearContents and writeObjects.

use super::{ClipPayload, ClipboardError};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSBitmapImageRep, NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};
use std::path::Path;
use std::sync::{Mutex, PoisonError};

const TEXT: &str = "public.utf8-plain-text";
const HTML: &str = "public.html";
const PNG: &str = "public.png";
const TIFF: &str = "public.tiff";
const FILE_URL: &str = "public.file-url";
/// nspasteboard.org markers: clipboard managers skip transient and concealed content.
const TRANSIENT: &str = "org.nspasteboard.TransientType";
const CONCEALED: &str = "org.nspasteboard.ConcealedType";

static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, PartialEq)]
enum Value<'a> {
    Text(&'a str),
    Png(&'a [u8]),
    /// Converted from the PNG when the item is built.
    TiffFromPng(&'a [u8]),
    FileUrl(&'a Path),
    Marker,
}

type Item<'a> = Vec<(&'static str, Value<'a>)>;

fn layout(payload: &ClipPayload) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let mut head = Vec::new();
    if let Some(text) = &payload.text {
        head.push((TEXT, Value::Text(text)));
    }
    if let Some(html) = &payload.html {
        head.push((HTML, Value::Text(html)));
    }
    if !head.is_empty() {
        items.push(head);
    }
    if let Some(png) = &payload.image_png {
        items.push(vec![(PNG, Value::Png(png)), (TIFF, Value::TiffFromPng(png))]);
    }
    for file in &payload.files {
        items.push(vec![(FILE_URL, Value::FileUrl(file))]);
    }
    if payload.transient {
        // Managers differ in whether they look at the pasteboard's or each item's types; mark every item.
        for item in &mut items {
            item.push((TRANSIENT, Value::Marker));
            item.push((CONCEALED, Value::Marker));
        }
    }
    items
}

pub fn write(payload: &ClipPayload) -> Result<(), ClipboardError> {
    let items = layout(payload);
    if items.is_empty() {
        return Err(ClipboardError::Failed("nothing to write".into()));
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    autoreleasepool(|_| {
        let objects = items
            .iter()
            .map(|item| build_item(item).map(ProtocolObject::from_retained))
            .collect::<Result<Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>>, _>>()?;
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if pasteboard.writeObjects(&NSArray::from_retained_slice(&objects)) {
            Ok(())
        } else {
            Err(ClipboardError::Failed("NSPasteboard writeObjects failed".into()))
        }
    })
}

fn build_item(item: &Item<'_>) -> Result<Retained<NSPasteboardItem>, ClipboardError> {
    let out = NSPasteboardItem::new();
    for (uti, value) in item {
        let uti = NSString::from_str(uti);
        let stored = match value {
            Value::Text(text) => out.setString_forType(&NSString::from_str(text), &uti),
            Value::Png(png) => out.setData_forType(&NSData::with_bytes(png), &uti),
            Value::TiffFromPng(png) => match tiff_from_png(png) {
                Some(tiff) => out.setData_forType(&tiff, &uti),
                None => {
                    log::warn!("could not convert the PNG to TIFF; writing PNG only");
                    true
                }
            },
            Value::FileUrl(path) => {
                let url = file_url(path)?;
                out.setString_forType(&url, &uti)
            }
            Value::Marker => out.setData_forType(&NSData::new(), &uti),
        };
        if !stored {
            return Err(ClipboardError::Failed(format!("could not store {uti} on the pasteboard item")));
        }
    }
    Ok(out)
}

/// Uncompressed TIFF: LZW would add a noticeable delay to Ctrl+C on a 4K frame.
fn tiff_from_png(png: &[u8]) -> Option<Retained<NSData>> {
    NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png))?.TIFFRepresentation()
}

fn file_url(path: &Path) -> Result<Retained<NSString>, ClipboardError> {
    let path_str =
        path.to_str().ok_or_else(|| ClipboardError::Failed(format!("path is not valid UTF-8: {}", path.display())))?;
    if !path.is_absolute() {
        return Err(ClipboardError::Failed(format!("file path must be absolute: {path_str}")));
    }
    NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path_str), false)
        .absoluteString()
        .ok_or_else(|| ClipboardError::Failed(format!("no file URL for {path_str}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn types(item: &Item<'_>) -> Vec<&'static str> {
        item.iter().map(|(uti, _)| *uti).collect()
    }

    #[test]
    fn set_layout_puts_text_on_one_item_and_each_file_alone() {
        let payload = ClipPayload {
            files: vec![PathBuf::from("/tmp/A.png"), PathBuf::from("/tmp/B.png")],
            text: Some("A1 note".into()),
            html: Some("<p>A1 note</p>".into()),
            ..Default::default()
        };
        let items = layout(&payload);
        assert_eq!(items.len(), 3);
        assert_eq!(types(&items[0]), vec![TEXT, HTML]);
        assert_eq!(items[1], vec![(FILE_URL, Value::FileUrl(Path::new("/tmp/A.png")))]);
        assert_eq!(items[2], vec![(FILE_URL, Value::FileUrl(Path::new("/tmp/B.png")))]);
        let text_items = items.iter().filter(|item| types(item).contains(&TEXT)).count();
        assert_eq!(text_items, 1);
    }

    #[test]
    fn image_layout_has_text_item_then_png_and_tiff() {
        let payload = ClipPayload { image_png: Some(vec![1, 2, 3]), text: Some("note".into()), ..Default::default() };
        let items = layout(&payload);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], vec![(TEXT, Value::Text("note"))]);
        assert_eq!(items[1], vec![(PNG, Value::Png(&[1, 2, 3])), (TIFF, Value::TiffFromPng(&[1, 2, 3]))]);
    }

    #[test]
    fn image_only_has_no_empty_text_item() {
        let payload = ClipPayload { image_png: Some(vec![9]), ..Default::default() };
        let items = layout(&payload);
        assert_eq!(items.len(), 1);
        assert_eq!(types(&items[0]), vec![PNG, TIFF]);
    }

    #[test]
    fn transient_text_is_marked() {
        let payload = ClipPayload { text: Some("caption".into()), transient: true, ..Default::default() };
        let items = layout(&payload);
        assert_eq!(items.len(), 1);
        assert_eq!(types(&items[0]), vec![TEXT, TRANSIENT, CONCEALED]);
    }

    #[test]
    fn regular_copies_are_not_marked() {
        let payload = ClipPayload { text: Some("caption".into()), ..Default::default() };
        assert!(!types(&layout(&payload)[0]).contains(&TRANSIENT));
    }

    #[test]
    fn empty_payload_has_no_items() {
        assert!(layout(&ClipPayload::default()).is_empty());
    }
}
