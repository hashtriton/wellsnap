//! Windows clipboard writer: every format of a payload goes in ONE OpenClipboard / EmptyClipboard /
//! SetClipboardData... / CloseClipboard session.
//!
//! Owner: EmptyClipboard on a clipboard opened with a NULL window sets a NULL owner, and then SetClipboardData
//! fails (EmptyClipboard docs). The owner is a hidden message-only window on its own thread with a message
//! loop: the system sends the owner messages such as WM_DESTROYCLIPBOARD when another app empties the
//! clipboard, and an owner thread that never pumps would make that app wait. Any thread may then open the
//! clipboard with this window, so writes from worker threads are fine.
//!
//! Bitmap: "PNG" (raw bytes) plus CF_DIBV5, 32 bpp, BI_BITFIELDS (masks inside the V5 header, nothing after
//! it), sRGB, bottom-up rows. Bottom-up is the orientation every DIB reader supports; Greenshot's encoder
//! (Dapplo.Windows DibCodec) makes the same choice. Chromium, Firefox and Office read "PNG" first. Qt
//! (Telegram Desktop) prefers an original CF_DIBV5 over "PNG" and locates the pixels at
//! `data size - biSizeImage`, so biSizeImage must be exact; without it Qt assumes 12 bytes of masks after the
//! header and shifts the picture by three pixels. The system synthesizes CF_DIB and CF_BITMAP on demand.

use super::{ClipPayload, ClipboardError};
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WNDCLASSW,
};
use windows::core::{HSTRING, w};

const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(25);

pub fn write(payload: &ClipPayload) -> Result<(), ClipboardError> {
    // Everything is encoded before the clipboard is opened: decoding a 4K PNG takes a while and other apps
    // cannot use the clipboard while it is open.
    let items = build_items(payload)?;
    if items.is_empty() {
        return Err(ClipboardError::Failed("nothing to copy".into()));
    }
    let owner = owner_window()?;
    let _open = OpenClipboardGuard::open(owner)?;
    unsafe { EmptyClipboard() }.map_err(|e| ClipboardError::Failed(format!("EmptyClipboard: {e}")))?;
    for (format, data) in items {
        // After a successful SetClipboardData the system owns the memory; on failure `data` frees it.
        if let Err(e) = unsafe { SetClipboardData(format, Some(HANDLE(data.0.0))) } {
            // All or nothing: drop the formats placed so far, so a later paste never gets half a payload.
            let _ = unsafe { EmptyClipboard() };
            return Err(ClipboardError::Failed(format!("SetClipboardData({format}): {e}")));
        }
        std::mem::forget(data);
    }
    Ok(())
}

/// Clipboard formats in the order they are placed: the most descriptive first, as Microsoft recommends.
fn build_items(payload: &ClipPayload) -> Result<Vec<(u32, Global)>, ClipboardError> {
    let mut items = Vec::new();
    if payload.transient {
        // Clipboard history and Cloud Clipboard formats (learn.microsoft.com, "Clipboard Formats").
        items.push((registered("ExcludeClipboardContentFromMonitorProcessing")?, global_from(&0u32.to_le_bytes())?));
        items.push((registered("CanIncludeInClipboardHistory")?, global_from(&0u32.to_le_bytes())?));
        items.push((registered("CanUploadToCloudClipboard")?, global_from(&0u32.to_le_bytes())?));
    }
    if !payload.files.is_empty() {
        items.push((u32::from(CF_HDROP.0), global_from(&drop_files(&payload.files)?)?));
        // DROPEFFECT_COPY: a paste copies the files instead of moving them.
        items.push((registered("Preferred DropEffect")?, global_from(&1u32.to_le_bytes())?));
    }
    if let Some(png) = &payload.image_png {
        items.push((registered("PNG")?, global_from(png)?));
        let image = decode_png(png).map_err(|e| ClipboardError::Failed(format!("PNG decode: {e}")))?;
        let size = dibv5_size(image.width, image.height)
            .ok_or_else(|| ClipboardError::Failed(format!("bitmap too large: {}x{}", image.width, image.height)))?;
        items.push((u32::from(CF_DIBV5.0), global_with(size, |out| write_dibv5(&image, out))?));
    }
    if let Some(html) = &payload.html {
        items.push((registered("HTML Format")?, global_from(&cf_html(html))?));
    }
    if let Some(text) = &payload.text {
        let units = unicode_text(text);
        let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        items.push((u32::from(CF_UNICODETEXT.0), global_from(&bytes)?));
    }
    Ok(items)
}

fn registered(name: &str) -> Result<u32, ClipboardError> {
    match unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) } {
        0 => Err(ClipboardError::Failed(format!(
            "RegisterClipboardFormat({name}): {}",
            windows::core::Error::from_thread()
        ))),
        id => Ok(id),
    }
}

/// Movable global memory that is freed on drop unless it was handed to the clipboard (`mem::forget`).
struct Global(HGLOBAL);

impl Drop for Global {
    fn drop(&mut self) {
        let _ = unsafe { GlobalFree(Some(self.0)) };
    }
}

fn global_from(bytes: &[u8]) -> Result<Global, ClipboardError> {
    global_with(bytes.len(), |out| out.copy_from_slice(bytes))
}

fn global_with(len: usize, fill: impl FnOnce(&mut [u8])) -> Result<Global, ClipboardError> {
    unsafe {
        // Zeroed so the slice below never covers uninitialized memory.
        let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, len.max(1))
            .map_err(|e| ClipboardError::Failed(format!("GlobalAlloc({len}): {e}")))?;
        let global = Global(handle);
        let ptr = GlobalLock(handle) as *mut u8;
        if ptr.is_null() {
            return Err(ClipboardError::Failed(format!("GlobalLock: {}", windows::core::Error::from_thread())));
        }
        fill(std::slice::from_raw_parts_mut(ptr, len));
        // Reports an "error" with code 0 once the lock count drops to zero, which is the expected outcome.
        let _ = GlobalUnlock(handle);
        Ok(global)
    }
}

struct OpenClipboardGuard;

impl OpenClipboardGuard {
    /// Another app (a clipboard manager, a remote desktop client) may hold the clipboard for a moment.
    fn open(owner: HWND) -> Result<Self, ClipboardError> {
        for attempt in 0..OPEN_ATTEMPTS {
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                std::thread::sleep(OPEN_RETRY_DELAY);
            }
        }
        Err(ClipboardError::Busy)
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// HWND of the clipboard owner window, stored as an integer because HWND is not Send.
static OWNER: Mutex<Option<isize>> = Mutex::new(None);

fn owner_window() -> Result<HWND, ClipboardError> {
    let mut owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
    if owner.is_none() {
        *owner = Some(spawn_owner_thread().map_err(ClipboardError::Failed)?);
    }
    Ok(HWND(owner.unwrap_or_default() as *mut c_void))
}

fn spawn_owner_thread() -> Result<isize, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("clipboard-owner".into())
        .spawn(move || {
            let created = unsafe { create_owner_window() };
            let ok = created.is_ok();
            let _ = tx.send(created.map(|hwnd| hwnd.0 as isize));
            if ok {
                let mut msg = MSG::default();
                // GetMessageW returns -1 on error and 0 on WM_QUIT.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                    unsafe { DispatchMessageW(&msg) };
                }
            }
        })
        .map_err(|e| format!("clipboard owner thread: {e}"))?;
    rx.recv().map_err(|e| format!("clipboard owner thread: {e}"))?
}

unsafe extern "system" fn owner_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn create_owner_window() -> Result<HWND, String> {
    unsafe {
        let module = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandle: {e}"))?;
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(owner_proc),
            hInstance: instance,
            lpszClassName: w!("WellSnapClipboardOwner"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!("RegisterClass: {}", windows::core::Error::from_thread()));
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("WellSnapClipboardOwner"),
            w!("WellSnap clipboard"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
        .map_err(|e| format!("CreateWindowEx: {e}"))
    }
}

/// CF_HDROP: a DROPFILES header (pFiles = 20, fWide = 1) followed by UTF-16 paths, each NUL-terminated,
/// and one more NUL that ends the list.
fn drop_files(files: &[PathBuf]) -> Result<Vec<u8>, ClipboardError> {
    const HEADER: u32 = 20;
    let mut out = Vec::with_capacity(HEADER as usize + files.len() * 128);
    out.extend_from_slice(&HEADER.to_le_bytes()); // pFiles
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for file in files {
        let units: Vec<u16> = file.as_os_str().encode_wide().collect();
        if units.is_empty() || units.contains(&0) {
            return Err(ClipboardError::Failed(format!("invalid file path {file:?}")));
        }
        out.extend(units.iter().chain(&[0]).flat_map(|u| u.to_le_bytes()));
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// CF_UNICODETEXT: UTF-16 with Windows line breaks and a terminating NUL.
fn unicode_text(text: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len() + 8);
    let mut prev = '\0';
    for ch in text.chars() {
        if ch == '\n' && prev != '\r' {
            out.push(u16::from(b'\r'));
        }
        let mut buf = [0u16; 2];
        out.extend_from_slice(ch.encode_utf16(&mut buf));
        prev = ch;
    }
    out.push(0);
    out
}

/// "HTML Format" (CF_HTML): a header with byte offsets into the UTF-8 data, then a document that wraps
/// the fragment in StartFragment / EndFragment comments. Same layout as Chromium writes.
fn cf_html(fragment: &str) -> Vec<u8> {
    const PREFIX: &str = "<html>\r\n<body>\r\n<!--StartFragment-->";
    const SUFFIX: &str = "<!--EndFragment-->\r\n</body>\r\n</html>";
    let header = |start_html: usize, end_html: usize, start_fragment: usize, end_fragment: usize| {
        format!(
            "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
             StartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
        )
    };
    let start_html = header(0, 0, 0, 0).len();
    let start_fragment = start_html + PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + SUFFIX.len();
    let mut out = header(start_html, end_html, start_fragment, end_fragment).into_bytes();
    out.reserve(end_html - start_html + 1);
    out.extend_from_slice(PREFIX.as_bytes());
    out.extend_from_slice(fragment.as_bytes());
    out.extend_from_slice(SUFFIX.as_bytes());
    out.push(0);
    out
}

struct DecodedImage {
    width: u32,
    height: u32,
    /// 1 gray, 2 gray + alpha, 3 RGB, 4 RGBA; 8 bits per sample.
    channels: usize,
    line_size: usize,
    data: Vec<u8>,
}

fn decode_png(png: &[u8]) -> Result<DecodedImage, String> {
    // Our own PNG: the default 64 MiB decoder limit would reject a large multi-monitor capture.
    let mut decoder = png::Decoder::new_with_limits(std::io::Cursor::new(png), png::Limits { bytes: usize::MAX });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut data = vec![0; size];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("indexed colors were not expanded".into()),
    };
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("unexpected bit depth {:?}", info.bit_depth));
    }
    Ok(DecodedImage { width: info.width, height: info.height, channels, line_size: info.line_size, data })
}

const DIBV5_HEADER: usize = 124;

/// Header plus 32 bpp pixels; None when biSizeImage (u32) or the dimensions (i32) cannot hold it.
fn dibv5_size(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || i32::try_from(width).is_err() || i32::try_from(height).is_err() {
        return None;
    }
    let pixels = u32::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    usize::try_from(pixels).ok()?.checked_add(DIBV5_HEADER)
}

/// BITMAPV5HEADER + bottom-up BGRA rows into `out`, which holds exactly `dibv5_size` bytes.
fn write_dibv5(image: &DecodedImage, out: &mut [u8]) {
    let (w, h) = (image.width as usize, image.height as usize);
    let row_bytes = w * 4;
    let put = |out: &mut [u8], at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(out, 0, DIBV5_HEADER as u32); // bV5Size
    put(out, 4, image.width); // bV5Width
    put(out, 8, image.height); // bV5Height > 0: bottom-up
    out[12..14].copy_from_slice(&1u16.to_le_bytes()); // bV5Planes
    out[14..16].copy_from_slice(&32u16.to_le_bytes()); // bV5BitCount
    put(out, 16, 3); // bV5Compression = BI_BITFIELDS
    put(out, 20, (row_bytes * h) as u32); // bV5SizeImage
    put(out, 40, 0x00FF_0000); // bV5RedMask
    put(out, 44, 0x0000_FF00); // bV5GreenMask
    put(out, 48, 0x0000_00FF); // bV5BlueMask
    put(out, 52, 0xFF00_0000); // bV5AlphaMask
    put(out, 56, 0x7352_4742); // bV5CSType = LCS_sRGB ('sRGB')
    put(out, 108, 4); // bV5Intent = LCS_GM_IMAGES

    let pixels = &mut out[DIBV5_HEADER..];
    let ch = image.channels;
    for y in 0..h {
        let src = &image.data[y * image.line_size..y * image.line_size + w * ch];
        let dst_start = (h - 1 - y) * row_bytes;
        let dst = &mut pixels[dst_start..dst_start + row_bytes];
        for (px, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src.chunks_exact(ch)) {
            let (r, g, b, a) = match ch {
                1 => (s[0], s[0], s[0], 255),
                2 => (s[0], s[0], s[0], s[1]),
                3 => (s[0], s[1], s[2], 255),
                _ => (s[0], s[1], s[2], s[3]),
            };
            *px = [b, g, r, a];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER};
    use windows::Win32::UI::Shell::DROPFILES;

    fn encode_png(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        out
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn header_value(data: &[u8], key: &str) -> usize {
        let text = std::str::from_utf8(&data[..105]).unwrap();
        let line = text.lines().find(|l| l.starts_with(key)).unwrap();
        line[key.len() + 1..].parse().unwrap()
    }

    #[test]
    fn drop_files_layout_matches_dropfiles() {
        let files = vec![PathBuf::from(r"C:\Temp\A.png"), PathBuf::from(r"C:\Temp\Б.png")];
        let bytes = drop_files(&files).unwrap();
        assert_eq!(size_of::<DROPFILES>(), 20);
        let header: DROPFILES = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) };
        // DROPFILES is packed: copy the fields out before comparing.
        let (p_files, wide, nc) = ({ header.pFiles }, { header.fWide }, { header.fNC });
        assert_eq!(p_files, 20);
        assert!(wide.as_bool());
        assert!(!nc.as_bool());
        let units: Vec<u16> = bytes[20..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let expected: Vec<u16> =
            r"C:\Temp\A.png".encode_utf16().chain([0]).chain(r"C:\Temp\Б.png".encode_utf16()).chain([0, 0]).collect();
        assert_eq!(units, expected);
    }

    #[test]
    fn drop_files_rejects_nul_and_empty() {
        assert!(drop_files(&[PathBuf::new()]).is_err());
        assert!(drop_files(&[PathBuf::from("a\0b")]).is_err());
    }

    #[test]
    fn cf_html_offsets_point_at_fragment() {
        let fragment = "<p>Снимок <b>A1</b>: кнопка</p>";
        let data = cf_html(fragment);
        assert!(data.starts_with(b"Version:0.9\r\n"));
        assert_eq!(*data.last().unwrap(), 0);
        let start_html = header_value(&data, "StartHTML");
        let end_html = header_value(&data, "EndHTML");
        let start_fragment = header_value(&data, "StartFragment");
        let end_fragment = header_value(&data, "EndFragment");
        assert_eq!(&data[start_fragment..end_fragment], fragment.as_bytes());
        assert!(data[start_html..].starts_with(b"<html>"));
        assert!(data[..end_html].ends_with(b"</html>"));
        assert_eq!(end_html, data.len() - 1);
        assert!(data[..start_fragment].ends_with(b"<!--StartFragment-->"));
        assert!(data[end_fragment..].starts_with(b"<!--EndFragment-->"));
    }

    #[test]
    fn unicode_text_uses_crlf_and_nul() {
        let units = unicode_text("A1\nB\r\nВ");
        assert_eq!(String::from_utf16(&units).unwrap(), "A1\r\nB\r\nВ\0");
        assert_eq!(unicode_text(""), vec![0]);
    }

    #[test]
    fn dibv5_header_and_bottom_up_rows() {
        // 3x2 RGB: top row red, green, blue; bottom row white, black, gray.
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 128, 128, 128];
        let image = decode_png(&encode_png(3, 2, png::ColorType::Rgb, &rgb)).unwrap();
        let size = dibv5_size(3, 2).unwrap();
        assert_eq!(size, 124 + 3 * 2 * 4);
        let mut out = vec![0u8; size];
        write_dibv5(&image, &mut out);

        assert_eq!(size_of::<BITMAPV5HEADER>(), DIBV5_HEADER);
        let header: BITMAPV5HEADER = unsafe { std::ptr::read_unaligned(out.as_ptr().cast()) };
        assert_eq!(header.bV5Size, 124);
        assert_eq!((header.bV5Width, header.bV5Height), (3, 2));
        assert_eq!((header.bV5Planes, header.bV5BitCount), (1, 32));
        assert_eq!(header.bV5Compression, BI_BITFIELDS);
        assert_eq!(header.bV5SizeImage, 24);
        assert_eq!(
            (header.bV5RedMask, header.bV5GreenMask, header.bV5BlueMask, header.bV5AlphaMask),
            (0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000)
        );
        assert_eq!(header.bV5CSType, 0x7352_4742);
        assert_eq!(header.bV5Intent, 4);
        assert_eq!(header.bV5ProfileData, 0);

        let px = |i: usize| u32_at(&out, 124 + i * 4);
        // First stored row is the bottom image row; each pixel reads as 0xAARRGGBB through the masks.
        assert_eq!([px(0), px(1), px(2)], [0xFFFF_FFFF, 0xFF00_0000, 0xFF80_8080]);
        assert_eq!([px(3), px(4), px(5)], [0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF]);
    }

    #[test]
    fn dibv5_keeps_alpha_and_expands_gray() {
        let rgba = [10, 20, 30, 40];
        let image = decode_png(&encode_png(1, 1, png::ColorType::Rgba, &rgba)).unwrap();
        let mut out = vec![0u8; dibv5_size(1, 1).unwrap()];
        write_dibv5(&image, &mut out);
        assert_eq!(&out[124..], &[30, 20, 10, 40]);

        let image = decode_png(&encode_png(2, 1, png::ColorType::Grayscale, &[7, 200])).unwrap();
        let mut out = vec![0u8; dibv5_size(2, 1).unwrap()];
        write_dibv5(&image, &mut out);
        assert_eq!(&out[124..], &[7, 7, 7, 255, 200, 200, 200, 255]);
    }

    #[test]
    fn dibv5_size_limits() {
        assert_eq!(dibv5_size(0, 5), None);
        assert_eq!(dibv5_size(40_000, 40_000), None);
        assert_eq!(dibv5_size(3840, 2160), Some(124 + 3840 * 2160 * 4));
    }

    #[test]
    fn bad_png_is_an_error() {
        assert!(decode_png(b"not a png").is_err());
    }

    /// Writes each payload kind to the real clipboard and reads the formats back.
    /// Replaces the current clipboard content. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn clipboard_round_trip() {
        use windows::Win32::System::DataExchange::{EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW};
        use windows::Win32::System::Memory::GlobalSize;
        use windows::Win32::System::Ole::CF_DIB;

        fn read_clipboard() -> Vec<(u32, String, Vec<u8>)> {
            let _open = OpenClipboardGuard::open(owner_window().unwrap()).unwrap();
            let mut out = Vec::new();
            let mut format = 0;
            loop {
                format = unsafe { EnumClipboardFormats(format) };
                if format == 0 {
                    break;
                }
                let mut name = [0u16; 128];
                let len = unsafe { GetClipboardFormatNameW(format, &mut name) };
                let name = String::from_utf16_lossy(&name[..len.max(0) as usize]);
                let bytes = unsafe {
                    match GetClipboardData(format) {
                        Ok(handle) if format != 2 && format != 3 && format != 14 && format != 9 => {
                            let global = HGLOBAL(handle.0);
                            let size = GlobalSize(global);
                            let ptr = GlobalLock(global) as *const u8;
                            let bytes =
                                if ptr.is_null() { Vec::new() } else { std::slice::from_raw_parts(ptr, size).to_vec() };
                            let _ = GlobalUnlock(global);
                            bytes
                        }
                        _ => Vec::new(),
                    }
                };
                out.push((format, name, bytes));
            }
            out
        }
        fn find(formats: &[(u32, String, Vec<u8>)], id: u32) -> &[u8] {
            &formats.iter().find(|f| f.0 == id).unwrap_or_else(|| panic!("format {id} missing")).2
        }
        fn summary(formats: &[(u32, String, Vec<u8>)]) -> Vec<String> {
            formats.iter().map(|(id, name, bytes)| format!("{id} {name:?} {}B", bytes.len())).collect()
        }

        // Odd width, so the pixel data size is not a multiple of 8 or 16.
        let (w, h) = (37u32, 5u32);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i * 7) as u8, (i * 3) as u8, (i * 11) as u8]).collect();
        let png_bytes = encode_png(w, h, png::ColorType::Rgb, &rgb);
        write(&ClipPayload {
            image_png: Some(png_bytes.clone()),
            text: Some("A1: тест\nA2".into()),
            ..Default::default()
        })
        .unwrap();
        let formats = read_clipboard();
        println!("image payload: {:?}", summary(&formats));
        assert_eq!(find(&formats, registered("PNG").unwrap()), png_bytes.as_slice());
        let dib = find(&formats, u32::from(CF_DIBV5.0));
        let mut expected = vec![0u8; dibv5_size(w, h).unwrap()];
        write_dibv5(&decode_png(&png_bytes).unwrap(), &mut expected);
        assert_eq!(dib.len(), expected.len(), "GlobalSize of CF_DIBV5 must be exact for Qt");
        assert_eq!(dib, expected.as_slice());
        let text = find(&formats, u32::from(CF_UNICODETEXT.0));
        assert_eq!(text, unicode_text("A1: тест\nA2").iter().flat_map(|u| u.to_le_bytes()).collect::<Vec<_>>());
        let synthesized = find(&formats, u32::from(CF_DIB.0));
        println!(
            "synthesized CF_DIB: biSize {} height {} bitcount {} compression {} sizeImage {} total {}B",
            u32_at(synthesized, 0),
            u32_at(synthesized, 8) as i32,
            u16::from_le_bytes([synthesized[14], synthesized[15]]),
            u32_at(synthesized, 16),
            u32_at(synthesized, 20),
            synthesized.len()
        );

        let dir = std::env::temp_dir().join("wellsnap-clipboard-test");
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<PathBuf> = ["A.png", "B.png"].iter().map(|n| dir.join(n)).collect();
        for f in &files {
            std::fs::write(f, &png_bytes).unwrap();
        }
        let html = "<p><b>A1</b> комментарий</p>";
        write(&ClipPayload {
            files: files.clone(),
            text: Some("A1 комментарий".into()),
            html: Some(html.into()),
            ..Default::default()
        })
        .unwrap();
        let formats = read_clipboard();
        println!("files payload: {:?}", summary(&formats));
        assert_eq!(find(&formats, u32::from(CF_HDROP.0)), drop_files(&files).unwrap().as_slice());
        assert_eq!(find(&formats, registered("Preferred DropEffect").unwrap()), &1u32.to_le_bytes());
        assert_eq!(find(&formats, registered("HTML Format").unwrap()), cf_html(html).as_slice());
        assert!(!find(&formats, u32::from(CF_UNICODETEXT.0)).is_empty());

        write(&ClipPayload { text: Some("transient".into()), transient: true, ..Default::default() }).unwrap();
        let formats = read_clipboard();
        println!("transient payload: {:?}", summary(&formats));
        for name in [
            "ExcludeClipboardContentFromMonitorProcessing",
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
        ] {
            assert_eq!(find(&formats, registered(name).unwrap()), &0u32.to_le_bytes(), "{name}");
        }

        // A write from another thread reuses the owner window.
        std::thread::spawn(|| write(&ClipPayload { text: Some("from a worker thread".into()), ..Default::default() }))
            .join()
            .unwrap()
            .unwrap();
        let formats = read_clipboard();
        assert_eq!(
            find(&formats, u32::from(CF_UNICODETEXT.0)),
            unicode_text("from a worker thread").iter().flat_map(|u| u.to_le_bytes()).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
