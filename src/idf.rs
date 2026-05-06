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
    pub mime_types: Vec<String>,
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
            mime_types: vec!["inode/directory".into()],
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
    // Filename first, no label
    out.push_str(&format!("{}\n", clean_field(&info.detail)));
    out.push('\n');
    if let Some(title) = info.title.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("Title: {}\n", clean_field(title)));
    }
    out.push_str(&format!("Type: {}\n", clean_field(&info.format)));
    match info.mime_types.as_slice() {
        [] => {}
        [mime_type] => out.push_str(&format!("Mime: {}\n", clean_field(mime_type))),
        mime_types => {
            out.push_str("Mime types:\n");
            for mime_type in mime_types {
                out.push_str(&format!("  {}\n", clean_field(mime_type)));
            }
        }
    }
    if let Some(composer) = info.composer.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("Composer: {}\n", clean_field(composer)));
    }
    if !info.extra.is_empty() {
        out.push('\n');
        for line in info.extra.iter().take(12) {
            out.push_str(&format!("{}\n", clean_field(line)));
        }
    }
    out.push('\n');
    if let Ok(modified) = meta.modified() {
        let dt = DateTime::<Local>::from(modified);
        out.push_str(&format!("Date: {}\n", dt.format("%Y-%m-%d  %H:%M")));
    }
    out.push_str(&format!("Size: {} bytes\n", meta.len()));
    out.push_str(&format!("Attr: {}\n", file_attrs(&meta)));
    Some(out)
}

#[cfg(unix)]
fn file_attrs(meta: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let ftype = if meta.is_dir() {
        'd'
    } else if meta.file_type().is_symlink() {
        'l'
    } else {
        '-'
    };
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    let mut s = String::with_capacity(10);
    s.push(ftype);
    for (bit, ch) in &bits {
        s.push(if mode & bit != 0 { *ch } else { '-' });
    }
    s
}

#[cfg(not(unix))]
fn file_attrs(meta: &fs::Metadata) -> String {
    if meta.permissions().readonly() {
        "-r--r--r--".into()
    } else {
        "-rw-rw-rw-".into()
    }
}

fn probe_file(path: &Path) -> Result<Option<IdInfo>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let file_name_lower = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    // Most format magic bytes are in the first few hundred bytes.
    // The deepest probe is ISO9660 at offset 0x8001 (32 769 B), so 64 KB
    // is always sufficient.  Capping the read here avoids stalling the UI
    // when the cursor lands on a large unrecognised file for the first time.
    const MAX_PROBE_BYTES: usize = 64 * 1024;
    const PDF_PROBE_BYTES: usize = 8 * 1024;
    let (data, file_len) = {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let file_len = file
            .metadata()
            .map(|m| m.len() as usize)
            .unwrap_or(MAX_PROBE_BYTES);
        let max_probe = if ext == "pdf" {
            PDF_PROBE_BYTES
        } else {
            MAX_PROBE_BYTES
        };
        let cap = file_len.min(max_probe);
        let mut buf = vec![0u8; cap];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        (buf, file_len)
    };

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
        Some(info_with_mime_types(
            mime_type,
            &["application/zip"],
            path,
            kind,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(info(
            "application/x-7z-compressed",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.len() >= 14 && data.get(7..14) == Some(b"**ACE**") {
        Some(info(
            "application/x-ace-compressed",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.len() >= 2 && data[0] == 0x1a && (1..=11).contains(&data[1]) {
        Some(info(
            "application/x-arc",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"ZOO ") {
        Some(info(
            "application/x-zoo",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"HLSQZ") {
        Some(info(
            "application/x-sqz",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x76, 0xff]) || data.starts_with(&[0xfa, 0xff]) {
        Some(info(
            "application/x-sq",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x1F, 0x8B]) {
        if ext == "vgz" {
            Some(info(
                "audio/x-vgm",
                path,
                IdfKind::Sample,
                None,
                None,
                vec![" VGZ (gzip-compressed VGM)".into()],
            ))
        } else if ext == "tgz" || file_name_lower.ends_with(".tar.gz") {
            Some(info(
                "application/x-compressed-tar",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        } else {
            Some(info(
                "application/gzip",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        }
    } else if data.starts_with(b"BZh") {
        if ext == "tbz" || ext == "tbz2" || file_name_lower.ends_with(".tar.bz2") {
            Some(info(
                "application/x-bzip-compressed-tar",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        } else {
            Some(info(
                "application/x-bzip2",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        }
    } else if data.starts_with(&[0x1f, 0x9d]) {
        if file_name_lower.ends_with(".tar.z") {
            Some(info(
                "application/x-tarz",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        } else {
            Some(info(
                "application/x-unix-compress",
                path,
                IdfKind::Archive,
                None,
                None,
                vec![],
            ))
        }
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
    } else if data.get(2..5) == Some(b"-lh") && data.get(6) == Some(&b'-') {
        Some(info(
            "application/x-lzh-compressed",
            path,
            IdfKind::Archive,
            lha_first_name(&data),
            None,
            lha_lines(&data),
        ))
    } else if data.starts_with(b"UC2\x1a") || data.starts_with(b"UE2") {
        Some(info(
            "application/x-uc2",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"ICE!")
        || data.starts_with(b"Ice!")
        || data.starts_with(b"TMM!")
        || data.starts_with(b"TSM!")
        || data.starts_with(b"SHE!")
    {
        Some(info(
            "application/x-packice",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"Vgm ") {
        Some(info(
            "audio/x-vgm",
            path,
            IdfKind::Sample,
            None,
            None,
            vgm_lines(&data),
        ))
    } else if data.starts_with(b"UF2\n") && data.get(4..8) == Some(&[0x57, 0x51, 0x5D, 0x9E]) {
        Some(info(
            "application/x-uf2",
            path,
            IdfKind::Other,
            None,
            None,
            uf2_lines(&data),
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
            image_info_lines(w, h, &data, ImageExifContainer::Png),
        ))
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        let (w, h) = webp_size(&data).unwrap_or((0, 0));
        Some(info(
            "image/webp",
            path,
            IdfKind::Bitmap,
            None,
            None,
            image_info_lines(w, h, &data, ImageExifContainer::Webp),
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
            image_info_lines(w, h, &data, ImageExifContainer::Jpeg),
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
            image_info_lines(0, 0, &data, ImageExifContainer::Tiff),
        ))
    } else if is_heif(&data, &ext) {
        Some(info(
            "image/heic",
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
    } else if data.starts_with(b"SQLite format 3\x00") {
        Some(info(
            "application/vnd.sqlite3",
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
            xm_lines(&data),
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
    } else if is_amsdos_file(&data, &ext) {
        Some(info(
            "application/x-amstrad-cpc-amsdos",
            path,
            IdfKind::Other,
            None,
            None,
            amsdos_lines(&data),
        ))
    } else if is_amstrad_dsk(&data, &ext) {
        Some(info(
            "application/x-amstrad-cpc-dsk",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if is_commodore_d64(file_len, &ext) {
        Some(info(
            "application/x-c64-d64",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if is_torrent(&data, &ext) {
        Some(info(
            "application/x-bittorrent",
            path,
            IdfKind::Other,
            torrent_name(&data),
            None,
            vec![],
        ))
    } else if is_vcard(&data, &ext) {
        Some(info(
            "text/vcard",
            path,
            IdfKind::Other,
            vcard_name(&data),
            None,
            vec![],
        ))
    } else if is_json(&data, &ext) {
        Some(info(
            "application/json",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if is_svg(&data, &ext) {
        Some(info(
            "image/svg+xml",
            path,
            IdfKind::Bitmap,
            svg_title(&data),
            None,
            svg_lines(&data),
        ))
    } else if is_xml_document(&data, &ext) {
        Some(info(
            xml_mime_type(&ext),
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if is_csv(&data, &ext) {
        Some(info("text/csv", path, IdfKind::Other, None, None, vec![]))
    } else if is_markdown(&data, &ext) {
        Some(info(
            "text/markdown",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if is_email_message(&data, &ext) {
        Some(info(
            "message/rfc822",
            path,
            IdfKind::Other,
            email_subject(&data),
            None,
            vec![],
        ))
    } else if ext == "mbox" {
        Some(info(
            "application/mbox",
            path,
            IdfKind::Other,
            None,
            None,
            vec![],
        ))
    } else if ext == "dmg" {
        Some(info(
            "application/x-apple-diskimage",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if let Some(mime_type) = archive_mime_type_from_extension(path, &ext) {
        Some(info(mime_type, path, IdfKind::Archive, None, None, vec![]))
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
    info_with_mime_types(mime_type, &[], path, kind, title, composer, extra)
}

fn info_with_mime_types(
    mime_type: &str,
    additional_mime_types: &[&str],
    path: &Path,
    kind: IdfKind,
    title: Option<String>,
    composer: Option<String>,
    extra: Vec<String>,
) -> IdInfo {
    let mut mime_types = Vec::with_capacity(additional_mime_types.len() + 1);
    mime_types.push(mime_type.to_string());
    for additional in additional_mime_types {
        if !mime_types.iter().any(|existing| existing == additional) {
            mime_types.push((*additional).to_string());
        }
    }

    IdInfo {
        format: format_from_mime_type(mime_type)
            .unwrap_or("Unknown file")
            .into(),
        mime_types,
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
    let mime_type = fallback_mime_type(path);
    IdInfo {
        format: "Unknown file".into(),
        mime_types: vec![mime_type],
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
        "application/x-ace-compressed" => Some("ACE archive"),
        "application/x-arc" => Some("ARC/PAK archive"),
        "application/x-zoo" => Some("ZOO archive"),
        "application/x-sq" => Some("SQ/SQ2 squeezed archive"),
        "application/x-sqz" => Some("SQZ archive"),
        "application/gzip" => Some("GZip archive"),
        "application/x-unix-compress" => Some("Unix Z compressed file"),
        "application/x-bzip2" => Some("BZip2 archive"),
        "application/x-xz" => Some("XZ archive"),
        "application/x-arj" => Some("ARJ archive"),
        "application/x-uc2" => Some("UC2 archive"),
        "application/x-packice" => Some("Pack-Ice compressed file"),
        "application/x-ice-compressed" => Some("ICE compressed file"),
        "application/x-ha" => Some("HA archive"),
        "application/x-hyp" => Some("HYP archive"),
        "application/x-compressed-tar" => Some("TGZ archive"),
        "application/x-bzip-compressed-tar" => Some("TBZ archive"),
        "application/x-tarz" => Some("TAR.Z archive"),
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
        "image/heic" => Some("HEIC bitmap"),
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
        "audio/x-vgm" => Some("VGM audio"),
        "application/x-uf2" => Some("UF2 firmware image"),
        "application/x-amstrad-cpc-amsdos" => Some("Amstrad AMSDOS file"),
        "application/x-amstrad-cpc-dsk" => Some("Amstrad CPC DSK image"),
        "application/x-c64-d64" => Some("Commodore 64 D64 disk image"),
        "application/x-bittorrent" => Some("BitTorrent metadata"),
        "application/x-sqlite3" => Some("SQLite database"),
        "application/x-apple-diskimage" => Some("Apple Disk Image"),
        "text/vcard" => Some("vCard contact"),
        "application/json" => Some("JSON document"),
        "image/svg+xml" => Some("SVG vector image"),
        "application/xml" | "text/xml" => Some("XML document"),
        "application/xhtml+xml" => Some("XHTML document"),
        "application/rss+xml" => Some("RSS feed"),
        "application/atom+xml" => Some("Atom feed"),
        "application/x-plist" => Some("Property list"),
        "text/csv" => Some("CSV table"),
        "text/markdown" => Some("Markdown document"),
        "message/rfc822" => Some("EML message"),
        "application/mbox" => Some("Mbox mailbox"),
        "text/html" => Some("HTML document"),
        "text/plain" => Some("Text file"),
        _ => None,
    }
}

fn archive_mime_type_from_extension(path: &Path, ext: &str) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".tar.gz") || ext == "tgz" {
        return Some("application/x-compressed-tar");
    }
    if name.ends_with(".tar.bz2") || ext == "tbz" || ext == "tbz2" {
        return Some("application/x-bzip-compressed-tar");
    }
    if name.ends_with(".tar.z") {
        return Some("application/x-tarz");
    }

    match ext {
        "ace" => Some("application/x-ace-compressed"),
        "arc" | "pak" => Some("application/x-arc"),
        "zoo" => Some("application/x-zoo"),
        "sq" | "sq2" | "qqq" => Some("application/x-sq"),
        "sqz" => Some("application/x-sqz"),
        "z" => Some("application/x-unix-compress"),
        "hyp" => Some("application/x-hyp"),
        "ha" => Some("application/x-ha"),
        "uc2" | "ue2" => Some("application/x-uc2"),
        "ice" => Some("application/x-ice-compressed"),
        "pi9" => Some("application/x-packice"),
        _ => None,
    }
}

fn zip_mime_type(data: &[u8], ext: &str) -> Option<&'static str> {
    if let Ok(mut archive) = ZipArchive::new(Cursor::new(data)) {
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
    }

    zip_mime_type_from_ext(ext)
}

fn zip_mime_type_from_ext(ext: &str) -> Option<&'static str> {
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
        "epub" => Some("application/epub+zip"),
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

fn png_info_lines(w: u32, h: u32, data: &[u8]) -> Vec<String> {
    let mut lines = wh_lines(w, h);
    // PNG IHDR: signature(8) + length(4) + "IHDR"(4) + width(4) + height(4) + bit_depth(1) + color_type(1)
    if data.len() >= 26 {
        let bit_depth = data[24];
        let color_type = data[25];
        let color_desc = match color_type {
            0 => "Grayscale",
            2 => "RGB",
            3 => "Indexed",
            4 => "Gray+Alpha",
            6 => "RGBA",
            _ => "Unknown",
        };
        lines.push(format!(" {}-bit {}", bit_depth, color_desc));
    }
    lines
}

fn wh_lines(w: u32, h: u32) -> Vec<String> {
    if w > 0 && h > 0 {
        vec![format!(" {} x {} pixels", w, h)]
    } else {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy)]
enum ImageExifContainer {
    Jpeg,
    Png,
    Tiff,
    Webp,
}

fn image_info_lines(w: u32, h: u32, data: &[u8], container: ImageExifContainer) -> Vec<String> {
    let mut lines = match container {
        ImageExifContainer::Png => png_info_lines(w, h, data),
        _ => wh_lines(w, h),
    };
    if matches!(container, ImageExifContainer::Jpeg) {
        lines.extend(jpeg_info_lines(data));
    }
    if matches!(container, ImageExifContainer::Tiff) {
        lines.extend(tiff_header_lines(data));
    }
    if let Some(exif) = image_exif_data(data, container) {
        lines.extend(exif_lines(exif));
    }
    lines
}

fn image_exif_data(data: &[u8], container: ImageExifContainer) -> Option<&[u8]> {
    match container {
        ImageExifContainer::Jpeg => jpeg_exif_data(data),
        ImageExifContainer::Png => png_exif_data(data),
        ImageExifContainer::Tiff => Some(data),
        ImageExifContainer::Webp => webp_exif_data(data),
    }
}

fn jpeg_exif_data(data: &[u8]) -> Option<&[u8]> {
    for segment in jpeg_segments(data) {
        if segment.marker == 0xe1 && segment.payload.starts_with(b"Exif\0\0") {
            return Some(&segment.payload[6..]);
        }
    }
    None
}

fn jpeg_comment_lines(data: &[u8]) -> Vec<String> {
    let Some(comment) = jpeg_comment(data) else {
        return Vec::new();
    };
    let comment = comment.trim();
    if comment.is_empty() {
        return Vec::new();
    }
    if let Some((label, value)) = jpeg_key_value_comment(comment) {
        return vec![format!(" {}: {}", label, value)];
    }
    vec![format!(" Comment: {}", comment)]
}

fn jpeg_info_lines(data: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(jfif) = jpeg_jfif_info(data) {
        lines.extend(jfif.lines());
    }
    lines.extend(jpeg_photoshop_lines(data));
    lines.extend(jpeg_iptc_lines(data));
    lines.extend(jpeg_tiff_header_lines(data));
    lines.extend(jpeg_xmp_lines(data));
    if let Some(frame) = jpeg_frame_info(data) {
        lines.extend(frame.lines());
    }
    lines.extend(jpeg_comment_lines(data));
    lines
}

fn jpeg_photoshop_lines(data: &[u8]) -> Vec<String> {
    let Some(header) = jpeg_photoshop_header(data) else {
        return Vec::new();
    };
    vec![format!(" {}", header)]
}

fn jpeg_photoshop_header(data: &[u8]) -> Option<String> {
    for segment in jpeg_segments(data) {
        if segment.marker == 0xed
            && let Some(header) = photoshop_header_from_app13(segment.payload)
        {
            return Some(header);
        }
    }
    None
}

fn jpeg_iptc_lines(data: &[u8]) -> Vec<String> {
    for segment in jpeg_segments(data) {
        if segment.marker == 0xed
            && let Some(iptc) = iptc_block_from_app13(segment.payload)
        {
            let mut lines = Vec::new();
            let version = iptc_iim_version(iptc)
                .map(|v| format!(" v{}", v))
                .unwrap_or_default();
            lines.push(format!(" IPTC: IIM{} ({} bytes)", version, iptc.len()));
            lines.extend(iptc_field_lines(iptc));
            return lines;
        }
    }
    Vec::new()
}

fn iptc_block_from_app13(payload: &[u8]) -> Option<&[u8]> {
    let mut i = if payload.starts_with(b"Photoshop 3.0\0") {
        b"Photoshop 3.0\0".len()
    } else {
        0
    };

    while i + 12 <= payload.len() {
        if payload.get(i..i + 4) != Some(b"8BIM") {
            i += 1;
            continue;
        }
        let resource_id = u16::from_be_bytes([payload[i + 4], payload[i + 5]]);
        let name_len = payload[i + 6] as usize;
        let name_padded = if (1 + name_len) % 2 == 0 {
            1 + name_len
        } else {
            1 + name_len + 1
        };
        let size_offset = i + 6 + name_padded;
        if size_offset + 4 > payload.len() {
            break;
        }
        let data_len = u32::from_be_bytes([
            payload[size_offset],
            payload[size_offset + 1],
            payload[size_offset + 2],
            payload[size_offset + 3],
        ]) as usize;
        let data_start = size_offset + 4;
        let data_end = data_start.checked_add(data_len)?;
        if data_end > payload.len() {
            break;
        }

        if resource_id == 0x0404 {
            return Some(&payload[data_start..data_end]);
        }

        i = data_end + (data_len % 2);
    }

    None
}

fn iptc_field_lines(data: &[u8]) -> Vec<String> {
    let mut object_name = None;
    let mut byline = None;
    let mut caption = None;
    let mut date_created = None;
    let mut copyright = None;
    let mut keywords = Vec::new();

    let mut i = 0usize;
    while i + 5 <= data.len() {
        if data[i] != 0x1c {
            i += 1;
            continue;
        }
        let record = data[i + 1];
        let dataset = data[i + 2];
        let len = u16::from_be_bytes([data[i + 3], data[i + 4]]) as usize;
        let value_start = i + 5;
        let Some(value_end) = value_start.checked_add(len) else {
            break;
        };
        if value_end > data.len() {
            break;
        }

        if record == 0x02 {
            let value = String::from_utf8_lossy(&data[value_start..value_end])
                .trim()
                .to_string();
            if !value.is_empty() {
                match dataset {
                    0x05 if object_name.is_none() => object_name = Some(value),
                    0x50 if byline.is_none() => byline = Some(value),
                    0x19 => keywords.push(value),
                    0x78 if caption.is_none() => caption = Some(value),
                    0x37 if date_created.is_none() => date_created = Some(value),
                    0x74 if copyright.is_none() => copyright = Some(value),
                    _ => {}
                }
            }
        }

        i = value_end;
    }

    let mut lines = Vec::new();
    if let Some(value) = object_name {
        lines.push(format!(" IPTC Object: {}", value));
    }
    if let Some(value) = byline {
        lines.push(format!(" IPTC Byline: {}", value));
    }
    if !keywords.is_empty() {
        lines.push(format!(" IPTC Keywords: {}", keywords.join(", ")));
    }
    if let Some(value) = caption {
        lines.push(format!(" IPTC Caption: {}", value));
    }
    if let Some(value) = date_created {
        lines.push(format!(" IPTC Date: {}", value));
    }
    if let Some(value) = copyright {
        lines.push(format!(" IPTC Copyright: {}", value));
    }
    lines
}

fn iptc_iim_version(data: &[u8]) -> Option<u16> {
    let mut i = 0usize;
    while i + 5 <= data.len() {
        if data[i] != 0x1c {
            i += 1;
            continue;
        }
        let record = data[i + 1];
        let dataset = data[i + 2];
        let len = u16::from_be_bytes([data[i + 3], data[i + 4]]) as usize;
        let value_start = i + 5;
        let value_end = value_start.checked_add(len)?;
        if value_end > data.len() {
            break;
        }
        if record == 0x02 && dataset == 0x00 && len == 2 {
            return Some(u16::from_be_bytes([
                data[value_start],
                data[value_start + 1],
            ]));
        }
        i = value_end;
    }
    None
}

fn jpeg_tiff_header_lines(data: &[u8]) -> Vec<String> {
    jpeg_exif_data(data)
        .map(tiff_header_lines)
        .unwrap_or_default()
}

fn tiff_header_lines(data: &[u8]) -> Vec<String> {
    let Some(line) = tiff_header_line(data) else {
        return Vec::new();
    };
    vec![line]
}

fn tiff_header_line(data: &[u8]) -> Option<String> {
    let (endian_label, little) = match data.get(..2)? {
        b"II" => ("Little-endian", true),
        b"MM" => ("Big-endian", false),
        _ => return None,
    };
    let magic = if little {
        u16::from_le_bytes([data[2], data[3]])
    } else {
        u16::from_be_bytes([data[2], data[3]])
    };
    if magic == 42 {
        let ifd0 = if little {
            u32::from_le_bytes([data[4], data[5], data[6], data[7]])
        } else {
            u32::from_be_bytes([data[4], data[5], data[6], data[7]])
        };
        return Some(format!(
            " TIFF header: {}, v42, IFD0 @ {}",
            endian_label, ifd0
        ));
    }
    if magic == 43 && data.len() >= 16 {
        let ifd0 = if little {
            u64::from_le_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ])
        } else {
            u64::from_be_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ])
        };
        return Some(format!(
            " TIFF header: {}, BigTIFF, IFD0 @ {}",
            endian_label, ifd0
        ));
    }
    None
}

fn photoshop_header_from_app13(payload: &[u8]) -> Option<String> {
    let header = payload.strip_prefix(b"Photoshop ")?;
    let end = header.iter().position(|&b| b == 0).unwrap_or(header.len());
    let version_text = String::from_utf8_lossy(&header[..end]).to_string();
    let version = version_text.trim();
    if version.is_empty() {
        None
    } else {
        Some(format!("Photoshop {}", version))
    }
}

fn jpeg_comment(data: &[u8]) -> Option<String> {
    for segment in jpeg_segments(data) {
        if segment.marker == 0xfe {
            let comment = String::from_utf8_lossy(segment.payload)
                .trim_matches(char::from(0))
                .to_string();
            if !comment.trim().is_empty() {
                return Some(comment);
            }
        }
    }
    None
}

fn jpeg_xmp_lines(data: &[u8]) -> Vec<String> {
    let Some(packet) = jpeg_xmp_packet(data) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(packet);
    let mut lines = Vec::new();
    if let Some(toolkit) = xml_attr_value(&text, "x:xmptk") {
        lines.push(format!(" XMP Toolkit: {}", toolkit));
    }
    if let Some(software) = xml_attr_value(&text, "stEvt:softwareAgent")
        .or_else(|| xml_attr_value(&text, "xmp:CreatorTool"))
    {
        lines.push(format!(" Software: {}", software));
    }
    lines
}

fn jpeg_xmp_packet(data: &[u8]) -> Option<&[u8]> {
    const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    for segment in jpeg_segments(data) {
        if segment.marker == 0xe1 && segment.payload.starts_with(XMP_HEADER) {
            return Some(&segment.payload[XMP_HEADER.len()..]);
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct JpegSegment<'a> {
    marker: u8,
    payload: &'a [u8],
}

fn jpeg_segments(data: &[u8]) -> Vec<JpegSegment<'_>> {
    let mut segments = Vec::new();
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xd8 || marker == 0x01 {
            continue;
        }
        if marker == 0xd9 || marker == 0xda || i + 2 > data.len() {
            break;
        }
        let Some(len_bytes) = data.get(i..i + 2) else {
            break;
        };
        let len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        segments.push(JpegSegment {
            marker,
            payload: &data[i + 2..i + len],
        });
        i += len;
    }
    segments
}

fn jpeg_key_value_comment(comment: &str) -> Option<(String, String)> {
    let (label, value) = comment.split_once(':')?;
    let label = label.trim();
    let value = value.trim();
    if label.is_empty() || value.is_empty() {
        return None;
    }
    let mut chars = label.chars();
    let first = chars.next()?;
    let normalized =
        first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase();
    Some((normalized, value.to_string()))
}

fn xml_attr_value(text: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
struct JpegJfifInfo {
    major: u8,
    minor: u8,
    unit: u8,
    x_density: u16,
    y_density: u16,
}

impl JpegJfifInfo {
    fn lines(self) -> Vec<String> {
        let mut lines = vec![format!(" JFIF: {}.{:02}", self.major, self.minor)];
        if self.x_density > 0 && self.y_density > 0 {
            let unit = match self.unit {
                1 => "dpi",
                2 => "dpcm",
                _ => "units",
            };
            lines.push(format!(
                " Resolution: {} x {} {}",
                self.x_density, self.y_density, unit
            ));
        }
        lines
    }
}

fn jpeg_jfif_info(data: &[u8]) -> Option<JpegJfifInfo> {
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xd8 || marker == 0x01 {
            continue;
        }
        if marker == 0xd9 || marker == 0xda || i + 2 > data.len() {
            break;
        }
        let len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        let payload = &data[i + 2..i + len];
        if marker == 0xe0 && payload.starts_with(b"JFIF\0") && payload.len() >= 12 {
            return Some(JpegJfifInfo {
                major: payload[5],
                minor: payload[6],
                unit: payload[7],
                x_density: u16::from_be_bytes([payload[8], payload[9]]),
                y_density: u16::from_be_bytes([payload[10], payload[11]]),
            });
        }
        i += len;
    }
    None
}

#[derive(Debug, Clone)]
struct JpegFrameInfo {
    process: &'static str,
    bits_per_sample: u8,
    component_count: u8,
    subsampling: Option<String>,
}

impl JpegFrameInfo {
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(" Encoding: {}", self.process)];
        lines.push(format!(" Precision: {} bit", self.bits_per_sample));
        lines.push(format!(" Components: {}", self.component_count));
        if let Some(subsampling) = self.subsampling.as_deref() {
            lines.push(format!(" Subsampling: {}", subsampling));
        }
        lines
    }
}

fn jpeg_frame_info(data: &[u8]) -> Option<JpegFrameInfo> {
    let mut i = 2usize;
    while i + 9 < data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xd8 || marker == 0x01 {
            continue;
        }
        if marker == 0xd9 || marker == 0xda || i + 2 > data.len() {
            break;
        }
        let len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        let payload = &data[i + 2..i + len];
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
            && payload.len() >= 6
        {
            let component_count = payload[5];
            return Some(JpegFrameInfo {
                process: jpeg_process_name(marker),
                bits_per_sample: payload[0],
                component_count,
                subsampling: jpeg_subsampling(payload),
            });
        }
        i += len;
    }
    None
}

fn jpeg_process_name(marker: u8) -> &'static str {
    match marker {
        0xC0 => "Baseline DCT",
        0xC1 => "Extended sequential DCT",
        0xC2 => "Progressive DCT",
        0xC3 => "Lossless sequential",
        0xC5 => "Differential sequential DCT",
        0xC6 => "Differential progressive DCT",
        0xC7 => "Differential lossless",
        0xC9 => "Extended sequential DCT (arithmetic)",
        0xCA => "Progressive DCT (arithmetic)",
        0xCB => "Lossless sequential (arithmetic)",
        0xCD => "Differential sequential DCT (arithmetic)",
        0xCE => "Differential progressive DCT (arithmetic)",
        0xCF => "Differential lossless (arithmetic)",
        _ => "JPEG",
    }
}

fn jpeg_subsampling(payload: &[u8]) -> Option<String> {
    let component_count = payload.get(5).copied()? as usize;
    if component_count < 3 || payload.len() < 6 + component_count * 3 {
        return None;
    }
    let y_sampling = payload.get(7).copied()?;
    let cb_sampling = payload.get(10).copied()?;
    let cr_sampling = payload.get(13).copied()?;
    if cb_sampling != cr_sampling {
        return None;
    }
    let y_h = y_sampling >> 4;
    let y_v = y_sampling & 0x0f;
    let c_h = cb_sampling >> 4;
    let c_v = cb_sampling & 0x0f;
    if y_h == 0 || y_v == 0 || c_h == 0 || c_v == 0 {
        return None;
    }
    let h_ratio = (y_h / c_h).max(1);
    let v_ratio = (y_v / c_v).max(1);
    let label = match (h_ratio, v_ratio) {
        (1, 1) => "4:4:4".to_string(),
        (2, 1) => "4:2:2".to_string(),
        (2, 2) => "4:2:0".to_string(),
        (4, 1) => "4:1:1".to_string(),
        (4, 2) => "4:1:0".to_string(),
        _ => format!("{}x{} / {}x{}", y_h, y_v, c_h, c_v),
    };
    Some(label)
}

fn png_exif_data(data: &[u8]) -> Option<&[u8]> {
    if !data.starts_with(b"\x89PNG\r\n\x1A\n") {
        return None;
    }
    let mut i = 8usize;
    while i + 12 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().ok()?) as usize;
        let kind = data.get(i + 4..i + 8)?;
        let payload_start = i + 8;
        let payload_end = payload_start.checked_add(len)?;
        if payload_end + 4 > data.len() {
            break;
        }
        if kind == b"eXIf" {
            return Some(&data[payload_start..payload_end]);
        }
        i = payload_end + 4;
    }
    None
}

fn webp_exif_data(data: &[u8]) -> Option<&[u8]> {
    if !data.starts_with(b"RIFF") || data.get(8..12) != Some(b"WEBP") {
        return None;
    }
    let mut i = 12usize;
    while i + 8 <= data.len() {
        let kind = data.get(i..i + 4)?;
        let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
        let payload_start = i + 8;
        let payload_end = payload_start.checked_add(len)?;
        if payload_end > data.len() {
            break;
        }
        if kind == b"EXIF" {
            return Some(&data[payload_start..payload_end]);
        }
        i = payload_end + (len & 1);
    }
    None
}

#[derive(Debug, Default)]
struct ExifSummary {
    make: Option<String>,
    model: Option<String>,
    lens: Option<String>,
    taken: Option<String>,
    orientation: Option<u16>,
    exposure: Option<(u32, u32)>,
    aperture: Option<(u32, u32)>,
    iso: Option<u32>,
    focal_length: Option<(u32, u32)>,
    focal_35mm: Option<u32>,
    gps_lat: Option<f64>,
    gps_lon: Option<f64>,
    gps_lat_ref: Option<String>,
    gps_lon_ref: Option<String>,
}

fn exif_lines(data: &[u8]) -> Vec<String> {
    let Some(summary) = parse_exif_summary(data) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(camera) = exif_camera_line(&summary) {
        out.push(format!(" Camera: {}", camera));
    }
    if let Some(lens) = summary.lens.as_deref() {
        out.push(format!(" Lens: {}", lens));
    }
    if let Some(taken) = summary.taken.as_deref() {
        out.push(format!(" Taken: {}", taken));
    }
    if let Some((num, den)) = summary.exposure.filter(|(_, den)| *den != 0) {
        out.push(format!(" Exposure: {}", format_exposure(num, den)));
    }
    if let Some((num, den)) = summary.aperture.filter(|(_, den)| *den != 0) {
        out.push(format!(" Aperture: f/{:.1}", num as f64 / den as f64));
    }
    if let Some(iso) = summary.iso.filter(|iso| *iso > 0) {
        out.push(format!(" ISO: {}", iso));
    }
    if let Some(focal) = exif_focal_line(&summary) {
        out.push(format!(" Focal length: {}", focal));
    }
    if let Some(orientation) = summary.orientation.and_then(orientation_label) {
        out.push(format!(" Orientation: {}", orientation));
    }
    if let Some(gps) = exif_gps_line(&summary) {
        out.push(format!(" GPS: {}", gps));
    }
    out
}

fn exif_camera_line(summary: &ExifSummary) -> Option<String> {
    match (summary.make.as_deref(), summary.model.as_deref()) {
        (Some(make), Some(model)) if model.to_lowercase().contains(&make.to_lowercase()) => {
            Some(model.to_string())
        }
        (Some(make), Some(model)) => Some(format!("{} {}", make, model)),
        (Some(make), None) => Some(make.to_string()),
        (None, Some(model)) => Some(model.to_string()),
        (None, None) => None,
    }
}

fn exif_focal_line(summary: &ExifSummary) -> Option<String> {
    let (num, den) = summary.focal_length.filter(|(_, den)| *den != 0)?;
    let mut value = format!("{:.1} mm", num as f64 / den as f64);
    if let Some(eq) = summary.focal_35mm.filter(|eq| *eq > 0) {
        value.push_str(&format!(" ({} mm eq.)", eq));
    }
    Some(value)
}

fn exif_gps_line(summary: &ExifSummary) -> Option<String> {
    let mut lat = summary.gps_lat?;
    let mut lon = summary.gps_lon?;
    if summary.gps_lat_ref.as_deref() == Some("S") {
        lat = -lat;
    }
    if summary.gps_lon_ref.as_deref() == Some("W") {
        lon = -lon;
    }
    Some(format!("{:.6}, {:.6}", lat, lon))
}

fn format_exposure(num: u32, den: u32) -> String {
    if num == 1 && den > 0 {
        format!("1/{} s", den)
    } else {
        let seconds = num as f64 / den as f64;
        if seconds < 1.0 && seconds > 0.0 {
            format!("1/{:.0} s", 1.0 / seconds)
        } else {
            format!("{:.1} s", seconds)
        }
    }
}

fn orientation_label(value: u16) -> Option<&'static str> {
    match value {
        1 => Some("Normal"),
        2 => Some("Mirrored horizontal"),
        3 => Some("Rotated 180"),
        4 => Some("Mirrored vertical"),
        5 => Some("Mirrored horizontal, rotated 270"),
        6 => Some("Rotated 90 CW"),
        7 => Some("Mirrored horizontal, rotated 90"),
        8 => Some("Rotated 270 CW"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum ExifEndian {
    Little,
    Big,
}

fn parse_exif_summary(data: &[u8]) -> Option<ExifSummary> {
    let endian = match data.get(..2)? {
        b"II" => ExifEndian::Little,
        b"MM" => ExifEndian::Big,
        _ => return None,
    };
    if read_u16(data, 2, endian)? != 42 {
        return None;
    }
    let ifd0 = read_u32(data, 4, endian)? as usize;
    let mut summary = ExifSummary::default();
    let (exif_ifd, gps_ifd) = parse_ifd0(data, ifd0, endian, &mut summary);
    if let Some(offset) = exif_ifd {
        parse_exif_ifd(data, offset, endian, &mut summary);
    }
    if let Some(offset) = gps_ifd {
        parse_gps_ifd(data, offset, endian, &mut summary);
    }
    has_exif_summary(&summary).then_some(summary)
}

fn has_exif_summary(summary: &ExifSummary) -> bool {
    summary.make.is_some()
        || summary.model.is_some()
        || summary.lens.is_some()
        || summary.taken.is_some()
        || summary.orientation.is_some()
        || summary.exposure.is_some()
        || summary.aperture.is_some()
        || summary.iso.is_some()
        || summary.focal_length.is_some()
        || summary.gps_lat.is_some()
        || summary.gps_lon.is_some()
}

fn parse_ifd0(
    data: &[u8],
    offset: usize,
    endian: ExifEndian,
    summary: &mut ExifSummary,
) -> (Option<usize>, Option<usize>) {
    let mut exif_ifd = None;
    let mut gps_ifd = None;
    for entry in ifd_entries(data, offset, endian) {
        match entry.tag {
            0x010f => summary.make = entry_ascii(data, entry, endian),
            0x0110 => summary.model = entry_ascii(data, entry, endian),
            0x0112 => summary.orientation = entry_u16(data, entry, endian),
            0x0132 if summary.taken.is_none() => {
                summary.taken = entry_ascii(data, entry, endian);
            }
            0x8769 => exif_ifd = entry_u32(data, entry, endian).map(|v| v as usize),
            0x8825 => gps_ifd = entry_u32(data, entry, endian).map(|v| v as usize),
            _ => {}
        }
    }
    (exif_ifd, gps_ifd)
}

fn parse_exif_ifd(data: &[u8], offset: usize, endian: ExifEndian, summary: &mut ExifSummary) {
    for entry in ifd_entries(data, offset, endian) {
        match entry.tag {
            0x829a => summary.exposure = entry_rational(data, entry, endian),
            0x829d => summary.aperture = entry_rational(data, entry, endian),
            0x8827 => summary.iso = entry_u32(data, entry, endian),
            0x9003 => {
                if let Some(taken) = entry_ascii(data, entry, endian) {
                    summary.taken = Some(taken);
                }
            }
            0x920a => summary.focal_length = entry_rational(data, entry, endian),
            0xa405 => summary.focal_35mm = entry_u32(data, entry, endian),
            0xa434 => summary.lens = entry_ascii(data, entry, endian),
            _ => {}
        }
    }
}

fn parse_gps_ifd(data: &[u8], offset: usize, endian: ExifEndian, summary: &mut ExifSummary) {
    for entry in ifd_entries(data, offset, endian) {
        match entry.tag {
            0x0001 => summary.gps_lat_ref = entry_ascii(data, entry, endian),
            0x0002 => summary.gps_lat = entry_gps_coord(data, entry, endian),
            0x0003 => summary.gps_lon_ref = entry_ascii(data, entry, endian),
            0x0004 => summary.gps_lon = entry_gps_coord(data, entry, endian),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IfdEntry {
    tag: u16,
    ty: u16,
    count: u32,
    value_offset: usize,
}

fn ifd_entries(data: &[u8], offset: usize, endian: ExifEndian) -> Vec<IfdEntry> {
    let Some(count) = read_u16(data, offset, endian).map(|v| v as usize) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for idx in 0..count.min(128) {
        let entry_offset = offset + 2 + idx * 12;
        if entry_offset + 12 > data.len() {
            break;
        }
        let Some(tag) = read_u16(data, entry_offset, endian) else {
            continue;
        };
        let Some(ty) = read_u16(data, entry_offset + 2, endian) else {
            continue;
        };
        let Some(count) = read_u32(data, entry_offset + 4, endian) else {
            continue;
        };
        entries.push(IfdEntry {
            tag,
            ty,
            count,
            value_offset: entry_offset + 8,
        });
    }
    entries
}

fn entry_bytes<'a>(data: &'a [u8], entry: IfdEntry, endian: ExifEndian) -> Option<&'a [u8]> {
    let unit = match entry.ty {
        1 | 2 | 7 => 1usize,
        3 => 2,
        4 | 9 => 4,
        5 | 10 => 8,
        _ => return None,
    };
    let len = unit.checked_mul(entry.count as usize)?;
    if len <= 4 {
        data.get(entry.value_offset..entry.value_offset + len)
    } else {
        let offset = read_u32(data, entry.value_offset, endian)? as usize;
        data.get(offset..offset.checked_add(len)?)
    }
}

fn entry_ascii(data: &[u8], entry: IfdEntry, endian: ExifEndian) -> Option<String> {
    if entry.ty != 2 {
        return None;
    }
    let bytes = entry_bytes(data, entry, endian)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let value = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn entry_u16(data: &[u8], entry: IfdEntry, endian: ExifEndian) -> Option<u16> {
    match entry.ty {
        3 => {
            let bytes = entry_bytes(data, entry, endian)?;
            read_u16(bytes, 0, endian)
        }
        4 => entry_u32(data, entry, endian).and_then(|v| u16::try_from(v).ok()),
        _ => None,
    }
}

fn entry_u32(data: &[u8], entry: IfdEntry, endian: ExifEndian) -> Option<u32> {
    match entry.ty {
        3 => entry_u16(data, entry, endian).map(u32::from),
        4 => {
            let bytes = entry_bytes(data, entry, endian)?;
            read_u32(bytes, 0, endian)
        }
        _ => None,
    }
}

fn entry_rational(data: &[u8], entry: IfdEntry, endian: ExifEndian) -> Option<(u32, u32)> {
    if entry.ty != 5 || entry.count == 0 {
        return None;
    }
    let bytes = entry_bytes(data, entry, endian)?;
    Some((read_u32(bytes, 0, endian)?, read_u32(bytes, 4, endian)?))
}

fn entry_gps_coord(data: &[u8], entry: IfdEntry, endian: ExifEndian) -> Option<f64> {
    if entry.ty != 5 || entry.count < 3 {
        return None;
    }
    let bytes = entry_bytes(data, entry, endian)?;
    let deg = rational_to_f64(read_u32(bytes, 0, endian)?, read_u32(bytes, 4, endian)?)?;
    let min = rational_to_f64(read_u32(bytes, 8, endian)?, read_u32(bytes, 12, endian)?)?;
    let sec = rational_to_f64(read_u32(bytes, 16, endian)?, read_u32(bytes, 20, endian)?)?;
    Some(deg + (min / 60.0) + (sec / 3600.0))
}

fn rational_to_f64(num: u32, den: u32) -> Option<f64> {
    (den != 0).then_some(num as f64 / den as f64)
}

fn read_u16(data: &[u8], offset: usize, endian: ExifEndian) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset + 2)?.try_into().ok()?;
    Some(match endian {
        ExifEndian::Little => u16::from_le_bytes(bytes),
        ExifEndian::Big => u16::from_be_bytes(bytes),
    })
}

fn read_u32(data: &[u8], offset: usize, endian: ExifEndian) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(match endian {
        ExifEndian::Little => u32::from_le_bytes(bytes),
        ExifEndian::Big => u32::from_be_bytes(bytes),
    })
}

fn pdf_lines(data: &[u8]) -> Vec<String> {
    const PDF_IDF_SCAN_BYTES: usize = 8 * 1024;
    let sample = &data[..data.len().min(PDF_IDF_SCAN_BYTES)];
    let text = String::from_utf8_lossy(sample);
    let mut lines = Vec::new();

    if let Some(version) = pdf_version(sample) {
        lines.push(format!(" PDF version {}", version));
    }
    if text.contains("/Linearized") {
        lines.push(" Linearized / fast web view".into());
    }
    if text.contains("/Encrypt") {
        lines.push(" Encrypted document".into());
    }
    if let Some(pages) = pdf_page_count(&text) {
        lines.push(format!(" {} page(s)", pages));
    }
    let object_count = byte_pattern_count(sample, b" obj");
    if object_count > 0 {
        lines.push(format!(" {} object marker(s) in probe", object_count));
    }

    for (key, label) in [
        ("Title", "Title"),
        ("Author", "Author"),
        ("Subject", "Subject"),
        ("Creator", "Creator"),
        ("Producer", "Producer"),
        ("CreationDate", "Created"),
        ("ModDate", "Modified"),
    ] {
        if let Some(value) = pdf_dict_string(&text, key) {
            lines.push(format!(" {}: {}", label, value));
        }
    }

    lines
}

fn pdf_version(data: &[u8]) -> Option<String> {
    let header = std::str::from_utf8(data.get(..data.len().min(16))?).ok()?;
    let rest = header.strip_prefix("%PDF-")?;
    let version = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    (!version.is_empty()).then_some(version)
}

fn pdf_page_count(text: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    for cap in text.match_indices("/Count") {
        let rest = &text[cap.0 + "/Count".len()..];
        let value = rest
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if let Ok(count) = value.parse::<usize>()
            && count > 0
        {
            best = Some(best.map_or(count, |current| current.max(count)));
        }
    }
    best
}

fn byte_pattern_count(data: &[u8], pattern: &[u8]) -> usize {
    if pattern.is_empty() || data.len() < pattern.len() {
        return 0;
    }
    data.windows(pattern.len())
        .filter(|window| *window == pattern)
        .count()
}

fn pdf_dict_string(text: &str, key: &str) -> Option<String> {
    let marker = format!("/{key}");
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find(&marker) {
        let start = search_from + pos + marker.len();
        let rest = text[start..].trim_start();
        let parsed = if rest.starts_with('(') {
            pdf_literal_string(rest)
        } else if rest.starts_with('<') && !rest.starts_with("<<") {
            pdf_hex_string(rest)
        } else {
            None
        };
        if let Some(value) = parsed.filter(|value| !value.trim().is_empty()) {
            return Some(value);
        }
        search_from = start;
    }
    None
}

fn pdf_literal_string(input: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in input.chars() {
        if depth == 0 {
            if ch == '(' {
                depth = 1;
            }
            continue;
        }
        if escaped {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                '(' | ')' | '\\' => out.push(ch),
                _ => out.push(ch),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '(' => {
                depth += 1;
                out.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(clean_pdf_text(&out));
                }
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    None
}

fn pdf_hex_string(input: &str) -> Option<String> {
    let end = input.get(1..)?.find('>')? + 1;
    let hex = input[1..end]
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>();
    if hex.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    let mut chars = hex.chars();
    while let Some(hi) = chars.next() {
        let lo = chars.next().unwrap_or('0');
        let pair = [hi, lo].iter().collect::<String>();
        bytes.push(u8::from_str_radix(&pair, 16).ok()?);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let mut out = String::new();
        for pair in bytes[2..].chunks(2) {
            if pair.len() == 2 {
                if let Some(ch) = char::from_u32(u16::from_be_bytes([pair[0], pair[1]]) as u32) {
                    out.push(ch);
                }
            }
        }
        Some(clean_pdf_text(&out))
    } else {
        Some(clean_pdf_text(&String::from_utf8_lossy(&bytes)))
    }
}

fn clean_pdf_text(text: &str) -> String {
    text.replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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

fn is_torrent(data: &[u8], ext: &str) -> bool {
    ext == "torrent"
        && data.starts_with(b"d")
        && data.ends_with(b"e")
        && (data
            .windows(b"8:announce".len())
            .any(|w| w == b"8:announce")
            || data.windows(b"4:info".len()).any(|w| w == b"4:info"))
}

fn torrent_name(data: &[u8]) -> Option<String> {
    bencode_string_after_key(data, b"4:name").and_then(|name| String::from_utf8(name).ok())
}

fn bencode_string_after_key(data: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let pos = data.windows(key.len()).position(|w| w == key)? + key.len();
    let mut idx = pos;
    while idx < data.len() && data[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == pos || data.get(idx) != Some(&b':') {
        return None;
    }
    let len = std::str::from_utf8(&data[pos..idx])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let start = idx + 1;
    let end = start.checked_add(len)?;
    (end <= data.len()).then(|| data[start..end].to_vec())
}

fn is_vcard(data: &[u8], ext: &str) -> bool {
    ext == "vcf"
        || String::from_utf8_lossy(&data[..data.len().min(512)])
            .to_ascii_uppercase()
            .contains("BEGIN:VCARD")
}

fn vcard_name(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&data[..data.len().min(4096)]);
    text.lines()
        .find_map(|line| {
            line.strip_prefix("FN:")
                .or_else(|| line.strip_prefix("fn:"))
        })
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn is_json(data: &[u8], ext: &str) -> bool {
    if !matches!(ext, "json" | "geojson") {
        return false;
    }
    let sample = String::from_utf8_lossy(&data[..data.len().min(1024)]);
    let trimmed = sample.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn is_heif(data: &[u8], ext: &str) -> bool {
    matches!(ext, "heic" | "heif") || is_heif_file_type_box(data)
}

fn is_heif_file_type_box(data: &[u8]) -> bool {
    if data.len() < 12 || data.get(4..8) != Some(b"ftyp") {
        return false;
    }

    data[8..].chunks_exact(4).take(16).any(|brand| {
        matches!(
            brand,
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
        )
    })
}

fn is_svg(data: &[u8], ext: &str) -> bool {
    if ext != "svg" {
        return false;
    }
    let sample = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_ascii_lowercase();
    sample.contains("<svg") || sample.contains("<!doctype svg")
}

fn svg_title(data: &[u8]) -> Option<String> {
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

fn is_xml_document(data: &[u8], ext: &str) -> bool {
    if !matches!(
        ext,
        "xml" | "xsd" | "xsl" | "xslt" | "xhtml" | "rss" | "atom" | "plist"
    ) {
        return false;
    }
    let sample = String::from_utf8_lossy(&data[..data.len().min(1024)]).to_ascii_lowercase();
    let trimmed = sample.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with("<?xml") || trimmed.starts_with('<')
}

fn xml_mime_type(ext: &str) -> &'static str {
    match ext {
        "xhtml" => "application/xhtml+xml",
        "rss" => "application/rss+xml",
        "atom" => "application/atom+xml",
        "plist" => "application/x-plist",
        _ => "application/xml",
    }
}

fn is_csv(data: &[u8], ext: &str) -> bool {
    if ext != "csv" {
        return false;
    }
    let sample = String::from_utf8_lossy(&data[..data.len().min(2048)]);
    let first_line = sample.lines().next().unwrap_or_default();
    first_line.contains(',') || first_line.contains(';') || first_line.contains('\t')
}

fn is_markdown(_data: &[u8], ext: &str) -> bool {
    matches!(ext, "md" | "markdown" | "mdown" | "mkd")
}

fn is_email_message(data: &[u8], ext: &str) -> bool {
    if ext == "eml" {
        return true;
    }
    let sample = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_ascii_lowercase();
    sample.contains("\nsubject:") && (sample.contains("\nfrom:") || sample.starts_with("from:"))
}

fn email_subject(data: &[u8]) -> Option<String> {
    let sample = String::from_utf8_lossy(&data[..data.len().min(8192)]);
    sample.lines().find_map(|line| {
        line.strip_prefix("Subject:")
            .or_else(|| line.strip_prefix("subject:"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn is_amstrad_dsk(data: &[u8], ext: &str) -> bool {
    ext == "dsk" && (data.starts_with(b"MV - CPC") || data.starts_with(b"EXTENDED CPC DSK"))
}

fn is_plausible_amsdos_name_byte(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b' ' | b'_' | b'-' | b'.')
}

fn amsdos_checksum_ok(data: &[u8]) -> bool {
    if data.len() < 128 {
        return false;
    }
    let sum = data[..67]
        .iter()
        .fold(0u16, |acc, b| acc.wrapping_add(*b as u16));
    let stored = u16::from_le_bytes([data[67], data[68]]);
    sum == stored
}

fn is_amsdos_file(data: &[u8], ext: &str) -> bool {
    if data.len() < 128 {
        return false;
    }

    if ext == "amsd" {
        return true;
    }

    let user = data[0];
    if user > 31 {
        return false;
    }

    let mut has_non_space = false;
    for &byte in &data[1..12] {
        let b = byte & 0x7f;
        if !is_plausible_amsdos_name_byte(b) {
            return false;
        }
        if b != b' ' {
            has_non_space = true;
        }
    }
    if !has_non_space {
        return false;
    }

    let length = u16::from_le_bytes([data[24], data[25]]) as usize;
    if length == 0 {
        return false;
    }

    let file_type = data[18];
    let content_kind = (file_type >> 1) & 0x07;
    if content_kind > 4 {
        return false;
    }

    if data[12..16].iter().any(|b| *b != 0) {
        return false;
    }

    if amsdos_checksum_ok(data) {
        return true;
    }

    // Some legacy files have an invalid checksum but still carry a valid header.
    // Keep supporting those by relying on structural checks above.

    true
}

fn amsdos_lines(data: &[u8]) -> Vec<String> {
    if data.len() < 128 {
        return Vec::new();
    }
    let mut out = Vec::new();

    let user = data[0];
    let raw_name = &data[1..12];
    let name = String::from_utf8_lossy(&raw_name.iter().map(|b| b & 0x7f).collect::<Vec<u8>>())
        .to_string();
    let base = name[..8].trim_end();
    let ext = name[8..].trim_end();
    let display = if ext.is_empty() {
        base.to_string()
    } else {
        format!("{}.{}", base, ext)
    };
    let file_type = data[18];
    let protected = (file_type & 0x01) != 0;
    let content_kind = (file_type >> 1) & 0x07;
    let version = (file_type >> 4) & 0x0f;
    let kind = match content_kind {
        0 => "BASIC",
        1 => "Binary",
        2 => "Screen",
        3 => "ASCII",
        _ => "Unknown",
    };
    let logical_length = u16::from_le_bytes([data[24], data[25]]) as usize;
    let load_address = u16::from_le_bytes([data[21], data[22]]);
    let entry_address = u16::from_le_bytes([data[26], data[27]]);
    let real_length = (data[64] as u32) | ((data[65] as u32) << 8) | ((data[66] as u32) << 16);
    let checksum = data[..67]
        .iter()
        .fold(0u16, |acc, b| acc.wrapping_add(*b as u16));
    let stored_checksum = u16::from_le_bytes([data[67], data[68]]);

    out.push(format!(" AMSDOS file: {}", display));
    out.push(format!(" User: {}", user));
    out.push(format!(" Type: {} (raw={})", kind, file_type));
    out.push(format!(
        " Protected: {}",
        if protected { "yes" } else { "no" }
    ));
    out.push(format!(" Version: {}", version));
    out.push(format!(" Load address: 0x{:04X}", load_address));
    out.push(format!(" Exec address: 0x{:04X}", entry_address));
    out.push(format!(" Logical length: {} bytes", logical_length));
    out.push(format!(" Real length: {} bytes", real_length));
    out.push(format!(
        " Checksum: {:04X} / stored {:04X} ({})",
        checksum,
        stored_checksum,
        if checksum == stored_checksum {
            "OK"
        } else {
            "mismatch"
        }
    ));

    out
}

fn is_commodore_d64(file_len: usize, ext: &str) -> bool {
    const D64_SIZES: &[usize] = &[174_848, 175_531, 196_608, 197_376, 205_312];
    ext == "d64" && D64_SIZES.contains(&file_len)
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

fn xm_lines(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() < 80 {
        return out;
    }
    let channels = u16::from_le_bytes([data[68], data[69]]);
    let patterns = u16::from_le_bytes([data[70], data[71]]);
    let instruments = u16::from_le_bytes([data[72], data[73]]);
    let bpm = u16::from_le_bytes([data[78], data[79]]);
    out.push(format!(" {} channel(s)", channels));
    out.push(format!(" {} pattern(s)", patterns));
    out.push(format!(" {} instrument(s)", instruments));
    if bpm > 0 {
        out.push(format!(" {} BPM", bpm));
    }
    out
}

fn lha_first_name(data: &[u8]) -> Option<String> {
    // Level-0/1 LZH headers: offset 0 = header size, 2..7 = method,
    // 7..11 = compressed size (LE32), 11..15 = original size (LE32),
    // 19 = filename length, 20.. = filename
    if data.len() < 22 {
        return None;
    }
    let name_len = data[19] as usize;
    if name_len == 0 || 20 + name_len > data.len() {
        return None;
    }
    let name = &data[20..20 + name_len];
    let s = String::from_utf8_lossy(name).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn lha_lines(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() >= 7 {
        // method ID is at offset 2..7, e.g. "-lh5-"
        if let Ok(method) = std::str::from_utf8(&data[2..7]) {
            out.push(format!(" Method {}", method));
        }
    }
    out
}

fn vgm_lines(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() >= 12 {
        let version = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let major = (version >> 8) & 0xF;
        let minor = version & 0xFF;
        out.push(format!(" VGM v{}.{:02x}", major, minor));
    }
    if data.len() >= 40 {
        let gd3_offset = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        if gd3_offset > 0 {
            let gd3_abs = 0x14usize.wrapping_add(gd3_offset as usize);
            if gd3_abs + 12 <= data.len() && &data[gd3_abs..gd3_abs + 4] == b"Gd3 " {
                // GD3 tag present — extract track name (first UTF-16LE string)
                let str_start = gd3_abs + 12;
                if str_start < data.len() {
                    let title = read_gd3_string(&data[str_start..]);
                    if let Some(t) = title.filter(|s| !s.is_empty()) {
                        out.push(format!(" Track: {}", t));
                    }
                }
            }
        }
    }
    out
}

fn read_gd3_string(data: &[u8]) -> Option<String> {
    let words: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&w| w != 0)
        .collect();
    let s = String::from_utf16(&words).ok()?;
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn uf2_lines(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() < 32 {
        return out;
    }
    let flags = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let num_blocks = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    out.push(format!(" {} block(s)", num_blocks));
    // Family ID present when bit 0x00002000 is set
    if flags & 0x0000_2000 != 0 && data.len() >= 484 {
        let family = u32::from_le_bytes([data[480], data[481], data[482], data[483]]);
        out.push(format!(" Family ID 0x{:08X}", family));
    }
    let target_addr = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    out.push(format!(" Target 0x{:08X}", target_addr));
    out
}

fn svg_lines(data: &[u8]) -> Vec<String> {
    let sample = String::from_utf8_lossy(&data[..data.len().min(4096)]);
    let lower = sample.to_ascii_lowercase();
    let svg_start = match lower.find("<svg") {
        Some(p) => p,
        None => return Vec::new(),
    };
    let tag_end = lower[svg_start..]
        .find('>')
        .map(|p| p + svg_start)
        .unwrap_or(sample.len());
    let tag = &sample[svg_start..tag_end];
    let mut out = Vec::new();

    // Try to extract width and height
    if let (Some(w), Some(h)) = (attr_value(tag, "width"), attr_value(tag, "height")) {
        out.push(format!(" {}  x  {}", w.trim(), h.trim()));
    } else if let Some(vb) = attr_value(tag, "viewBox") {
        out.push(format!(" viewBox {}", vb.trim()));
    }
    out
}

fn attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let key = format!("{}=", attr);
    let pos = lower.find(key.as_str())?;
    let rest = &tag[pos + key.len()..];
    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        Some(&rest[1..1 + end])
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'')?;
        Some(&rest[1..1 + end])
    } else {
        None
    }
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
            info.mime_types[0],
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            info.mime_types,
            vec![
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "application/zip"
            ]
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
        assert_eq!(info.mime_types[0], "application/zip");
        assert_eq!(info.mime_types, vec!["application/zip"]);
        assert_eq!(info.format, "ZIP archive");
        assert_eq!(info.kind, IdfKind::Archive);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn epub_zip_keeps_epub_mime_when_probe_is_truncated() {
        let path = std::env::temp_dir().join(format!("kkc-idf-{}.epub", std::process::id()));
        {
            let file = fs::File::create(&path).expect("create epub");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("mimetype", options).expect("mimetype");
            zip.write_all(b"application/epub+zip")
                .expect("write mimetype");
            zip.start_file("META-INF/container.xml", options)
                .expect("container xml");
            zip.write_all(
                br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            )
            .expect("write container");
            zip.start_file("OEBPS/content.opf", options)
                .expect("content opf");
            zip.write_all(br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/></package>"#)
                .expect("write opf");
            zip.start_file("OEBPS/payload.bin", options)
                .expect("payload");
            zip.write_all(&vec![0x5a; 80 * 1024])
                .expect("write payload");
            zip.finish().expect("finish epub");
        }

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("epub should be detected");
        assert_eq!(info.mime_types[0], "application/epub+zip");
        assert_eq!(
            info.mime_types,
            vec!["application/epub+zip", "application/zip"]
        );
        assert_eq!(info.kind, IdfKind::Archive);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn text_formats_are_detected_before_plain_text() {
        let root = std::env::temp_dir().join(format!("kkc-idf-text-{}", std::process::id()));
        fs::create_dir_all(&root).expect("temp dir");

        let cases = [
            (
                "sample.json",
                br#"{"name":"KKC"}"#.as_slice(),
                "application/json",
                "JSON document",
            ),
            (
                "contact.vcf",
                b"BEGIN:VCARD\nFN:Test User\nEND:VCARD\n".as_slice(),
                "text/vcard",
                "vCard contact",
            ),
            (
                "image.svg",
                b"<svg><title>Logo</title></svg>".as_slice(),
                "image/svg+xml",
                "SVG vector image",
            ),
            (
                "download.torrent",
                b"d8:announce13:http://tracker4:infod4:name4:demoee".as_slice(),
                "application/x-bittorrent",
                "BitTorrent metadata",
            ),
        ];

        for (name, data, mime_type, format) in cases {
            let path = root.join(name);
            fs::write(&path, data).expect("write sample");
            let info = probe_file(&path)
                .expect("probe should not fail")
                .expect("sample should be detected");
            assert_eq!(info.mime_types[0], mime_type);
            assert_eq!(info.format, format);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pdf_metadata_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-pdf-{}.pdf", std::process::id()));
        fs::write(
            &path,
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Count 12 >>\nendobj\n3 0 obj\n<< /Title (Demo PDF) /Author (Miguel) /Producer <feff004b004b0043> /CreationDate (D:20260429110442+02'00') >>\nendobj\ntrailer\n<< /Root 1 0 R /Info 3 0 R >>\n",
        )
        .expect("write pdf");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("pdf should be detected");
        assert_eq!(info.mime_types[0], "application/pdf");
        assert_eq!(info.format, "PDF document");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("PDF version 1.7"))
        );
        assert!(info.extra.iter().any(|line| line.contains("12 page(s)")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Title: Demo PDF"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Author: Miguel"))
        );
        assert!(info.extra.iter().any(|line| line.contains("Producer: KKC")));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_exif_metadata_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-exif-{}.jpg", std::process::id()));
        fs::write(&path, jpeg_with_exif()).expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert_eq!(info.mime_types[0], "image/jpeg");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("32 x 16 pixels"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Camera: Canon EOS R5"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Taken: 2026:04:30 12:34:56"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Exposure: 1/125 s"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Aperture: f/2.8"))
        );
        assert!(info.extra.iter().any(|line| line.contains("ISO: 400")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Focal length: 50.0 mm"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("TIFF header: Little-endian, v42, IFD0 @ 8"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_creator_comment_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-comment-{}.jpg", std::process::id()));
        fs::write(
            &path,
            jpeg_with_comment("CREATOR: gd-jpeg v1.0 (using IJG JPEG v80), quality = 75."),
        )
        .expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert_eq!(info.mime_types[0], "image/jpeg");
        assert!(info.extra.iter().any(|line| {
            line.contains("Creator: gd-jpeg v1.0 (using IJG JPEG v80), quality = 75.")
        }));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_jfif_and_frame_metadata_are_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-jfif-{}.jpg", std::process::id()));
        fs::write(
            &path,
            jpeg_with_jfif_comment("CREATOR: gd-jpeg v1.0", 96, 96),
        )
        .expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert!(info.extra.iter().any(|line| line.contains("JFIF: 1.01")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Resolution: 96 x 96 dpi"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Encoding: Baseline DCT"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Precision: 8 bit"))
        );
        assert!(info.extra.iter().any(|line| line.contains("Components: 3")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Subsampling: 4:2:0"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_photoshop_header_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-ps-{}.jpg", std::process::id()));
        fs::write(&path, jpeg_with_photoshop_app13("3.0")).expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert!(info.extra.iter().any(|line| line.contains("Photoshop 3.0")));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_xmp_software_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-xmp-{}.jpg", std::process::id()));
        fs::write(&path, jpeg_with_xmp_software("Affinity Photo 1.10.6")).expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("XMP Toolkit: XMP Core 5.5.0"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Software: Affinity Photo 1.10.6"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_iptc_header_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-iptc-{}.jpg", std::process::id()));
        fs::write(&path, jpeg_with_iptc_app13(4)).expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC: IIM v4 (7 bytes)"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn jpeg_iptc_fields_are_reported() {
        let path =
            std::env::temp_dir().join(format!("kkc-idf-iptc-fields-{}.jpg", std::process::id()));
        fs::write(
            &path,
            jpeg_with_iptc_fields_app13(
                4,
                "State Of The Art",
                "Miguel Van Hove",
                &["fire", "affinity", "jpeg"],
                "Demo caption",
                "20260505",
                "(c) 2026 KKC",
            ),
        )
        .expect("write jpeg");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("jpeg should be detected");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Object: State Of The Art"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Byline: Miguel Van Hove"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Keywords: fire, affinity, jpeg"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Caption: Demo caption"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Date: 20260505"))
        );
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("IPTC Copyright: (c) 2026 KKC"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn tiff_header_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-tiff-{}.tif", std::process::id()));
        fs::write(&path, test_exif_tiff()).expect("write tiff");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("tiff should be detected");
        assert_eq!(info.mime_types[0], "image/tiff");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("TIFF header: Little-endian, v42, IFD0 @ 8"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn heic_file_type_box_is_detected_as_image() {
        let path = std::env::temp_dir().join(format!("kkc-idf-heic-{}.heic", std::process::id()));
        fs::write(
            &path,
            b"\x00\x00\x00\x28ftypheic\x00\x00\x00\x00mif1MiHEMiPrmiaf",
        )
        .expect("write heic");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("heic should be detected");
        assert_eq!(info.mime_types[0], "image/heic");
        assert_eq!(info.format, "HEIC bitmap");
        assert_eq!(info.kind, IdfKind::Bitmap);

        let _ = fs::remove_file(path);
    }

    fn jpeg_with_exif() -> Vec<u8> {
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(test_exif_tiff());
        let app1_len = u16::try_from(app1.len() + 2).expect("APP1 length fits");

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(app1_len.to_be_bytes());
        jpeg.extend(app1);
        jpeg.extend([
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        jpeg
    }

    fn jpeg_with_comment(comment: &str) -> Vec<u8> {
        jpeg_with_jfif_comment(comment, 1, 1)
    }

    fn jpeg_with_jfif_comment(comment: &str, x_density: u16, y_density: u16) -> Vec<u8> {
        let app0 = [
            b'J',
            b'F',
            b'I',
            b'F',
            0x00,
            0x01,
            0x01,
            0x01,
            (x_density >> 8) as u8,
            (x_density & 0xff) as u8,
            (y_density >> 8) as u8,
            (y_density & 0xff) as u8,
            0x00,
            0x00,
        ];
        let app0_len = u16::try_from(app0.len() + 2).expect("APP0 length fits");
        let mut payload = comment.as_bytes().to_vec();
        let len = u16::try_from(payload.len() + 2).expect("COM length fits");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
        jpeg.extend(app0_len.to_be_bytes());
        jpeg.extend(app0);
        jpeg.extend([0xff, 0xfe]);
        jpeg.extend(len.to_be_bytes());
        jpeg.append(&mut payload);
        jpeg.extend([
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        jpeg
    }

    fn jpeg_with_photoshop_app13(version: &str) -> Vec<u8> {
        let mut app13 = b"Photoshop ".to_vec();
        app13.extend(version.as_bytes());
        app13.push(0);
        app13.extend(b"8BIM\x04\x04\0\0\0\0\0\0");
        let app13_len = u16::try_from(app13.len() + 2).expect("APP13 length fits");

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xed];
        jpeg.extend(app13_len.to_be_bytes());
        jpeg.extend(app13);
        jpeg.extend([
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        jpeg
    }

    fn jpeg_with_xmp_software(software: &str) -> Vec<u8> {
        let xmp = format!(
            "http://ns.adobe.com/xap/1.0/\0<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"XMP Core 5.5.0\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:stEvt=\"http://ns.adobe.com/xap/1.0/sType/ResourceEvent#\"><xmpMM:History xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\"><rdf:Seq><rdf:li stEvt:action=\"produced\" stEvt:softwareAgent=\"{software}\"/></rdf:Seq></xmpMM:History></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
        );
        let app1 = xmp.into_bytes();
        let app1_len = u16::try_from(app1.len() + 2).expect("APP1 XMP length fits");

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(app1_len.to_be_bytes());
        jpeg.extend(app1);
        jpeg.extend([
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        jpeg
    }

    fn jpeg_with_iptc_app13(version: u16) -> Vec<u8> {
        jpeg_with_iptc_fields_app13(version, "", "", &[], "", "", "")
    }

    fn jpeg_with_iptc_fields_app13(
        version: u16,
        object: &str,
        byline: &str,
        keywords: &[&str],
        caption: &str,
        date: &str,
        copyright: &str,
    ) -> Vec<u8> {
        let mut iptc = vec![0x1c, 0x02, 0x00, 0x00, 0x02];
        iptc.extend(version.to_be_bytes());
        if !object.is_empty() {
            append_iptc_dataset(&mut iptc, 0x05, object);
        }
        if !byline.is_empty() {
            append_iptc_dataset(&mut iptc, 0x50, byline);
        }
        for keyword in keywords {
            append_iptc_dataset(&mut iptc, 0x19, keyword);
        }
        if !caption.is_empty() {
            append_iptc_dataset(&mut iptc, 0x78, caption);
        }
        if !date.is_empty() {
            append_iptc_dataset(&mut iptc, 0x37, date);
        }
        if !copyright.is_empty() {
            append_iptc_dataset(&mut iptc, 0x74, copyright);
        }

        let mut app13 = b"Photoshop 3.0\0".to_vec();
        app13.extend(b"8BIM");
        app13.extend(0x0404u16.to_be_bytes());
        app13.push(0x00);
        app13.push(0x00);
        app13.extend((iptc.len() as u32).to_be_bytes());
        app13.extend(&iptc);
        if iptc.len() % 2 == 1 {
            app13.push(0x00);
        }

        let app13_len = u16::try_from(app13.len() + 2).expect("APP13 length fits");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xed];
        jpeg.extend(app13_len.to_be_bytes());
        jpeg.extend(app13);
        jpeg.extend([
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        jpeg
    }

    fn append_iptc_dataset(buf: &mut Vec<u8>, dataset: u8, value: &str) {
        let bytes = value.as_bytes();
        let len = u16::try_from(bytes.len()).expect("IPTC field length fits");
        buf.push(0x1c);
        buf.push(0x02);
        buf.push(dataset);
        buf.extend(len.to_be_bytes());
        buf.extend(bytes);
    }

    fn test_exif_tiff() -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        append_ifd_placeholder(&mut tiff, 4);
        let make = append_ascii(&mut tiff, "Canon");
        let model = append_ascii(&mut tiff, "EOS R5");
        let exif_ifd = tiff.len() as u32;
        append_ifd_placeholder(&mut tiff, 6);
        let taken = append_ascii(&mut tiff, "2026:04:30 12:34:56");
        let exposure = append_rational(&mut tiff, 1, 125);
        let aperture = append_rational(&mut tiff, 28, 10);
        let focal = append_rational(&mut tiff, 50, 1);
        let lens = append_ascii(&mut tiff, "RF 50mm");

        write_ifd_entry(&mut tiff, 8, 0, 0x010f, 2, 6, make);
        write_ifd_entry(&mut tiff, 8, 1, 0x0110, 2, 7, model);
        write_ifd_entry(&mut tiff, 8, 2, 0x0112, 3, 1, 1);
        write_ifd_entry(&mut tiff, 8, 3, 0x8769, 4, 1, exif_ifd);

        write_ifd_entry(&mut tiff, exif_ifd as usize, 0, 0x9003, 2, 20, taken);
        write_ifd_entry(&mut tiff, exif_ifd as usize, 1, 0x829a, 5, 1, exposure);
        write_ifd_entry(&mut tiff, exif_ifd as usize, 2, 0x829d, 5, 1, aperture);
        write_ifd_entry(&mut tiff, exif_ifd as usize, 3, 0x8827, 3, 1, 400);
        write_ifd_entry(&mut tiff, exif_ifd as usize, 4, 0x920a, 5, 1, focal);
        write_ifd_entry(&mut tiff, exif_ifd as usize, 5, 0xa434, 2, 8, lens);
        tiff
    }

    fn append_ifd_placeholder(buf: &mut Vec<u8>, entries: u16) {
        buf.extend(entries.to_le_bytes());
        buf.resize(buf.len() + entries as usize * 12 + 4, 0);
    }

    fn append_ascii(buf: &mut Vec<u8>, value: &str) -> u32 {
        let offset = buf.len() as u32;
        buf.extend(value.as_bytes());
        buf.push(0);
        offset
    }

    fn append_rational(buf: &mut Vec<u8>, num: u32, den: u32) -> u32 {
        let offset = buf.len() as u32;
        buf.extend(num.to_le_bytes());
        buf.extend(den.to_le_bytes());
        offset
    }

    fn write_ifd_entry(
        buf: &mut [u8],
        ifd_offset: usize,
        idx: usize,
        tag: u16,
        ty: u16,
        count: u32,
        value: u32,
    ) {
        let offset = ifd_offset + 2 + idx * 12;
        buf[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
        buf[offset + 2..offset + 4].copy_from_slice(&ty.to_le_bytes());
        buf[offset + 4..offset + 8].copy_from_slice(&count.to_le_bytes());
        if ty == 3 && count == 1 {
            buf[offset + 8..offset + 10].copy_from_slice(&(value as u16).to_le_bytes());
        } else {
            buf[offset + 8..offset + 12].copy_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn disk_images_are_detected() {
        let root = std::env::temp_dir().join(format!("kkc-idf-disk-{}", std::process::id()));
        fs::create_dir_all(&root).expect("temp dir");

        let dsk_path = root.join("disk.dsk");
        let mut dsk = b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n".to_vec();
        dsk.resize(256, 0);
        fs::write(&dsk_path, dsk).expect("write dsk");
        let dsk_info = probe_file(&dsk_path)
            .expect("probe dsk should not fail")
            .expect("dsk should be detected");
        assert_eq!(dsk_info.mime_types[0], "application/x-amstrad-cpc-dsk");
        assert_eq!(dsk_info.format, "Amstrad CPC DSK image");

        let d64_path = root.join("disk.d64");
        fs::write(&d64_path, vec![0u8; 174_848]).expect("write d64");
        let d64_info = probe_file(&d64_path)
            .expect("probe d64 should not fail")
            .expect("d64 should be detected");
        assert_eq!(d64_info.mime_types[0], "application/x-c64-d64");
        assert_eq!(d64_info.format, "Commodore 64 D64 disk image");

        let _ = fs::remove_dir_all(root);
    }
}
