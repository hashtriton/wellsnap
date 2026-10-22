//! macOS capture backend: ScreenCaptureKit (macOS 14+, SCScreenshotManager). CGWindowListCreateImage is
//! deprecated since 14 and regresses on newer releases, so it is not used even as a fallback.
//!
//! Coordinate systems. `MonitorInfo` comes from tao: position = CGDisplayBounds origin (global display space
//! in points, origin at the top-left of the main display, y down) times THAT monitor's backingScaleFactor, and
//! size = the display mode size in points times the same factor. SCDisplay.frame lives in the same global point
//! space, so dividing x, y, width and height by `scale` gives back the display frame in points to match on.
//! The capture is requested at width x height pixels, which is the overlay window's backing size, so one
//! frame pixel maps to one webview device pixel.
//!
//! Output is tightly packed, top-down, opaque RGBA: the row packing and the BGRA swizzle are one pass here,
//! because a 5K frame is 59 MB and a separate copy plus swizzle would walk it twice.

use super::{CaptureError, MonitorInfo, bgra_to_rgba_opaque, monitor_points};
use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapInfo, CGColorSpace, CGContext, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGDisplayBounds, CGImageByteOrderInfo, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCDisplay, SCRunningApplication, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration, SCStreamErrorCode, SCStreamErrorDomain,
};
use std::ptr::NonNull;
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

/// The first SCShareableContent query of a process can take about a second.
const CONTENT_TIMEOUT: Duration = Duration::from_secs(5);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);
/// kCVPixelFormatType_32BGRA ('BGRA').
const PIXEL_FORMAT_BGRA: u32 = u32::from_be_bytes(*b"BGRA");
/// Display frames from tao are rounded to whole physical pixels.
const FRAME_TOLERANCE_POINTS: f64 = 1.0;
/// Queries in a row that may lack our own app before the cache is trusted anyway; see `own_app_refresh_due`.
const OWN_APP_RETRIES: u32 = 3;
/// Smallest share of a frame worth its own thread in `pack_rows_rgba`.
const MIN_PART_BYTES: usize = 1 << 20;

pub fn has_permission() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Shows the system prompt (only the first time) and returns at once; a grant usually needs a relaunch.
pub fn request_permission() -> bool {
    CGRequestScreenCaptureAccess()
}

/// Displays and our own application from the last SCShareableContent query. The query is slow, so it is
/// reused until a monitor no longer matches or a capture with it fails.
struct Content {
    displays: Vec<Retained<SCDisplay>>,
    own_app: Option<Retained<SCRunningApplication>>,
    /// Queries in a row, this one included, that did not list our own application.
    own_app_misses: u32,
}

// SAFETY: SCDisplay and SCRunningApplication are immutable snapshot objects (Sendable in Swift); they are
// only read after creation.
unsafe impl Send for Content {}

/// Moves an SCShareableContent out of the completion handler's queue.
struct SendContent(Retained<SCShareableContent>);

// SAFETY: as for `Content`; the receiving thread only reads the displays and applications.
unsafe impl Send for SendContent {}

static CONTENT: Mutex<Option<Content>> = Mutex::new(None);

struct Attempt {
    error: CaptureError,
    /// The content cache was refreshed during this attempt, so retrying with a fresh one is pointless.
    refreshed: bool,
}

/// Fills the display cache at startup so the first capture is not slowed by the query. Skipped without
/// permission, because the query itself can trigger the system prompt.
pub fn prewarm() {
    if !has_permission() {
        return;
    }
    autoreleasepool(|_| {
        let mut cache = CONTENT.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.is_none() {
            match fetch_content() {
                Ok(content) => *cache = Some(content),
                Err(err) => log::warn!("display list prewarm failed: {err}"),
            }
        }
    });
}

/// Captures one monitor as tightly packed, top-down, opaque RGBA.
pub fn capture_rgba(m: &MonitorInfo) -> Result<(u32, u32, Vec<u8>), CaptureError> {
    if m.width == 0 || m.height == 0 {
        return Err(CaptureError::Failed(format!("empty monitor rect {}x{}", m.width, m.height)));
    }
    if !has_permission() {
        return Err(CaptureError::PermissionDenied);
    }
    autoreleasepool(|_| match attempt(m, false) {
        Ok(frame) => Ok(frame),
        Err(Attempt { error: CaptureError::Failed(reason), refreshed: false }) => {
            log::warn!("capture of {} failed with cached display list ({reason}), refreshing", m.name);
            attempt(m, true).map_err(|a| a.error)
        }
        Err(a) => Err(a.error),
    })
}

fn attempt(m: &MonitorInfo, force_refresh: bool) -> Result<(u32, u32, Vec<u8>), Attempt> {
    let target = monitor_points(m);
    let mut refreshed = false;
    let (display, own_app) = {
        // Monitors are captured in parallel; holding the lock while refreshing makes the others wait for one
        // query instead of starting their own.
        let mut cache = CONTENT.lock().unwrap_or_else(PoisonError::into_inner);
        let stale = cache.as_ref().is_none_or(|c| {
            own_app_refresh_due(c.own_app.is_some(), c.own_app_misses) || find_display(&c.displays, target).is_none()
        });
        if force_refresh || stale {
            let misses = cache.as_ref().map_or(0, |c| c.own_app_misses);
            let mut content = fetch_content().map_err(|error| Attempt { error, refreshed: true })?;
            content.own_app_misses = if content.own_app.is_some() { 0 } else { misses.saturating_add(1) };
            *cache = Some(content);
            refreshed = true;
        }
        let content = cache
            .as_ref()
            .ok_or_else(|| Attempt { error: CaptureError::Failed("no shareable content".into()), refreshed })?;
        let display = find_display(&content.displays, target).ok_or_else(|| Attempt {
            error: CaptureError::Failed(format!("no ScreenCaptureKit display matches monitor {}", m.name)),
            refreshed,
        })?;
        (display, content.own_app.clone())
    };
    let image =
        screenshot(&display, own_app.as_deref(), m.width, m.height).map_err(|error| Attempt { error, refreshed })?;
    image_to_rgba(&image).map_err(|error| Attempt { error, refreshed })
}

/// A query taken before WellSnap owned any window does not list it, and then the strip and notices would be
/// burned into every frame. Such a cache is refreshed, but only a few times in a row in case the app is never
/// listed, since each query costs tens of milliseconds.
fn own_app_refresh_due(has_own_app: bool, misses: u32) -> bool {
    !has_own_app && misses < OWN_APP_RETRIES
}

fn frames_match(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= FRAME_TOLERANCE_POINTS)
}

fn rect_points(r: CGRect) -> [f64; 4] {
    [r.origin.x, r.origin.y, r.size.width, r.size.height]
}

/// SCDisplay.frame is a snapshot from the query. After identical monitors swap places, an old object still
/// matches the old position and would capture the other screen, so the display's current bounds must match
/// too; otherwise the cache counts as stale and is queried again. A disconnected display has empty bounds.
fn display_matches(snapshot: [f64; 4], live: [f64; 4], target: [f64; 4]) -> bool {
    frames_match(snapshot, target) && frames_match(live, target)
}

fn find_display(displays: &[Retained<SCDisplay>], target: [f64; 4]) -> Option<Retained<SCDisplay>> {
    displays
        .iter()
        .find(|d| {
            let snapshot = rect_points(unsafe { d.frame() });
            let live = rect_points(CGDisplayBounds(unsafe { d.displayID() }));
            display_matches(snapshot, live, target)
        })
        .cloned()
}

fn fetch_content() -> Result<Content, CaptureError> {
    let (tx, rx) = mpsc::channel::<Result<SendContent, CaptureError>>();
    let handler = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        // SAFETY: ScreenCaptureKit passes a valid object or nil; retaining keeps it alive past the handler.
        let result = match unsafe { Retained::retain(content) } {
            Some(content) => Ok(SendContent(content)),
            None => Err(map_error(error, "SCShareableContent")),
        };
        let _ = tx.send(result);
    });
    // Off-screen windows count too: our overlays are hidden at query time, but the application must still be
    // listed so the filter can exclude the snapshot strip later.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, false, &handler,
        );
    }
    let content = rx
        .recv_timeout(CONTENT_TIMEOUT)
        .map_err(|_| CaptureError::Failed("SCShareableContent did not answer in time".into()))??
        .0;
    let pid = std::process::id() as i32;
    let own_app = unsafe { content.applications() }.iter().find(|app| unsafe { app.processID() } == pid);
    if own_app.is_none() {
        log::warn!("WellSnap is not in the shareable application list; its windows may appear in captures");
    }
    Ok(Content { displays: unsafe { content.displays() }.to_vec(), own_app, own_app_misses: 0 })
}

fn screenshot(
    display: &SCDisplay,
    own_app: Option<&SCRunningApplication>,
    width: u32,
    height: u32,
) -> Result<CFRetained<CGImage>, CaptureError> {
    let excluded = match own_app {
        Some(app) => NSArray::from_slice(&[app]),
        None => NSArray::new(),
    };
    let (tx, rx) = mpsc::channel::<Result<CFRetained<CGImage>, CaptureError>>();
    let handler = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = match NonNull::new(image) {
            // SAFETY: a non-null CGImage from the handler; retaining keeps it alive past the handler.
            Some(image) => Ok(unsafe { CFRetained::retain(image) }),
            None => Err(map_error(error, "SCScreenshotManager")),
        };
        let _ = tx.send(result);
    });
    unsafe {
        let filter = SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &excluded,
            &NSArray::new(),
        );
        let config = SCStreamConfiguration::new();
        config.setWidth(width as usize);
        config.setHeight(height as usize);
        config.setShowsCursor(false);
        config.setPixelFormat(PIXEL_FORMAT_BGRA);
        // The webview treats frame pixels as sRGB; converting here keeps the frozen frame looking like the
        // screen on wide-gamut displays. kCGColorSpaceSRGB is a static constant, so it outlives `config`.
        config.setColorSpaceName(kCGColorSpaceSRGB);
        config.setCaptureResolution(SCCaptureResolutionType::Best);
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&handler));
    }
    rx.recv_timeout(CAPTURE_TIMEOUT)
        .map_err(|_| CaptureError::Failed("ScreenCaptureKit did not return a screenshot in time".into()))?
}

fn map_error(error: *mut NSError, what: &str) -> CaptureError {
    // SAFETY: the completion handlers pass a valid NSError or nil.
    let Some(error) = (unsafe { error.as_ref() }) else {
        return CaptureError::Failed(format!("{what} returned neither a result nor an error"));
    };
    let declined = error.code() == SCStreamErrorCode::UserDeclined.0
        && unsafe { SCStreamErrorDomain }.isEqualToString(&error.domain());
    if declined {
        CaptureError::PermissionDenied
    } else {
        CaptureError::Failed(format!("{what}: {} ({})", error.localizedDescription(), error.code()))
    }
}

/// Tightly packed, top-down, opaque RGBA. SDR captures normally arrive as 32-bit little-endian
/// premultiplied-first sRGB (BGRA in memory), which is repacked and swizzled in one pass; anything else is
/// redrawn into such a bitmap first.
fn image_to_rgba(image: &CGImage) -> Result<(u32, u32, Vec<u8>), CaptureError> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(CaptureError::Failed(format!("image too large: {width}x{height}")));
    };
    if width == 0 || height == 0 {
        return Err(CaptureError::Failed("ScreenCaptureKit returned an empty image".into()));
    }
    if let Some(pixels) = copy_native_rgba(image, width, height) {
        return Ok((w, h, pixels));
    }
    let mut pixels = draw_bgra(image, width, height)?;
    bgra_to_rgba_opaque(&mut pixels);
    Ok((w, h, pixels))
}

fn copy_native_rgba(image: &CGImage, width: usize, height: usize) -> Option<Vec<u8>> {
    let info = CGImage::bitmap_info(Some(image));
    let order = CGImageByteOrderInfo(info.0 & CGBitmapInfo::ByteOrderInfoMask.0);
    let alpha = CGImage::alpha_info(Some(image));
    let bgra_layout = CGImage::bits_per_pixel(Some(image)) == 32
        && CGImage::bits_per_component(Some(image)) == 8
        && order == CGImageByteOrderInfo::Order32Little
        && [CGImageAlphaInfo::PremultipliedFirst, CGImageAlphaInfo::NoneSkipFirst, CGImageAlphaInfo::First]
            .contains(&alpha);
    let srgb = CGImage::color_space(Some(image))
        .and_then(|space| CGColorSpace::name(Some(&space)))
        .is_some_and(|name| *name == *unsafe { kCGColorSpaceSRGB });
    if !bgra_layout || !srgb {
        return None;
    }
    let provider = CGImage::data_provider(Some(image))?;
    let data = CGDataProvider::data(Some(&provider))?;
    // SAFETY: the CFData is an immutable copy owned by `data` for the duration of this borrow.
    let bytes = unsafe { data.as_bytes_unchecked() };
    pack_rows_rgba(bytes, width, height, CGImage::bytes_per_row(Some(image)))
}

fn draw_bgra(image: &CGImage, width: usize, height: usize) -> Result<Vec<u8>, CaptureError> {
    let stride = width * 4;
    let mut pixels = vec![0u8; stride * height];
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or_else(|| CaptureError::Failed("no sRGB color space".into()))?;
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    // SAFETY: `pixels` holds stride * height bytes and outlives the context, which is dropped below.
    let context =
        unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast(), width, height, 8, stride, Some(&space), info) }
            .ok_or_else(|| CaptureError::Failed("CGBitmapContextCreate failed".into()))?;
    // Bitmap context memory starts with the top row, so the result is already top-down.
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    drop(context);
    Ok(pixels)
}

/// Copies `height` rows of `width` BGRA pixels out of a buffer whose rows are `stride` bytes apart into
/// tightly packed RGBA with alpha 255. Split across threads like `bgra_to_rgba_opaque`.
fn pack_rows_rgba(bytes: &[u8], width: usize, height: usize, stride: usize) -> Option<Vec<u8>> {
    let row = width.checked_mul(4)?;
    if row == 0 || stride < row || height == 0 {
        return None;
    }
    let needed = stride.checked_mul(height - 1)?.checked_add(row)?;
    if bytes.len() < needed {
        return None;
    }
    let mut out = vec![0u8; row.checked_mul(height)?];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let rows_per_part = height.div_ceil(threads).max(MIN_PART_BYTES / row).max(1);
    std::thread::scope(|scope| {
        for (part, dst) in out.chunks_mut(rows_per_part * row).enumerate() {
            let first_row = part * rows_per_part;
            scope.spawn(move || {
                for (i, dst_row) in dst.chunks_exact_mut(row).enumerate() {
                    let start = (first_row + i) * stride;
                    let src_row = &bytes[start..start + row];
                    for (d, s) in dst_row.as_chunks_mut::<4>().0.iter_mut().zip(src_row.as_chunks::<4>().0) {
                        *d = [s[2], s[1], s[0], 255];
                    }
                }
            });
        }
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_matching_tolerates_rounding_only() {
        let target = [1512.0, -100.0, 2560.0, 1440.0];
        assert!(frames_match([1512.0, -100.0, 2560.0, 1440.0], target));
        assert!(frames_match([1511.5, -100.5, 2560.0, 1440.0], target));
        assert!(!frames_match([1512.0, -100.0, 1920.0, 1080.0], target));
        assert!(!frames_match([0.0, 0.0, 2560.0, 1440.0], target));
    }

    #[test]
    fn swapped_identical_monitors_do_not_match_the_old_snapshot() {
        let left = [0.0, 0.0, 2560.0, 1440.0];
        let right = [2560.0, 0.0, 2560.0, 1440.0];
        // Display A was queried at the left; the user then moved it to the right.
        assert!(!display_matches(left, right, left));
        assert!(!display_matches(left, right, right));
        assert!(display_matches(left, left, left));
        // A disconnected display reports empty bounds.
        assert!(!display_matches(left, [0.0; 4], left));
    }

    #[test]
    fn missing_own_app_is_refreshed_a_few_times_only() {
        assert!(!own_app_refresh_due(true, 0));
        assert!(own_app_refresh_due(false, 1));
        assert!(own_app_refresh_due(false, OWN_APP_RETRIES - 1));
        assert!(!own_app_refresh_due(false, OWN_APP_RETRIES));
    }

    #[test]
    fn pack_rows_removes_padding_and_swizzles() {
        // 2x2 BGRA pixels, rows padded to 12 bytes.
        let src = [1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0];
        let rgba = vec![3, 2, 1, 255, 7, 6, 5, 255, 11, 10, 9, 255, 15, 14, 13, 255];
        assert_eq!(pack_rows_rgba(&src, 2, 2, 12), Some(rgba));
    }

    #[test]
    fn pack_rows_accepts_unpadded_last_row() {
        let src = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8];
        assert_eq!(pack_rows_rgba(&src, 1, 2, 8), Some(vec![3, 2, 1, 255, 7, 6, 5, 255]));
    }

    #[test]
    fn pack_rows_tight_buffer_is_swizzled() {
        let src: Vec<u8> = (0..32).collect();
        let expected: Vec<u8> = src.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        assert_eq!(pack_rows_rgba(&src, 4, 2, 16), Some(expected));
    }

    #[test]
    fn pack_rows_splits_large_frames_across_threads() {
        // 1024x600 with 64 bytes of padding per row: several MIN_PART_BYTES parts.
        let (width, height, stride) = (1024, 600, 1024 * 4 + 64);
        let src: Vec<u8> = (0..stride * height).map(|i| (i % 251) as u8).collect();
        let out = pack_rows_rgba(&src, width, height, stride).unwrap();
        assert_eq!(out.len(), width * height * 4);
        for (y, x) in [(0, 0), (255, 1023), (256, 0), (599, 1023)] {
            let s = &src[y * stride + x * 4..][..4];
            assert_eq!(out[(y * width + x) * 4..][..4], [s[2], s[1], s[0], 255]);
        }
    }

    #[test]
    fn pack_rows_rejects_short_or_inconsistent_buffers() {
        assert_eq!(pack_rows_rgba(&[0; 15], 2, 2, 8), None);
        assert_eq!(pack_rows_rgba(&[0; 64], 4, 2, 8), None);
        assert_eq!(pack_rows_rgba(&[0; 64], 1, 0, 4), None);
        assert_eq!(pack_rows_rgba(&[0; 64], 0, 2, 4), None);
    }
}
