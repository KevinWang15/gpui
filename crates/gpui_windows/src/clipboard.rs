use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use collections::FxHashMap;
use itertools::Itertools;
use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL, POINT},
    System::{
        DataExchange::{
            CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats,
            GetClipboardData, GetClipboardFormatNameW, OpenClipboard, RegisterClipboardFormatW,
            SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT},
    },
    UI::Shell::{DROPFILES, DragQueryFileW, HDROP},
};
use windows::core::{Owned, PCWSTR};

use gpui::{
    ClipboardEntry, ClipboardItem, ClipboardString, ExternalPaths, Image, ImageFormat,
    SharedString, hash,
};

const DRAGDROP_GET_FILES_COUNT: u32 = 0xFFFFFFFF;

static CLIPBOARD_HASH_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GPUI internal text hash")));
static CLIPBOARD_METADATA_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GPUI internal metadata")));
static CLIPBOARD_SVG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("image/svg+xml")));
static CLIPBOARD_GIF_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GIF")));
static CLIPBOARD_PNG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("PNG")));
static CLIPBOARD_JPG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("JFIF")));

static IMAGE_FORMATS_MAP: LazyLock<FxHashMap<u32, ImageFormat>> = LazyLock::new(|| {
    let mut map = FxHashMap::default();
    map.insert(*CLIPBOARD_PNG_FORMAT, ImageFormat::Png);
    map.insert(*CLIPBOARD_GIF_FORMAT, ImageFormat::Gif);
    map.insert(*CLIPBOARD_JPG_FORMAT, ImageFormat::Jpeg);
    map.insert(*CLIPBOARD_SVG_FORMAT, ImageFormat::Svg);
    map
});

fn register_clipboard_format(format: PCWSTR) -> u32 {
    let ret = unsafe { RegisterClipboardFormatW(format) };
    if ret == 0 {
        panic!(
            "Error when registering clipboard format: {}",
            std::io::Error::last_os_error()
        );
    }
    log::debug!(
        "Registered clipboard format {} as {}",
        unsafe { format.display() },
        ret
    );
    ret
}

fn get_clipboard_data(format: u32) -> Option<LockedGlobal> {
    let global = HGLOBAL(unsafe { GetClipboardData(format).ok() }?.0);
    LockedGlobal::lock(global)
}

pub(crate) fn write_to_clipboard(item: ClipboardItem) {
    let Some(_clip) = ClipboardGuard::open() else {
        return;
    };

    let result: Result<()> = (|| {
        unsafe { EmptyClipboard()? };
        for entry in item.entries() {
            match entry {
                ClipboardEntry::String(string) => write_string(string)?,
                ClipboardEntry::Image(image) => write_image(image)?,
                ClipboardEntry::ExternalPaths(paths) => write_files(paths)?,
                ClipboardEntry::Custom { format, bytes } => write_custom(format.as_ref(), bytes)?,
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        log::error!("Failed to write to clipboard: {e}");
    }
}

pub(crate) fn read_from_clipboard() -> Option<ClipboardItem> {
    let _clip = ClipboardGuard::open()?;

    let mut entries = Vec::new();
    let mut have_text = false;
    let mut have_image = false;
    let mut have_files = false;

    let count = unsafe { CountClipboardFormats() };
    let mut format = 0;
    for _ in 0..count {
        format = unsafe { EnumClipboardFormats(format) };

        if !have_text && format == CF_UNICODETEXT.0 as u32 {
            if let Some(entry) = read_string() {
                entries.push(entry);
                have_text = true;
            }
        } else if !have_image && is_image_format(format) {
            if let Some(entry) = read_image(format) {
                entries.push(entry);
                have_image = true;
            }
        } else if !have_files && format == CF_HDROP.0 as u32 {
            if let Some(entry) = read_files() {
                entries.push(entry);
                have_files = true;
            }
        } else if format >= 0xC000
            && format != *CLIPBOARD_HASH_FORMAT
            && format != *CLIPBOARD_METADATA_FORMAT
            && !is_image_format(format)
        {
            if let Some(entry) = read_custom(format) {
                entries.push(entry);
            }
        }
    }

    if entries.is_empty() {
        log_unsupported_clipboard_formats();
        return None;
    }
    Some(ClipboardItem { entries })
}

pub(crate) fn with_file_names<F>(hdrop: HDROP, mut f: F)
where
    F: FnMut(String),
{
    let file_count = unsafe { DragQueryFileW(hdrop, DRAGDROP_GET_FILES_COUNT, None) };
    for file_index in 0..file_count {
        let filename_length = unsafe { DragQueryFileW(hdrop, file_index, None) } as usize;
        let mut buffer = vec![0u16; filename_length + 1];
        let ret = unsafe { DragQueryFileW(hdrop, file_index, Some(buffer.as_mut_slice())) };
        if ret == 0 {
            log::error!("unable to read file name of dragged file");
            continue;
        }
        match String::from_utf16(&buffer[0..filename_length]) {
            Ok(file_name) => f(file_name),
            Err(e) => log::error!("dragged file name is not UTF-16: {}", e),
        }
    }
}

fn set_clipboard_bytes<T>(data: &[T], format: u32) -> Result<()> {
    unsafe {
        let global = Owned::new(GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(data))?);
        let ptr = GlobalLock(*global);
        anyhow::ensure!(!ptr.is_null(), "GlobalLock returned null");
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as _, data.len());
        GlobalUnlock(*global).ok();
        SetClipboardData(format, Some(HANDLE(global.0)))?;
        // SetClipboardData succeeded — the system now owns the memory.
        std::mem::forget(global);
    }
    Ok(())
}

fn get_clipboard_string(format: u32) -> Option<String> {
    let locked = get_clipboard_data(format)?;
    let bytes = locked.as_bytes();
    let words_len = bytes.len() / std::mem::size_of::<u16>();
    if words_len == 0 {
        return Some(String::new());
    }
    let slice = unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u16, words_len) };
    let actual_len = slice.iter().position(|&c| c == 0).unwrap_or(words_len);
    Some(String::from_utf16_lossy(&slice[..actual_len]))
}

fn is_image_format(format: u32) -> bool {
    IMAGE_FORMATS_MAP.contains_key(&format) || format == CF_DIB.0 as u32
}

fn write_string(item: &ClipboardString) -> Result<()> {
    // CF_UNICODETEXT is null-terminated, so replace embedded NUL characters with spaces.
    let text = item.text.replace('\0', " ").replace("\r\n", "\n");
    // Windows applications expect CRLF line endings in CF_UNICODETEXT.
    let wide: Vec<u16> = text
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(Some(0))
        .collect_vec();
    set_clipboard_bytes(&wide, CF_UNICODETEXT.0 as u32)?;

    if let Some(metadata) = item.metadata.as_ref() {
        let hash_bytes = ClipboardString::text_hash(&text).to_ne_bytes();
        set_clipboard_bytes(&hash_bytes, *CLIPBOARD_HASH_FORMAT)?;

        let wide: Vec<u16> = metadata.encode_utf16().chain(Some(0)).collect_vec();
        set_clipboard_bytes(&wide, *CLIPBOARD_METADATA_FORMAT)?;
    }
    Ok(())
}

fn write_image(item: &Image) -> Result<()> {
    let native_format = match item.format {
        ImageFormat::Svg => Some(*CLIPBOARD_SVG_FORMAT),
        ImageFormat::Gif => Some(*CLIPBOARD_GIF_FORMAT),
        ImageFormat::Png => Some(*CLIPBOARD_PNG_FORMAT),
        ImageFormat::Jpeg => Some(*CLIPBOARD_JPG_FORMAT),
        _ => None,
    };
    if let Some(format) = native_format {
        set_clipboard_bytes(item.bytes(), format)?;
    }

    // Also provide a PNG copy and a CF_DIB fallback for broad compatibility with Windows apps
    // (many classic Win32 apps only read CF_DIB and ignore registered "PNG").
    // SVG can't be rasterized by the image crate, so skip it.
    if item.format != ImageFormat::Svg {
        if native_format != Some(*CLIPBOARD_PNG_FORMAT)
            && let Some(png_bytes) = convert_to_png(item.bytes(), item.format)
        {
            set_clipboard_bytes(&png_bytes, *CLIPBOARD_PNG_FORMAT)?;
        }
        if let Some(dib_bytes) = convert_to_dib(item.bytes(), item.format) {
            let _ = set_clipboard_bytes(&dib_bytes, CF_DIB.0 as u32);
        }
    }
    Ok(())
}

fn write_files(paths: &ExternalPaths) -> Result<()> {
    let items = paths.paths();
    if items.is_empty() {
        return Ok(());
    }
    let header_size = std::mem::size_of::<DROPFILES>();
    let mut wide_paths: Vec<u16> = Vec::new();
    for path in items {
        wide_paths.extend(path.as_os_str().encode_wide());
        wide_paths.push(0);
    }
    // Double-NUL terminator required by CF_HDROP.
    wide_paths.push(0);

    let total_bytes = header_size + wide_paths.len() * std::mem::size_of::<u16>();
    let mut buf = vec![0u8; total_bytes];
    let dropfiles = DROPFILES {
        pFiles: header_size as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: false.into(),
        fWide: true.into(),
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            &dropfiles as *const DROPFILES as *const u8,
            buf.as_mut_ptr(),
            header_size,
        );
        std::ptr::copy_nonoverlapping(
            wide_paths.as_ptr() as *const u8,
            buf.as_mut_ptr().add(header_size),
            wide_paths.len() * std::mem::size_of::<u16>(),
        );
    }
    set_clipboard_bytes(&buf, CF_HDROP.0 as u32)
}

fn write_custom(format: &str, bytes: &[u8]) -> Result<()> {
    let wide: Vec<u16> = OsStr::new(format).encode_wide().chain([0]).collect();
    let fmt = unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) };
    anyhow::ensure!(fmt != 0, "RegisterClipboardFormatW failed for {format:?}");
    set_clipboard_bytes(bytes, fmt)
}

fn read_custom(format: u32) -> Option<ClipboardEntry> {
    let mut name_buf = [0u16; 128];
    let len = unsafe { GetClipboardFormatNameW(format, &mut name_buf) } as usize;
    if len == 0 {
        return None;
    }
    let name = String::from_utf16_lossy(&name_buf[..len]);
    // Skip standard Windows shell / OLE metadata formats that aren't app-specific binary data.
    if matches!(
        name.as_str(),
        "DataObject"
            | "Ole Private Data"
            | "Preferred DropEffect"
            | "Shell IDList Array"
            | "Rich Text Format"
            | "HTML Format"
            | "CanIncludeInClipboardHistory"
            | "CanUploadClipboardToCloud"
            | "ExcludeClipboardContentFromMonitorProcessing"
            | "Clipboard Viewer Ignore"
    ) {
        return None;
    }
    let locked = get_clipboard_data(format)?;
    // Cap custom format reads to 16 MiB to avoid pathological memory copies.
    if locked.size == 0 || locked.size > 16 * 1024 * 1024 {
        return None;
    }
    Some(ClipboardEntry::Custom {
        format: SharedString::from(name),
        bytes: locked.as_bytes().to_vec(),
    })
}

fn convert_to_dib(bytes: &[u8], format: ImageFormat) -> Option<Vec<u8>> {
    let img_format = gpui_to_image_format(format)?;
    let image = image::load_from_memory_with_format(bytes, img_format).ok()?;
    let mut bmp = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .ok()?;
    // Strip the 14-byte BITMAPFILEHEADER so CF_DIB starts at BITMAPINFOHEADER.
    (bmp.len() > 14).then(|| bmp[14..].to_vec())
}

fn convert_to_png(bytes: &[u8], format: ImageFormat) -> Option<Vec<u8>> {
    let img_format = gpui_to_image_format(format)?;
    let image = image::load_from_memory_with_format(bytes, img_format)
        .map_err(|e| log::warn!("Failed to decode image for PNG conversion: {e}"))
        .ok()?;
    let mut buf = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| log::warn!("Failed to encode PNG: {e}"))
        .ok()?;
    Some(buf)
}

fn read_string() -> Option<ClipboardEntry> {
    // Normalize to LF so that apps the same line endings on every platform.
    let text = get_clipboard_string(CF_UNICODETEXT.0 as u32)?.replace("\r\n", "\n");
    let metadata = read_clipboard_metadata(&text);
    Some(ClipboardEntry::String(ClipboardString { text, metadata }))
}

fn read_clipboard_metadata(text: &str) -> Option<String> {
    let locked = get_clipboard_data(*CLIPBOARD_HASH_FORMAT)?;
    let hash_bytes: [u8; 8] = locked.as_bytes().get(..8)?.try_into().ok()?;
    let hash = u64::from_ne_bytes(hash_bytes);
    if hash != ClipboardString::text_hash(text) {
        return None;
    }
    get_clipboard_string(*CLIPBOARD_METADATA_FORMAT)
}

fn read_image(format: u32) -> Option<ClipboardEntry> {
    let locked = get_clipboard_data(format)?;
    let (bytes, image_format) = if format == CF_DIB.0 as u32 {
        (convert_dib_to_bmp(locked.as_bytes())?, ImageFormat::Bmp)
    } else {
        let image_format = *IMAGE_FORMATS_MAP.get(&format)?;
        (locked.as_bytes().to_vec(), image_format)
    };
    let id = hash(&bytes);
    Some(ClipboardEntry::Image(Image {
        format: image_format,
        bytes,
        id,
    }))
}

fn read_files() -> Option<ClipboardEntry> {
    let locked = get_clipboard_data(CF_HDROP.0 as u32)?;
    let hdrop = HDROP(locked.ptr as *mut _);
    let mut filenames = Vec::new();
    with_file_names(hdrop, |name| filenames.push(std::path::PathBuf::from(name)));
    Some(ClipboardEntry::ExternalPaths(ExternalPaths(
        filenames.into(),
    )))
}

/// DIB is BMP without the 14-byte BITMAPFILEHEADER. Prepend one.
fn convert_dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    const BITMAPFILEHEADER_SIZE: u32 = 14;
    const BITMAPINFOHEADER_SIZE: u32 = 40;
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;

    if dib.len() < BITMAPINFOHEADER_SIZE as usize {
        return None;
    }

    let header_size = u32::from_le_bytes(dib[0..4].try_into().ok()?);
    if header_size < BITMAPINFOHEADER_SIZE || (header_size as usize) > dib.len() {
        return None;
    }
    let bit_count = u16::from_le_bytes(dib[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(dib[16..20].try_into().ok()?);
    let colors_used = u32::from_le_bytes(dib[32..36].try_into().ok()?);

    // For a 40-byte BITMAPINFOHEADER, BI_BITFIELDS / BI_ALPHABITFIELDS color masks follow
    // immediately after the header. Extended headers (BITMAPV2/V3/V4/V5, size > 40) already
    // embed those channel masks within `header_size`.
    let bitfield_masks_size = if header_size == BITMAPINFOHEADER_SIZE && bit_count > 8 {
        match compression {
            BI_BITFIELDS => 12u32,
            BI_ALPHABITFIELDS => 16u32,
            _ => 0u32,
        }
    } else {
        0u32
    };

    let palette_entries = if bit_count > 0 && bit_count <= 8 {
        if colors_used == 0 {
            1u32 << bit_count
        } else {
            colors_used
        }
    } else {
        colors_used
    };
    let color_table_size = palette_entries.checked_mul(4)?;

    let pixel_offset = BITMAPFILEHEADER_SIZE
        .checked_add(header_size)?
        .checked_add(bitfield_masks_size)?
        .checked_add(color_table_size)?;
    let file_size = u32::try_from(dib.len())
        .ok()?
        .checked_add(BITMAPFILEHEADER_SIZE)?;
    if pixel_offset > file_size {
        return None;
    }

    let mut bmp = Vec::with_capacity(file_size as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]); // reserved
    bmp.extend_from_slice(&pixel_offset.to_le_bytes());
    bmp.extend_from_slice(dib);

    // Many Windows apps write 32-bit BI_RGB DIBs with every alpha byte set to 0 (meaning opaque
    // BGRX). If all alpha bytes are 0, normalize them to 255 so image decoding doesn't produce a
    // fully transparent image.
    if bit_count == 32 && compression == BI_RGB {
        let off = pixel_offset as usize;
        if off < bmp.len() {
            let pixels = &mut bmp[off..];
            let all_zero_alpha = pixels.chunks_exact(4).all(|px| px[3] == 0);
            if all_zero_alpha {
                for px in pixels.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }
        }
    }
    Some(bmp)
}

fn log_unsupported_clipboard_formats() {
    let count = unsafe { CountClipboardFormats() };
    let mut format = 0;
    for _ in 0..count {
        format = unsafe { EnumClipboardFormats(format) };
        let mut buffer = [0u16; 64];
        unsafe { GetClipboardFormatNameW(format, &mut buffer) };
        let format_name = String::from_utf16_lossy(&buffer);
        log::warn!(
            "Try to paste with unsupported clipboard format: {}, {}.",
            format,
            format_name
        );
    }
}

fn gpui_to_image_format(value: ImageFormat) -> Option<image::ImageFormat> {
    match value {
        ImageFormat::Png => Some(image::ImageFormat::Png),
        ImageFormat::Jpeg => Some(image::ImageFormat::Jpeg),
        ImageFormat::Webp => Some(image::ImageFormat::WebP),
        ImageFormat::Gif => Some(image::ImageFormat::Gif),
        ImageFormat::Bmp => Some(image::ImageFormat::Bmp),
        ImageFormat::Tiff => Some(image::ImageFormat::Tiff),
        other => {
            log::warn!("No image crate equivalent for format: {other:?}");
            None
        }
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Option<Self> {
        // Retry briefly when another process (Clipboard History, RDP, Excel) holds the clipboard
        // lock momentarily.
        const DELAYS_MS: [u64; 10] = [0, 5, 10, 15, 25, 40, 60, 80, 100, 120];
        let mut last_err = None;
        for &delay_ms in &DELAYS_MS {
            if delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Some(Self),
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(e) = last_err {
            log::error!("Failed to open clipboard after retries: {e}");
        }
        None
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        if let Err(e) = unsafe { CloseClipboard() } {
            log::error!("Failed to close clipboard: {e}");
        }
    }
}

struct LockedGlobal {
    global: HGLOBAL,
    ptr: *const u8,
    size: usize,
}

impl LockedGlobal {
    fn lock(global: HGLOBAL) -> Option<Self> {
        let size = unsafe { GlobalSize(global) };
        let ptr = unsafe { GlobalLock(global) };
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            global,
            ptr: ptr as *const u8,
            size,
        })
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }
}

impl Drop for LockedGlobal {
    fn drop(&mut self) {
        unsafe { GlobalUnlock(self.global).ok() };
    }
}

#[cfg(test)]
pub(crate) fn read_raw_clipboard_text() -> Option<String> {
    let _clip = ClipboardGuard::open()?;
    get_clipboard_string(CF_UNICODETEXT.0 as u32)
}

#[cfg(test)]
mod tests {
    use super::convert_dib_to_bmp;

    fn write_base_header(buf: &mut [u8], header_size: u32, bit_count: u16, compression: u32, image_size: u32) {
        buf[0..4].copy_from_slice(&header_size.to_le_bytes());
        buf[4..8].copy_from_slice(&1i32.to_le_bytes()); // width
        buf[8..12].copy_from_slice(&1i32.to_le_bytes()); // height
        buf[12..14].copy_from_slice(&1u16.to_le_bytes()); // planes
        buf[14..16].copy_from_slice(&bit_count.to_le_bytes());
        buf[16..20].copy_from_slice(&compression.to_le_bytes());
        buf[20..24].copy_from_slice(&image_size.to_le_bytes());
    }

    #[test]
    fn convert_dib_v1_bitfields_includes_12_byte_masks() {
        let mut dib = vec![0u8; 40 + 12 + 4];
        write_base_header(&mut dib, 40, 32, 3, 4);
        dib[40..44].copy_from_slice(&0x00ff_0000u32.to_le_bytes()); // R
        dib[44..48].copy_from_slice(&0x0000_ff00u32.to_le_bytes()); // G
        dib[48..52].copy_from_slice(&0x0000_00ffu32.to_le_bytes()); // B
        dib[52..56].copy_from_slice(&[56, 34, 12, 0]); // BGRX -> RGB(12, 34, 56)

        let bmp = convert_dib_to_bmp(&dib).expect("valid v1 bitfields DIB");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 14 + 40 + 12);
        let decoded = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
            .expect("decodes v1 bitfields BMP")
            .to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([12, 34, 56, 255]));
    }

    #[test]
    fn convert_dib_v4_and_v5_bitfields_do_not_double_count_masks() {
        for header_size in [108u32, 124u32] {
            let mut dib = vec![0u8; header_size as usize + 4];
            write_base_header(&mut dib, header_size, 32, 3, 4);
            // Channel masks live inside the V4/V5 header at offsets 40..56.
            dib[40..44].copy_from_slice(&0x00ff_0000u32.to_le_bytes()); // R
            dib[44..48].copy_from_slice(&0x0000_ff00u32.to_le_bytes()); // G
            dib[48..52].copy_from_slice(&0x0000_00ffu32.to_le_bytes()); // B
            dib[52..56].copy_from_slice(&0xff00_0000u32.to_le_bytes()); // A
            dib[header_size as usize..header_size as usize + 4]
                .copy_from_slice(&[56, 34, 12, 255]); // BGRA -> RGBA(12, 34, 56, 255)

            let bmp = convert_dib_to_bmp(&dib).expect("valid extended bitfields DIB");
            assert_eq!(
                u32::from_le_bytes(bmp[10..14].try_into().unwrap()),
                14 + header_size
            );
            let decoded = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
                .expect("decodes extended bitfields BMP")
                .to_rgba8();
            assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([12, 34, 56, 255]));
        }
    }

    #[test]
    fn convert_dib_32bit_bi_rgb_normalizes_all_zero_alpha() {
        let mut dib = vec![0u8; 40 + 4];
        write_base_header(&mut dib, 40, 32, 0, 4);
        dib[40..44].copy_from_slice(&[56, 34, 12, 0]);

        let bmp = convert_dib_to_bmp(&dib).expect("valid 32-bit BI_RGB DIB");
        let decoded = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
            .expect("decodes 32-bit BI_RGB BMP")
            .to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([12, 34, 56, 255]));
    }

    #[test]
    fn convert_dib_rejects_malformed_or_overflowing_headers() {
        assert!(convert_dib_to_bmp(&[0u8; 20]).is_none());

        let mut bad_header_size = vec![0u8; 44];
        write_base_header(&mut bad_header_size, 12, 32, 0, 4);
        assert!(convert_dib_to_bmp(&bad_header_size).is_none());

        let mut header_past_end = vec![0u8; 44];
        write_base_header(&mut header_past_end, 124, 32, 3, 4);
        assert!(convert_dib_to_bmp(&header_past_end).is_none());

        let mut palette_overflow = vec![0u8; 44];
        write_base_header(&mut palette_overflow, 40, 8, 0, 4);
        palette_overflow[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(convert_dib_to_bmp(&palette_overflow).is_none());
    }
}
