use anyhow::Result;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdfKind {
    Module,
    Sample,
    Archive,
    Bitmap,
    Animation,
    Other,
}

#[derive(Debug, Clone)]
pub struct IdInfo {
    pub format: String,
    pub mime_type: String,
    pub detail: String,
    pub kind: IdfKind,
    pub title: Option<String>,
    pub composer: Option<String>,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    size: u64,
    modified: Option<i64>,
}

fn cache() -> &'static Mutex<HashMap<CacheKey, Option<IdInfo>>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, Option<IdInfo>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn probe_path(path: &Path) -> Option<IdInfo> {
    let meta = fs::metadata(path).ok()?;
    if meta.is_dir() {
        return Some(IdInfo {
            format: "Directory".into(),
            mime_type: "inode/directory".into(),
            detail: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: IdfKind::Other,
            title: None,
            composer: None,
            extra: Vec::new(),
        });
    }

    let modified = meta
        .modified()
        .ok()
        .map(DateTime::<Local>::from)
        .map(|dt| dt.timestamp());
    let key = CacheKey {
        path: path.to_path_buf(),
        size: meta.len(),
        modified,
    };

    if let Ok(guard) = cache().lock()
        && let Some(hit) = guard.get(&key)
    {
        return hit.clone();
    }

    let probed = probe_file(path)
        .ok()
        .flatten()
        .or_else(|| Some(fallback_info(path)));
    if let Ok(mut guard) = cache().lock() {
        guard.insert(key, probed.clone());
    }
    probed
}

pub fn render_idf_card(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let info = probe_path(path)?;
    let mut out = String::new();
    out.push_str("Ketchup Killers IDF\n");
    out.push('\n');
    if let Some(title) = info.title.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("Title: {}\n", clean_field(title)));
    }
    out.push_str(&format!(
        "Type: {}\n",
        clean_field(&info.format),
    ));
     out.push_str(&format!(
        "Mime: {}\n",
        clean_field(&info.mime_type)
    ));
    out.push_str(&format!("Name: {}\n", clean_field(&info.detail)));
    if let Some(composer) = info.composer.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("Composer: {}\n", clean_field(composer)));
    }
    if !info.extra.is_empty() {
        out.push('\n');
        for line in info.extra.iter().take(8) {
            out.push_str(&format!("{}\n", clean_field(line)));
        }
    }
    out.push('\n');
    out.push_str(&format!("True Size: {} bytes\n", meta.len()));
    Some(out)
}

fn probe_file(path: &Path) -> Result<Option<IdInfo>> {
    let data = fs::read(path)?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let info = if data.starts_with(b"PK\x03\x04")
        || data.starts_with(b"PK\x05\x06")
        || data.starts_with(b"PK\x07\x08")
    {
        let mime_type = zip_mime_type(&data, &ext).unwrap_or("application/zip");
        let kind = if is_office_mime_type(mime_type) {
            IdfKind::Other
        } else {
            IdfKind::Archive
        };
        Some(info(mime_type, path, kind, None, None, vec![]))
    } else if data.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(info(
            "application/x-7z-compressed",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x1F, 0x8B]) {
        Some(info(
            "application/gzip",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"BZh") {
        Some(info(
            "application/x-bzip2",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Some(info(
            "application/x-xz",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"\x60\xEA") {
        Some(info(
            "application/x-arj",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"MSCF") {
        Some(info(
            "application/vnd.ms-cab-compressed",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Some(info(
            "application/zstd",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"-lh") && data.get(6) == Some(&b'-') {
        Some(info(
            "application/x-lzh-compressed",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if is_tar(&data) {
        Some(info(
            "application/x-tar",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if is_iso9660(&data) {
        Some(info(
            "application/x-iso9660-image",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"Rar!\x1A\x07\x00") {
        Some(info(
            "application/vnd.rar",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"Rar!\x1A\x07\x01\x00") {
        Some(info(
            "application/vnd.rar",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"\x89PNG\r\n\x1A\n") {
        let (w, h) = png_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/png",
            path,
            IdfKind::Bitmap,
            None,
            None,
            wh_lines(w, h),
        ))
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        let (w, h) = webp_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/webp",
            path,
            IdfKind::Bitmap,
            None,
            None,
            wh_lines(w, h),
        ))
    } else if data.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some(info(
            "image/vnd.microsoft.icon",
            path,
            IdfKind::Bitmap,
            None,
            None,
            ico_lines(&data),
        ))
    } else if is_pcx(&data) {
        Some(info(
            "image/x-pcx",
            path,
            IdfKind::Bitmap,
            None,
            None,
            pcx_lines(&data),
        ))
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        let (w, h) = gif_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/gif",
            path,
            IdfKind::Bitmap,
            None,
            None,
            wh_lines(w, h),
        ))
    } else if data.starts_with(b"\xFF\xD8\xFF") {
        let (w, h) = jpeg_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/jpeg",
            path,
            IdfKind::Bitmap,
            None,
            None,
            wh_lines(w, h),
        ))
    } else if data.starts_with(b"BM") {
        let (w, h) = bmp_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/bmp",
            path,
            IdfKind::Bitmap,
            None,
            None,
            wh_lines(w, h),
        ))
    } else if is_tiff(&data) {
        Some(info(
            "image/tiff",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"8BPS") {
        Some(info(
            "image/vnd.adobe.photoshop",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if is_tga(&data, &ext) {
        Some(info(
            "image/x-tga",
            path,
            IdfKind::Bitmap,
            None,
            None,
            tga_lines(&data),
        ))
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
        Some(wav_info(path, &data))
    } else if data.starts_with(b"FORM") && data.get(8..12) == Some(b"AIFF") {
        Some(info(
            "audio/aiff",
            path,
            IdfKind::Sample,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b".snd") {
        Some(info(
            "audio/basic",
            path,
            IdfKind::Sample,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"AVI ") {
        Some(info(
            "video/x-msvideo",
            path,
            IdfKind::Animation,
            None,
            None,
            vec![],
        ))
    } else if data.len() > 12 && &data[4..8] == b"ftyp" {
        Some(info(
            "video/mp4",
            path,
            IdfKind::Animation,
            None,
            None,
            mp4_lines(&data),
        ))
    } else if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some(info(
            "video/x-matroska",
            path,
            IdfKind::Animation,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"fLaC") {
        Some(info(
            "audio/flac",
            path,
            IdfKind::Sample,
            None,
            None,
            flac_lines(&data),
        ))
    } else if data.starts_with(b"OggS") {
        Some(info(
            "application/ogg",
            path,
            IdfKind::Sample,
            None,
            None,
            ogg_lines(&data),
        ))
    } else if data.starts_with(b"ID3") {
        Some(info(
            "audio/mpeg",
            path,
            IdfKind::Sample,
            id3v1_title(&data),
            None,
            vec![],
        ))
    } else if data.starts_with(b"MThd") {
        Some(midi_info(path, &data))
    } else if data.starts_with(b"%PDF-") {
        Some(info(
            "application/pdf",
            path,
            IdfKind::Other,
            None,
            None,
            pdf_lines(&data),
        ))
    } else if data.starts_with(b"{\\rtf") {
        Some(info(
            "application/rtf",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"MZ") {
        Some(info(
            "application/vnd.microsoft.portable-executable",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"\x7FELF") {
        Some(info(
            "application/x-elf",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if is_s3m(&data) {
        Some(info(
            "audio/x-s3m",
            path,
            IdfKind::Module,
            fixed_text(&data[..28]),
            None,
            vec![],
        ))
    } else if is_xm(&data) {
        Some(info(
            "audio/x-xm",
            path,
            IdfKind::Module,
            fixed_text(&data[17..37]),
            tracker_name(&data, 38, 58),
            vec![],
        ))
    } else if is_it(&data) {
        Some(info(
            "audio/x-it",
            path,
            IdfKind::Module,
            fixed_text(&data[4..30]),
            None,
            vec![],
        ))
    } else if is_mod(&data) {
        Some(info(
            "audio/x-mod",
            path,
            IdfKind::Module,
            fixed_text(&data[..20]),
            None,
            vec![],
        ))
    } else if matches!(ext.as_str(), "htm" | "html") || looks_like_html(&data) {
        Some(info(
            "text/html",
            path,
            IdfKind::Other,
            html_title(&data),
            None,
            vec![],
        ))
    } else if matches!(ext.as_str(), "ans" | "nfo" | "diz") {
        Some(info("text/plain", path, IdfKind::Other, None, None, vec![]))
    } else if seems_text(&data) {
        Some(info("text/plain", path, IdfKind::Other, None, None, vec![]))
    } else {
        None
    };

    Ok(info)
}

fn info(
    mime_type: &str,
    path: &Path,
    kind: IdfKind,
    title: Option<String>,
    composer: Option<String>,
    extra: Vec<String>,
) -> IdInfo {
    IdInfo {
        format: format_from_mime_type(mime_type)
            .unwrap_or("Unknown file")
            .into(),
        mime_type: mime_type.into(),
        detail: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        kind,
        title,
        composer,
        extra,
    }
}

fn fallback_info(path: &Path) -> IdInfo {
    IdInfo {
        format: "Unknown file".into(),
        mime_type: fallback_mime_type(path),
        detail: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        kind: IdfKind::Other,
        title: None,
        composer: None,
        extra: Vec::new(),
    }
}

fn fallback_mime_type(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!("application/{ext}"))
        .unwrap_or_else(|| "application/octet-stream".into())
}

fn format_from_mime_type(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "application/zip" => Some("ZIP archive"),
        "application/x-7z-compressed" => Some("7-Zip archive"),
        "application/gzip" => Some("GZip archive"),
        "application/x-bzip2" => Some("BZip2 archive"),
        "application/x-xz" => Some("XZ archive"),
        "application/x-arj" => Some("ARJ archive"),
        "application/vnd.ms-cab-compressed" => Some("CAB archive"),
        "application/zstd" => Some("Zstandard archive"),
        "application/x-lzh-compressed" => Some("LHA/LZH archive"),
        "application/x-tar" => Some("TAR archive"),
        "application/x-iso9660-image" => Some("ISO-9660 image"),
        "application/vnd.rar" => Some("RAR archive"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Some("Microsoft Word document")
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            Some("Microsoft Excel spreadsheet")
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            Some("Microsoft PowerPoint presentation")
        }
        "application/vnd.ms-word.document.macroEnabled.12" => Some("Microsoft Word document"),
        "application/vnd.ms-excel.sheet.macroEnabled.12" => Some("Microsoft Excel spreadsheet"),
        "application/vnd.ms-powerpoint.presentation.macroEnabled.12" => {
            Some("Microsoft PowerPoint presentation")
        }
        "application/vnd.oasis.opendocument.text" => Some("OpenDocument text"),
        "application/vnd.oasis.opendocument.spreadsheet" => Some("OpenDocument spreadsheet"),
        "application/vnd.oasis.opendocument.presentation" => Some("OpenDocument presentation"),
        "image/png" => Some("PNG bitmap"),
        "image/webp" => Some("WebP bitmap"),
        "image/vnd.microsoft.icon" => Some("ICO bitmap"),
        "image/x-pcx" => Some("PCX bitmap"),
        "image/gif" => Some("GIF bitmap"),
        "image/jpeg" => Some("JPEG bitmap"),
        "image/bmp" => Some("BMP bitmap"),
        "image/tiff" => Some("TIFF bitmap"),
        "image/vnd.adobe.photoshop" => Some("Photoshop bitmap"),
        "image/x-tga" => Some("TGA bitmap"),
        "audio/wav" => Some("WAV sample"),
        "audio/aiff" => Some("AIFF audio"),
        "audio/basic" => Some("AU audio"),
        "video/x-msvideo" => Some("AVI animation"),
        "video/mp4" => Some("MP4/MOV container"),
        "video/x-matroska" => Some("Matroska container"),
        "audio/flac" => Some("FLAC audio"),
        "application/ogg" => Some("Ogg stream"),
        "audio/mpeg" => Some("MP3 audio"),
        "audio/midi" => Some("MIDI song"),
        "application/pdf" => Some("PDF document"),
        "application/rtf" => Some("RTF document"),
        "application/vnd.microsoft.portable-executable" => Some("DOS/Windows executable"),
        "application/x-elf" => Some("ELF executable"),
        "audio/x-s3m" => Some("Scream Tracker module"),
        "audio/x-xm" => Some("FastTracker module"),
        "audio/x-it" => Some("Impulse Tracker module"),
        "audio/x-mod" => Some("ProTracker module"),
        "text/html" => Some("HTML document"),
        "text/plain" => Some("Text file"),
        _ => None,
    }
}

fn zip_mime_type(data: &[u8], ext: &str) -> Option<&'static str> {
    let mut archive = ZipArchive::new(Cursor::new(data)).ok()?;
    let mut has_content_types = false;
    let mut has_word = false;
    let mut has_excel = false;
    let mut has_powerpoint = false;
    let mut has_macro_word = false;
    let mut has_macro_excel = false;
    let mut has_macro_powerpoint = false;
    let mut odf_mime = None;

    for idx in 0..archive.len().min(256) {
        let Ok(mut file) = archive.by_index(idx) else {
            continue;
        };
        let name = file.name().to_ascii_lowercase();
        match name.as_str() {
            "[content_types].xml" => has_content_types = true,
            "word/document.xml" => has_word = true,
            "xl/workbook.xml" => has_excel = true,
            "ppt/presentation.xml" => has_powerpoint = true,
            "word/vbaproject.bin" => has_macro_word = true,
            "xl/vbaproject.bin" => has_macro_excel = true,
            "ppt/vbaproject.bin" => has_macro_powerpoint = true,
            "mimetype" => {
                use std::io::Read;
                let mut value = String::new();
                if file.read_to_string(&mut value).is_ok() {
                    odf_mime = Some(value.trim().to_string());
                }
            }
            _ => {}
        }
    }

    if let Some(mime) = odf_mime.as_deref().and_then(open_document_mime_type) {
        return Some(mime);
    }
    if has_content_types {
        if has_word {
            return Some(if has_macro_word || ext == "docm" {
                "application/vnd.ms-word.document.macroEnabled.12"
            } else {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            });
        }
        if has_excel {
            return Some(if has_macro_excel || ext == "xlsm" {
                "application/vnd.ms-excel.sheet.macroEnabled.12"
            } else {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            });
        }
        if has_powerpoint {
            return Some(if has_macro_powerpoint || ext == "pptm" {
                "application/vnd.ms-powerpoint.presentation.macroEnabled.12"
            } else {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            });
        }
    }

    match ext {
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "docm" => Some("application/vnd.ms-word.document.macroEnabled.12"),
        "xlsm" => Some("application/vnd.ms-excel.sheet.macroEnabled.12"),
        "pptm" => Some("application/vnd.ms-powerpoint.presentation.macroEnabled.12"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        _ => None,
    }
}

fn open_document_mime_type(value: &str) -> Option<&'static str> {
    match value {
        "application/vnd.oasis.opendocument.text" => {
            Some("application/vnd.oasis.opendocument.text")
        }
        "application/vnd.oasis.opendocument.spreadsheet" => {
            Some("application/vnd.oasis.opendocument.spreadsheet")
        }
        "application/vnd.oasis.opendocument.presentation" => {
            Some("application/vnd.oasis.opendocument.presentation")
        }
        _ => None,
    }
}

fn is_office_mime_type(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.ms-word.document.macroEnabled.12"
            | "application/vnd.ms-excel.sheet.macroEnabled.12"
            | "application/vnd.ms-powerpoint.presentation.macroEnabled.12"
            | "application/vnd.oasis.opendocument.text"
            | "application/vnd.oasis.opendocument.spreadsheet"
            | "application/vnd.oasis.opendocument.presentation"
    )
}

fn wh_lines(w: u32, h: u32) -> Vec<String> {
    if w > 0 && h > 0 {
        vec![format!(" {} x {} pixels", w, h)]
    } else {
        Vec::new()
    }
}

fn pdf_lines(data: &[u8]) -> Vec<String> {
    let sample = String::from_utf8_lossy(&data[..data.len().min(16)]);
    sample
        .strip_prefix("%PDF-")
        .map(|v| vec![format!(" PDF version {}", v.trim())])
        .unwrap_or_default()
}

fn midi_info(path: &Path, data: &[u8]) -> IdInfo {
    let mut extra = Vec::new();
    if data.len() >= 14 {
        let format = u16::from_be_bytes([data[8], data[9]]);
        let tracks = u16::from_be_bytes([data[10], data[11]]);
        extra.push(format!(" MIDI format {}", format));
        extra.push(format!(" {} track(s)", tracks));
    }
    info("audio/midi", path, IdfKind::Module, None, None, extra)
}

fn wav_info(path: &Path, data: &[u8]) -> IdInfo {
    let mut extra = Vec::new();
    if let Some((channels, rate, bits)) = wav_fmt(data) {
        extra.push(format!(" {} Hz", rate));
        extra.push(format!(" {} channel(s)", channels));
        extra.push(format!(" {} bit", bits));
    }
    info("audio/wav", path, IdfKind::Sample, None, None, extra)
}

fn wav_fmt(data: &[u8]) -> Option<(u16, u32, u16)> {
    let mut i = 12usize;
    while i + 8 <= data.len() {
        let id = &data[i..i + 4];
        let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
        i += 8;
        if id == b"fmt " && i + len <= data.len() && len >= 16 {
            let channels = u16::from_le_bytes(data[i + 2..i + 4].try_into().ok()?);
            let rate = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?);
            let bits = u16::from_le_bytes(data[i + 14..i + 16].try_into().ok()?);
            return Some((channels, rate, bits));
        }
        i += len + (len & 1);
    }
    None
}

fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 24 {
        Some((
            u32::from_be_bytes(data[16..20].try_into().ok()?),
            u32::from_be_bytes(data[20..24].try_into().ok()?),
        ))
    } else {
        None
    }
}

fn gif_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 10 {
        Some((
            u16::from_le_bytes(data[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(data[8..10].try_into().ok()?) as u32,
        ))
    } else {
        None
    }
}

fn bmp_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 26 {
        Some((
            i32::from_le_bytes(data[18..22].try_into().ok()?).unsigned_abs(),
            i32::from_le_bytes(data[22..26].try_into().ok()?).unsigned_abs(),
        ))
    } else {
        None
    }
}

fn webp_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 {
        return None;
    }
    match data.get(12..16)? {
        b"VP8 " => {
            if data.len() >= 30 {
                let w = u16::from_le_bytes(data[26..28].try_into().ok()?) as u32 & 0x3fff;
                let h = u16::from_le_bytes(data[28..30].try_into().ok()?) as u32 & 0x3fff;
                Some((w, h))
            } else {
                None
            }
        }
        b"VP8L" => {
            if data.len() >= 25 {
                let b0 = data[21] as u32;
                let b1 = data[22] as u32;
                let b2 = data[23] as u32;
                let b3 = data[24] as u32;
                let w = 1 + (b0 | ((b1 & 0x3F) << 8));
                let h = 1 + ((b1 >> 6) | (b2 << 2) | ((b3 & 0x0F) << 10));
                Some((w, h))
            } else {
                None
            }
        }
        b"VP8X" => {
            if data.len() >= 30 {
                let w = 1 + u32::from_le_bytes([data[24], data[25], data[26], 0]);
                let h = 1 + u32::from_le_bytes([data[27], data[28], data[29], 0]);
                Some((w, h))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn ico_lines(data: &[u8]) -> Vec<String> {
    if data.len() >= 8 {
        let count = u16::from_le_bytes([data[4], data[5]]);
        let w = if data[6] == 0 { 256 } else { data[6] as u32 };
        let h = if data[7] == 0 { 256 } else { data[7] as u32 };
        let mut out = vec![format!(" {} icon(s)", count)];
        if w > 0 && h > 0 {
            out.push(format!(" {} x {} pixels", w, h));
        }
        out
    } else {
        Vec::new()
    }
}

fn is_pcx(data: &[u8]) -> bool {
    data.len() >= 128
        && data[0] == 0x0A
        && matches!(data[2], 0 | 1)
        && matches!(data[3], 1 | 2 | 4 | 8)
}

fn pcx_lines(data: &[u8]) -> Vec<String> {
    if data.len() >= 12 {
        let xmin = u16::from_le_bytes([data[4], data[5]]) as i32;
        let ymin = u16::from_le_bytes([data[6], data[7]]) as i32;
        let xmax = u16::from_le_bytes([data[8], data[9]]) as i32;
        let ymax = u16::from_le_bytes([data[10], data[11]]) as i32;
        let w = (xmax - xmin + 1).max(0) as u32;
        let h = (ymax - ymin + 1).max(0) as u32;
        wh_lines(w, h)
    } else {
        Vec::new()
    }
}

fn is_tga(data: &[u8], ext: &str) -> bool {
    ext == "tga" && data.len() >= 18
}

fn tga_lines(data: &[u8]) -> Vec<String> {
    if data.len() >= 16 {
        let w = u16::from_le_bytes([data[12], data[13]]) as u32;
        let h = u16::from_le_bytes([data[14], data[15]]) as u32;
        wh_lines(w, h)
    } else {
        Vec::new()
    }
}

fn mp4_lines(data: &[u8]) -> Vec<String> {
    if data.len() >= 12 {
        let brand = String::from_utf8_lossy(&data[8..12]).to_string();
        vec![format!(" Brand {}", brand)]
    } else {
        Vec::new()
    }
}

fn flac_lines(data: &[u8]) -> Vec<String> {
    if data.len() < 42 {
        return Vec::new();
    }
    let sr_hi = data[27] as u32;
    let sr_mid = data[28] as u32;
    let sr_lo = data[29] as u32;
    let sample_rate = (sr_hi << 12) | (sr_mid << 4) | (sr_lo >> 4);
    let channels = ((data[29] >> 1) & 0x07) as u32 + 1;
    let bits = (((data[29] & 0x01) as u32) << 4) | ((data[30] as u32) >> 4);
    let bits = bits + 1;
    let mut out = Vec::new();
    if sample_rate > 0 {
        out.push(format!(" {} Hz", sample_rate));
    }
    out.push(format!(" {} channel(s)", channels));
    out.push(format!(" {} bit", bits));
    out
}

fn ogg_lines(data: &[u8]) -> Vec<String> {
    let sample = &data[..data.len().min(4096)];
    if sample.windows(8).any(|w| w == b"OpusHead") {
        vec![" Opus stream".into()]
    } else if sample.windows(6).any(|w| w == b"vorbis") {
        vec![" Vorbis stream".into()]
    } else if sample.windows(5).any(|w| w == b"FLAC") {
        vec![" FLAC-in-Ogg".into()]
    } else {
        Vec::new()
    }
}

fn is_tar(data: &[u8]) -> bool {
    data.len() > 262 && &data[257..262] == b"ustar"
}

fn is_iso9660(data: &[u8]) -> bool {
    data.len() > 0x8006 && &data[0x8001..0x8006] == b"CD001"
}

fn is_tiff(data: &[u8]) -> bool {
    data.starts_with(b"II*\0") || data.starts_with(b"MM\0*")
}

fn jpeg_size(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xD8 || marker == 0xD9 {
            continue;
        }
        let len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) && len >= 7 {
            let h = u16::from_be_bytes(data[i + 3..i + 5].try_into().ok()?) as u32;
            let w = u16::from_be_bytes(data[i + 5..i + 7].try_into().ok()?) as u32;
            return Some((w, h));
        }
        i += len;
    }
    None
}

fn fixed_text(data: &[u8]) -> Option<String> {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let s = String::from_utf8_lossy(&data[..end]).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn tracker_name(data: &[u8], start: usize, end: usize) -> Option<String> {
    if data.len() >= end {
        fixed_text(&data[start..end])
    } else {
        None
    }
}

fn id3v1_title(data: &[u8]) -> Option<String> {
    if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
        fixed_text(&data[data.len() - 125..data.len() - 95])
    } else {
        None
    }
}

fn html_title(data: &[u8]) -> Option<String> {
    let sample = String::from_utf8_lossy(&data[..data.len().min(8192)]);
    let lower = sample.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let end = lower[start + 7..].find("</title>")?;
    let title = sample[start + 7..start + 7 + end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn clean_field(s: &str) -> String {
    s.trim().replace('\n', " ").replace('\r', " ")
}

fn looks_like_html(data: &[u8]) -> bool {
    let sample = String::from_utf8_lossy(&data[..data.len().min(1024)]).to_ascii_lowercase();
    sample.contains("<html") || sample.contains("<body") || sample.contains("<a href")
}

fn seems_text(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(4096)];
    let bad = sample
        .iter()
        .filter(|&&b| b == 0 || (b < 0x09) || (b > 0x0d && b < 0x20))
        .count();
    bad * 100 / sample.len().max(1) < 5
}

fn is_mod(data: &[u8]) -> bool {
    data.len() > 1084
        && matches!(
            data.get(1080..1084),
            Some(b"M.K.")
                | Some(b"M!K!")
                | Some(b"FLT4")
                | Some(b"4CHN")
                | Some(b"6CHN")
                | Some(b"8CHN")
        )
}

fn is_s3m(data: &[u8]) -> bool {
    data.len() > 48 && data.get(44..48) == Some(b"SCRM")
}

fn is_xm(data: &[u8]) -> bool {
    data.starts_with(b"Extended Module: ")
}

fn is_it(data: &[u8]) -> bool {
    data.starts_with(b"IMPM")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn docx_zip_is_detected_as_office_document() {
        let path = std::env::temp_dir().join(format!("kkc-idf-{}.docx", std::process::id()));
        {
            let file = fs::File::create(&path).expect("create docx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("[Content_Types].xml", options)
                .expect("content types");
            zip.write_all(b"<Types></Types>")
                .expect("write content types");
            zip.start_file("word/document.xml", options)
                .expect("document xml");
            zip.write_all(b"<w:document/>").expect("write document");
            zip.finish().expect("finish docx");
        }

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("docx should be detected");
        assert_eq!(
            info.mime_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(info.format, "Microsoft Word document");
        assert_eq!(info.kind, IdfKind::Other);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn plain_zip_stays_zip_archive() {
        let path = std::env::temp_dir().join(format!("kkc-idf-{}.zip", std::process::id()));
        {
            let file = fs::File::create(&path).expect("create zip");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("readme.txt", options).expect("readme");
            zip.write_all(b"hello").expect("write readme");
            zip.finish().expect("finish zip");
        }

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("zip should be detected");
        assert_eq!(info.mime_type, "application/zip");
        assert_eq!(info.format, "ZIP archive");
        assert_eq!(info.kind, IdfKind::Archive);

        let _ = fs::remove_file(path);
    }
}
