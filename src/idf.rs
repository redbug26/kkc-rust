use anyhow::Result;
use chrono::{DateTime, Local};
use delharc::decode::{Decoder, Lh5Decoder};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
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
    let max_probe = if ext == "pdf" {
        PDF_PROBE_BYTES
    } else {
        MAX_PROBE_BYTES
    };
    let probe = crate::file_cache::read_prefix(path, max_probe)?;
    let data = probe.bytes;
    let file_len = probe.file_len as usize;

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
        let ym_from_lzh = parse_ym_info(&data).or_else(|_| {
            if matches!(ext.as_str(), "ym" | "ym5" | "ym6") && data.len() < file_len {
                let full = crate::file_cache::read_file(path, None)?;
                parse_ym_info(&full.bytes)
            } else {
                anyhow::bail!("Not a YM payload")
            }
        });

        match ym_from_lzh {
            Ok(song) => Some(info(
                "audio/x-ym",
                path,
                IdfKind::Module,
                Some(song.song_name.clone()),
                Some(song.song_author.clone()),
                ym_lines(&song),
            )),
            Err(err) if matches!(ext.as_str(), "ym" | "ym5" | "ym6") => {
                let mut lines = vec![" YM file in LZH container (decode failed)".into()];
                lines.push(format!(" Decode error: {err}"));
                lines.extend(lha_lines(&data));
                Some(info(
                    "audio/x-ym",
                    path,
                    IdfKind::Module,
                    lha_first_name(&data),
                    None,
                    lines,
                ))
            }
            Err(_) => Some(info(
                "application/x-lzh-compressed",
                path,
                IdfKind::Archive,
                lha_first_name(&data),
                None,
                lha_lines(&data),
            )),
        }
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
    } else if is_webshots_wbc(&data, file_len) {
        Some(info_with_mime_types(
            "image/x-webshots",
            &["application/x-webshots"],
            path,
            IdfKind::Bitmap,
            webshots_title(&data),
            None,
            webshots_wbc_lines(&data, file_len),
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
    } else if data.starts_with(&[
        0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
    ]) {
        Some(info("image/jp2", path, IdfKind::Bitmap, None, None, vec![]))
    } else if data.starts_with(b"/* XPM */") {
        Some(info(
            "image/x-xpixmap",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"IIN1") {
        Some(info(
            "image/x-niff",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"AT&TFORM") && data.len() > 12 && &data[8..12] == b"DjVu" {
        Some(info(
            "image/vnd.djvu",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"%bitmap\0") {
        Some(info(
            "image/x-fbm",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.len() > 2
        && data[0] == b'P'
        && (1..=7).contains(&(data[1] - b'0' + 1))
        && (data.len() > 2 && matches!(data[2], b' ' | b'\t' | b'\n' | b'\r'))
    {
        let format_name = match data[1] {
            b'1' => "image/x-portable-bitmap",
            b'2' => "image/x-portable-graymap",
            b'3' => "image/x-portable-pixmap",
            b'4' => "image/x-portable-bitmap",
            b'5' => "image/x-portable-graymap",
            b'6' => "image/x-portable-pixmap",
            b'7' => "image/x-portable-pixmap",
            _ => "image/x-portable-pixmap",
        };
        Some(info(format_name, path, IdfKind::Bitmap, None, None, vec![]))
    } else if data.starts_with(&[0x00, 0x01, 0x00, 0x08]) {
        Some(info(
            "image/x-gem",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x59, 0xa6, 0x6a, 0x95]) {
        Some(info(
            "image/x-sun-raster",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0xf1, 0x00, 0x40, 0xbb]) {
        Some(info(
            "image/x-cmu-raster",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.len() > 2 && data[0] == 0x4f && data[1] == b':' {
        Some(info(
            "image/x-solitaire",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"id=ImageMagick") {
        Some(info(
            "image/x-miff",
            path,
            IdfKind::Bitmap,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"wOFF") {
        Some(info("font/woff", path, IdfKind::Other, None, None, vec![]))
    } else if data.starts_with(b"wOF2") {
        Some(info("font/woff2", path, IdfKind::Other, None, None, vec![]))
    } else if data.starts_with(b"SQLite format 3\0") {
        Some(info(
            "application/x-sqlite3",
            path,
            IdfKind::Other,
            None,
            None,
            sqlite_lines(&data),
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
    } else if let Some(mp3) = mp3_info(path, &data, &ext) {
        Some(mp3)
    } else if data.starts_with(b"MMD1") {
        Some(info(
            "audio/x-octamed",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" OctaMED Pro v1".into()],
        ))
    } else if data.starts_with(b"MMD3") {
        Some(info(
            "audio/x-octamed",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" OctaMED Soundstudio v3".into()],
        ))
    } else if data.starts_with(b"OctaMEDCmpr") {
        Some(info(
            "audio/x-octamed-compressed",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" OctaMED Compressed".into()],
        ))
    } else if data.starts_with(b"MThd   ") {
        // Standard MIDI with explicit length check
        Some(midi_info(path, &data))
    } else if data.starts_with(b"MThd") {
        Some(midi_info(path, &data))
    } else if data.starts_with(b"FC14") {
        Some(info(
            "audio/x-fc14",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Future Composer 1.4 Module".into()],
        ))
    } else if data.starts_with(b"SMOD") {
        Some(info(
            "audio/x-smod",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Smod Module".into()],
        ))
    } else if data.starts_with(b"AON4artofnoise") {
        Some(info(
            "audio/x-aon4",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Art Of Noise Module".into()],
        ))
    } else if data.starts_with(b"ARP.") {
        Some(info(
            "audio/x-arp",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" The Holy Noise Module".into()],
        ))
    } else if data.starts_with(b"BeEp\x00") {
        Some(info(
            "audio/x-jamcracker",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" JamCracker Module".into()],
        ))
    } else if data.starts_with(b"COSO\x00") {
        Some(info(
            "audio/x-coso",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Hippel-COSO Module".into()],
        ))
    } else if data.starts_with(b"FTMN") {
        Some(info(
            "audio/x-ftmn",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" FaceTheMusic Module".into()],
        ))
    } else if data.starts_with(b"EMOD") {
        Some(info(
            "audio/x-emod",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Extended MOD Module".into()],
        ))
    } else if data.starts_with(b"CTMF") {
        Some(info(
            "audio/x-ctmf",
            path,
            IdfKind::Module,
            None,
            None,
            vec![" Creative Music Format".into()],
        ))
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
    } else if is_sid(&data) {
        Some(info(
            "audio/x-sid",
            path,
            IdfKind::Module,
            sid_title(&data),
            sid_author(&data),
            sid_lines(&data),
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
    } else if ext == "ay" || looks_like_ay(&data) {
        match parse_ay_info(&data) {
            Ok(song) => Some(info(
                "audio/x-ay",
                path,
                IdfKind::Module,
                song.first_track_name.clone(),
                song.author.clone(),
                ay_lines(&song),
            )),
            Err(_) if ext == "ay" => Some(info(
                "audio/x-ay",
                path,
                IdfKind::Module,
                None,
                None,
                vec![" AY file (header parsing failed)".into()],
            )),
            Err(_) => None,
        }
    } else if let Some(gme) = detect_gme_module(&data, &ext) {
        Some(info(
            gme.mime_type,
            path,
            gme.kind,
            None,
            None,
            vec![format!(" Format: {}", gme.label), " Decoder family: Game Music Emu".into()],
        ))
    } else if ext == "ayt" || looks_like_ayt(&data) {
        match parse_ayt_info(&data) {
            Ok(song) => Some(info(
                "audio/x-ayt",
                path,
                IdfKind::Module,
                None,
                None,
                ayt_lines(&song),
            )),
            Err(_) if ext == "ayt" => Some(info(
                "audio/x-ayt",
                path,
                IdfKind::Module,
                None,
                None,
                vec![" AYT file (header parsing failed)".into()],
            )),
            Err(_) => None,
        }
    } else if matches!(ext.as_str(), "ym" | "ym5" | "ym6") || looks_like_ym(&data) {
        let ym_info = parse_ym_info(&data).or_else(|_| {
            if matches!(ext.as_str(), "ym" | "ym5" | "ym6") && data.len() < file_len {
                let full = crate::file_cache::read_file(path, None)?;
                parse_ym_info(&full.bytes)
            } else {
                anyhow::bail!("YM header parsing failed")
            }
        });
        match ym_info {
            Ok(song) => Some(info(
                "audio/x-ym",
                path,
                IdfKind::Module,
                Some(song.song_name.clone()),
                Some(song.song_author.clone()),
                ym_lines(&song),
            )),
            Err(_) if matches!(ext.as_str(), "ym" | "ym5" | "ym6") => Some(info(
                "audio/x-ym",
                path,
                IdfKind::Module,
                None,
                None,
                vec![" YM file (header parsing failed)".into()],
            )),
            Err(_) => None,
        }
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
    } else if data.starts_with(b"ZXTape!\x1a") {
        Some(info(
            "application/x-tzx",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"RZX!") {
        Some(info(
            "application/x-rzx",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"\x13\x00\x00") {
        Some(info(
            "application/x-tap",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"PLUS3DOS\x1a") {
        Some(info(
            "application/x-plus3dos",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"CLIB\x1a") {
        Some(info(
            "application/x-ags-archive",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"PP20") || data.starts_with(b"PP11") {
        Some(info(
            "application/x-powerpacker",
            path,
            IdfKind::Archive,
            None,
            None,
            powerpacker_lines(&data),
        ))
    } else if data.starts_with(b"XPKF") {
        Some(info(
            "application/x-xpk",
            path,
            IdfKind::Archive,
            None,
            None,
            xpk_lines(&data),
        ))
    } else if data.starts_with(b"DMS!") {
        Some(info(
            "application/x-amiga-dms",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"IMP!") || data.starts_with(b"IMPL") {
        Some(info(
            "application/x-amiga-imploder",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if let Some(mime_type) = amiga_adf_mime_type(&data, file_len, &ext) {
        Some(info(mime_type, path, IdfKind::Archive, None, None, vec![]))
    } else if is_commodore_d64(file_len, &ext) {
        Some(info(
            "application/x-c64-d64",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(&[0x89, 0x4c, 0x5a, 0x4f, 0x00, 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(info(
            "application/x-lzop",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"070701")
        || data.starts_with(b"070702")
        || data.starts_with(b"070707")
    {
        Some(info(
            "application/x-cpio",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.len() >= 7
        && data[0] == 0xe9
        && data[1] == 0x2c
        && data[2] == 0x01
        && &data[3..7] == b"JAM\x20"
    {
        Some(info(
            "application/x-jam-archive",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"HPAK") {
        Some(info(
            "application/x-hpack",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"PAR\x00") {
        Some(info(
            "application/x-parity-archive",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"SQSH") {
        Some(info(
            "application/x-acorn-sqsh",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.len() >= 12
        && data[0] == 0
        && data[1..11].iter().all(|&b| b == b' ')
        && data[11] == 0
    {
        Some(info(
            "application/x-lbr",
            path,
            IdfKind::Archive,
            None,
            None,
            vec![],
        ))
    } else if data.starts_with(b"Archive\x00") {
        Some(info(
            "application/x-risc-os-arcfs",
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
    } else if let Some(mime_type) = affinity_mime_type_from_extension(&ext) {
        Some(info(mime_type, path, IdfKind::Other, None, None, vec![]))
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
        "image/x-webshots" | "application/x-webshots" => Some("Webshots picture"),
        "image/heic" => Some("HEIC bitmap"),
        "image/vnd.adobe.photoshop" => Some("Photoshop bitmap"),
        "image/x-tga" => Some("TGA bitmap"),
        "font/woff" => Some("WOFF font"),
        "font/woff2" => Some("WOFF2 font"),
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
        "audio/x-sid" => Some("Commodore 64 SID music"),
        "audio/x-mod" => Some("ProTracker module"),
        "audio/x-ayt" => Some("AYT tracker stream"),
        "audio/x-ym" => Some("YM tracker stream"),
        "audio/x-ym6" => Some("YM tracker stream"),
        "audio/x-vgm" => Some("VGM audio"),
        "application/x-lzop" => Some("LZOP compressed archive"),
        "application/x-cpio" => Some("CPIO archive"),
        "application/x-jam-archive" => Some("JAM archive"),
        "application/x-hpack" => Some("HPACK archive"),
        "application/x-parity-archive" => Some("Parity archive"),
        "image/jp2" => Some("JPEG 2000 bitmap"),
        "image/x-xpixmap" => Some("X PixMap bitmap"),
        "image/x-niff" => Some("NIFF image"),
        "image/vnd.djvu" => Some("DjVu image"),
        "image/x-fbm" => Some("FBM bitmap"),
        "image/x-portable-bitmap" => Some("Portable BitMap image"),
        "image/x-portable-graymap" => Some("Portable GrayMap image"),
        "image/x-portable-pixmap" => Some("Portable PixMap image"),
        "audio/x-fc14" => Some("Future Composer 1.4 module"),
        "audio/x-smod" => Some("Smod module"),
        "audio/x-aon4" => Some("Art Of Noise module"),
        "audio/x-arp" => Some("The Holy Noise module"),
        "audio/x-jamcracker" => Some("JamCracker module"),
        "audio/x-coso" => Some("Hippel-COSO module"),
        "audio/x-ftmn" => Some("FaceTheMusic module"),
        "audio/x-emod" => Some("Extended MOD module"),
        "audio/x-ctmf" => Some("Creative Music Format"),
        "application/x-ags-archive" => Some("Adventure Game Studio archive"),
        "application/x-uf2" => Some("UF2 firmware image"),
        "application/x-amstrad-cpc-amsdos" => Some("Amstrad AMSDOS file"),
        "application/x-amstrad-cpc-dsk" => Some("Amstrad CPC DSK image"),
        "application/x-amiga-adf-ofs" => Some("Amiga Disk Format (OFS) image"),
        "application/x-amiga-adf-ffs" => Some("Amiga Disk Format (FFS) image"),
        "application/x-c64-d64" => Some("Commodore 64 D64 disk image"),
        "application/x-powerpacker" => Some("PowerPacker compressed file"),
        "application/x-xpk" => Some("XPK compressed file"),
        "application/x-amiga-dms" => Some("Amiga DMS disk image"),
        "application/x-amiga-imploder" => Some("Amiga Imploder compressed file"),
        "application/x-bittorrent" => Some("BitTorrent metadata"),
        "application/x-sqlite3" => Some("SQLite database"),
        "application/x-apple-diskimage" => Some("Apple Disk Image"),
        "application/x-affinity-photo" => Some("Affinity Photo document"),
        "application/x-affinity-designer" => Some("Affinity Designer document"),
        "application/x-affinity-publisher" => Some("Affinity Publisher document"),
        "application/x-affinity-common" => Some("Affinity document"),
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
        "image/x-gem" => Some("GEM image"),
        "image/x-sun-raster" => Some("Sun raster image"),
        "image/x-cmu-raster" => Some("CMU raster image"),
        "image/x-solitaire" => Some("Solitaire image"),
        "image/x-miff" => Some("MIFF image"),
        "audio/x-octamed" => Some("OctaMED module"),
        "audio/x-octamed-compressed" => Some("OctaMED compressed module"),
        "application/x-acorn-sqsh" => Some("Acorn squished archive"),
        "application/x-lbr" => Some("LBR archive"),
        "application/x-risc-os-arcfs" => Some("RISC OS ArcFS archive"),
        "application/x-tzx" => Some("ZX Spectrum TZX tape image"),
        "application/x-rzx" => Some("ZX Spectrum RZX recording"),
        "application/x-tap" => Some("ZX Spectrum TAP tape image"),
        "application/x-plus3dos" => Some("Spectrum +3 DOS disk image"),
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

fn affinity_mime_type_from_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "afphoto" => Some("application/x-affinity-photo"),
        "afdesign" => Some("application/x-affinity-designer"),
        "afpub" => Some("application/x-affinity-publisher"),
        "aftemplate" | "afassets" | "afstyles" => Some("application/x-affinity-common"),
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

#[derive(Debug, Clone)]
struct AytInfo {
    version: u8,
    pattern_size: u8,
    sequence_count: usize,
    frame_count: usize,
    loop_frame: usize,
    frame_rate: u32,
    platform_name: &'static str,
    master_clock_hz: f64,
    active_registers: Vec<u8>,
}

#[derive(Debug, Clone)]
struct AyInfo {
    version: u8,
    track_count: usize,
    first_track: usize,
    author: Option<String>,
    comment: Option<String>,
    first_track_name: Option<String>,
    first_track_length_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct GmeModuleInfo {
    mime_type: &'static str,
    label: &'static str,
    kind: IdfKind,
}

fn looks_like_ayt(data: &[u8]) -> bool {
    parse_ayt_info(data).is_ok()
}

fn looks_like_ym(data: &[u8]) -> bool {
    parse_ym_info(data).is_ok()
}

fn looks_like_ay(data: &[u8]) -> bool {
    parse_ay_info(data).is_ok()
}

fn parse_ay_info(data: &[u8]) -> anyhow::Result<AyInfo> {
    if data.len() < 20 || data.get(0..8) != Some(b"ZXAYEMUL") {
        anyhow::bail!("Not an AY (ZXAYEMUL) file");
    }

    let version = data[8];
    let max_track = *data
        .get(16)
        .ok_or_else(|| anyhow::anyhow!("AY header truncated (max track)"))?
        as usize;
    let first_track_raw = *data
        .get(17)
        .ok_or_else(|| anyhow::anyhow!("AY header truncated (first track)"))?
        as usize;
    let track_count = max_track.saturating_add(1);

    let tracks_ptr = ay_get_data_ptr(data, 18, track_count.saturating_mul(4))
        .ok_or_else(|| anyhow::anyhow!("Missing AY track table"))?;

    let author = ay_get_data_ptr(data, 12, 1).and_then(|pos| ay_read_c_string(data, pos));
    let comment = ay_get_data_ptr(data, 14, 1).and_then(|pos| ay_read_c_string(data, pos));

    let first_track = first_track_raw.min(track_count.saturating_sub(1));
    let entry_pos = tracks_ptr + first_track * 4;
    let first_track_name =
        ay_get_data_ptr(data, entry_pos, 1).and_then(|pos| ay_read_c_string(data, pos));

    let first_track_length_ms = ay_get_data_ptr(data, entry_pos + 2, 6).and_then(|pos| {
        if pos + 6 <= data.len() {
            let frames = u16::from_be_bytes([data[pos + 4], data[pos + 5]]) as u32;
            if frames == 0 {
                None
            } else {
                Some(frames.saturating_mul(20))
            }
        } else {
            None
        }
    });

    Ok(AyInfo {
        version,
        track_count,
        first_track,
        author,
        comment,
        first_track_name,
        first_track_length_ms,
    })
}

fn ay_get_data_ptr(data: &[u8], ptr_pos: usize, min_size: usize) -> Option<usize> {
    if ptr_pos + 2 > data.len() {
        return None;
    }
    let offset = i16::from_be_bytes([data[ptr_pos], data[ptr_pos + 1]]) as isize;
    if offset == 0 {
        return None;
    }
    let target = ptr_pos as isize + offset;
    if target < 0 {
        return None;
    }
    let start = target as usize;
    if start.checked_add(min_size)? > data.len() {
        return None;
    }
    Some(start)
}

fn ay_read_c_string(data: &[u8], pos: usize) -> Option<String> {
    if pos >= data.len() {
        return None;
    }
    let end = data[pos..]
        .iter()
        .position(|&b| b == 0)
        .map(|idx| pos + idx)
        .unwrap_or(data.len());
    let text = std::str::from_utf8(&data[pos..end]).ok()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn ay_lines(song: &AyInfo) -> Vec<String> {
    let mut lines = vec![
        " Format: ZXAYEMUL".to_string(),
        format!(" Version: {}", song.version),
        format!(" Tracks: {}", song.track_count),
        format!(" First track: {}", song.first_track + 1),
    ];
    if let Some(ms) = song.first_track_length_ms {
        lines.push(format!(" First track length: {:.2} s", ms as f64 / 1000.0));
    }
    if let Some(name) = &song.first_track_name {
        lines.push(format!(" Track name: {}", name));
    }
    if let Some(comment) = &song.comment {
        if !comment.is_empty() {
            lines.push(format!(" Comment: {}", comment));
        }
    }
    lines
}

fn detect_gme_module(data: &[u8], ext: &str) -> Option<GmeModuleInfo> {
    if data.starts_with(b"NSFE") || ext == "nsfe" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-nsfe",
            label: "NSFE",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"NESM\x1A") || ext == "nsf" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-nsf",
            label: "NSF",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"SNES-SPC700 Sound File Data") || ext == "spc" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-spc",
            label: "SPC",
            kind: IdfKind::Sample,
        });
    }
    if data.starts_with(b"GBS") || ext == "gbs" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-gbs",
            label: "GBS",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"GYMX") || data.starts_with(b"GYM ") || ext == "gym" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-gym",
            label: "GYM",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"HESM") || ext == "hes" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-hes",
            label: "HES",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"KSSX") || ext == "kss" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-kss",
            label: "KSS",
            kind: IdfKind::Module,
        });
    }
    if data.starts_with(b"SAP\r\n") || data.starts_with(b"SAP\n") || ext == "sap" {
        return Some(GmeModuleInfo {
            mime_type: "audio/x-sap",
            label: "SAP",
            kind: IdfKind::Module,
        });
    }
    None
}

fn parse_ayt_info(data: &[u8]) -> anyhow::Result<AytInfo> {
    if data.len() < 14 {
        anyhow::bail!("AYT file too short");
    }

    let version = data[0];
    let active_mask = u16::from_le_bytes([data[1], data[2]]);
    let pattern_size = data[3];
    if pattern_size == 0 {
        anyhow::bail!("Invalid AYT pattern size");
    }

    let first_seq = u16::from_le_bytes([data[4], data[5]]) as usize;
    let loop_seq = u16::from_le_bytes([data[6], data[7]]) as usize;
    let nb_ptr = u16::from_le_bytes([data[10], data[11]]) as usize;
    let platform_freq = data[12];

    if first_seq < 14 || first_seq > data.len() {
        anyhow::bail!("Invalid AYT first sequence pointer");
    }

    let active_registers = (0u8..14)
        .filter(|reg| {
            let bit = 15usize.saturating_sub(*reg as usize);
            bit >= 2 && ((active_mask >> bit) & 1) != 0
        })
        .collect::<Vec<_>>();
    if active_registers.is_empty() {
        anyhow::bail!("AYT has no active registers");
    }

    let present_count = active_registers.len();
    if nb_ptr < present_count {
        anyhow::bail!("Invalid AYT pointer count");
    }
    let seq_words = nb_ptr - present_count;
    if seq_words == 0 || seq_words % present_count != 0 {
        anyhow::bail!("Invalid AYT sequence words");
    }

    let sequence_count = seq_words / present_count;
    let frame_count = sequence_count * pattern_size as usize;
    let one_seq_bytes = present_count * 2;
    let loop_frame = if loop_seq >= first_seq && one_seq_bytes > 0 {
        let delta = loop_seq - first_seq;
        if delta % one_seq_bytes == 0 {
            (delta / one_seq_bytes) * pattern_size as usize
        } else {
            0
        }
    } else {
        0
    };

    let platform_id = platform_freq & 0x1F;
    let freq_code = (platform_freq >> 5) & 0x07;
    let (platform_name, master_clock_hz) = match platform_id {
        0 => ("Amstrad CPC", 1_000_000.0),
        1 => ("Oric", 1_000_000.0),
        2 => ("ZXUno", 1_750_000.0),
        3 => ("Pentagon", 1_750_000.0),
        4 => ("Timex TS2068", 1_764_000.0),
        5 => ("ZX 128", 1_773_450.0),
        6 => ("MSX", 1_789_772.0),
        7 => ("Atari ST", 2_000_000.0),
        8 => ("VG5000", 1_000_000.0),
        _ => ("Unknown", 1_000_000.0),
    };
    let frame_rate = match freq_code {
        0 => 50,
        1 => 25,
        2 => 60,
        3 => 30,
        4 => 100,
        5 => 200,
        _ => 50,
    };

    Ok(AytInfo {
        version,
        pattern_size,
        sequence_count,
        frame_count,
        loop_frame,
        frame_rate,
        platform_name,
        master_clock_hz,
        active_registers,
    })
}

fn ayt_lines(song: &AytInfo) -> Vec<String> {
    vec![
        format!(" Version: {}.{}", song.version >> 4, song.version & 0x0F),
        format!(" Platform: {}", song.platform_name),
        format!(" Frame rate: {} Hz", song.frame_rate),
        format!(" Master clock: {:.0} Hz", song.master_clock_hz),
        format!(" Pattern size: {}", song.pattern_size),
        format!(" Sequences: {}", song.sequence_count),
        format!(" Frames: {}", song.frame_count),
        format!(" Loop frame: {}", song.loop_frame),
        format!(" Active regs: {:?}", song.active_registers),
    ]
}

#[derive(Debug, Clone)]
struct YmInfo {
    magic: String,
    nb_frames: u32,
    attributes: u32,
    nb_drums: u16,
    clock_rate: u32,
    player_rate: u16,
    loop_frame: u32,
    song_name: String,
    song_author: String,
    song_comment: String,
}

fn parse_ym_info(data: &[u8]) -> anyhow::Result<YmInfo> {
    let owned;
    let data = if is_lzh_compressed(data) {
        owned = decompress_lzh(data)?;
        owned.as_slice()
    } else {
        data
    };

    if data.len() < 12 {
        anyhow::bail!("YM file too short");
    }

    let magic_bytes: [u8; 4] = data[..4]
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid YM header"))?;
    let magic = std::str::from_utf8(&magic_bytes)
        .unwrap_or("YM??")
        .to_string();

    if magic == "YM2!" || magic == "YM3!" || magic == "YM3b" {
        return parse_ym_info_legacy(data, &magic);
    }

    if magic != "YM5!" && magic != "YM6!" {
        anyhow::bail!("Not a valid YM2/YM3/YM5/YM6 file");
    }
    if data.get(4..12) != Some(b"LeOnArD!") {
        anyhow::bail!("Invalid YM signature");
    }

    let mut ptr = 12usize;
    let nb_frames = read_be_u32(data, &mut ptr)?;
    let attributes = read_be_u32(data, &mut ptr)?;
    let nb_drums = read_be_u16(data, &mut ptr)?;
    let clock_rate = read_be_u32(data, &mut ptr)?;
    let player_rate = read_be_u16(data, &mut ptr)?;
    let loop_frame = read_be_u32(data, &mut ptr)?;
    let extra_size = read_be_u16(data, &mut ptr)? as usize;

    ptr = ptr.saturating_add(extra_size);
    if ptr > data.len() {
        anyhow::bail!("YM extra data exceeds file size");
    }

    for _ in 0..nb_drums {
        let drum_size = read_be_u32(data, &mut ptr)? as usize;
        ptr = ptr.saturating_add(drum_size);
        if ptr > data.len() {
            anyhow::bail!("YM digidrum data exceeds file size");
        }
    }

    let song_name = read_nt_string(data, &mut ptr)?;
    let song_author = read_nt_string(data, &mut ptr)?;
    let song_comment = read_nt_string(data, &mut ptr)?;

    Ok(YmInfo {
        magic,
        nb_frames,
        attributes,
        nb_drums,
        clock_rate,
        player_rate,
        loop_frame,
        song_name,
        song_author,
        song_comment,
    })
}

fn parse_ym_info_legacy(data: &[u8], magic: &str) -> anyhow::Result<YmInfo> {
    if data.len() < 4 + 14 {
        anyhow::bail!("Legacy YM file too short");
    }

    let payload_len = data.len() - 4;
    let (nb_frames, loop_frame) = if magic == "YM3b" {
        if payload_len < 4 {
            anyhow::bail!("YM3b file too short");
        }
        let frames = (payload_len - 4) / 14;
        let loop_bytes = &data[data.len() - 4..];
        let loop_frame =
            u32::from_le_bytes([loop_bytes[0], loop_bytes[1], loop_bytes[2], loop_bytes[3]]);
        (frames as u32, loop_frame)
    } else {
        ((payload_len / 14) as u32, 0)
    };

    if nb_frames == 0 {
        anyhow::bail!("Legacy YM contains no frames");
    }

    Ok(YmInfo {
        magic: magic.to_string(),
        nb_frames,
        attributes: 1,
        nb_drums: 0,
        clock_rate: 2_000_000,
        player_rate: 50,
        loop_frame,
        song_name: String::new(),
        song_author: String::new(),
        song_comment: String::new(),
    })
}

fn ym_lines(song: &YmInfo) -> Vec<String> {
    let mut lines = vec![
        format!(" Format: {}", song.magic),
        format!(" Frames: {}", song.nb_frames),
        format!(" Player rate: {} Hz", song.player_rate),
        format!(" Clock rate: {} Hz", song.clock_rate),
        format!(" Loop frame: {}", song.loop_frame),
        format!(" Attributes: 0x{:08X}", song.attributes),
        format!(" Digidrums: {}", song.nb_drums),
    ];

    if !song.song_name.is_empty() {
        lines.push(format!(" Title: {}", song.song_name));
    }
    if !song.song_author.is_empty() {
        lines.push(format!(" Author: {}", song.song_author));
    }
    if !song.song_comment.is_empty() {
        lines.push(String::new());
        lines.push(" Comment:".into());
        lines.extend(song.song_comment.lines().map(|line| format!("  {line}")));
    }

    lines
}

fn is_lzh_compressed(data: &[u8]) -> bool {
    data.len() >= 7 && data.get(2..5) == Some(b"-lh") && data.get(6) == Some(&b'-')
}

fn decompress_lzh(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    match decompress_lzh_strict(data) {
        Ok(out) => Ok(out),
        Err(strict_err) => {
            if let Ok(out) = decompress_lzh_stsound_compat(data) {
                return Ok(out);
            }
            if let Some(repaired) = repair_lzh_level0_header_checksum(data) {
                decompress_lzh_strict(&repaired).map_err(|retry_err| {
                    anyhow::anyhow!(
                        "YM LZH decode failed (strict: {strict_err}; checksum-repair retry: {retry_err})"
                    )
                })
            } else {
                Err(strict_err)
            }
        }
    }
}

fn decompress_lzh_strict(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut reader = delharc::LhaDecodeReader::new(data)
        .map_err(|err| anyhow::anyhow!("YM LZH header decode failed: {err}"))?;

    loop {
        if !reader.header().is_directory() {
            if !reader.is_decoder_supported() {
                anyhow::bail!("YM LZH compression method is not supported");
            }

            let mut out = Vec::new();
            reader
                .read_to_end(&mut out)
                .map_err(|err| anyhow::anyhow!("YM LZH data decode failed: {err}"))?;
            reader
                .crc_check()
                .map_err(|err| anyhow::anyhow!("YM LZH CRC check failed: {err}"))?;
            return Ok(out);
        }

        let has_more = reader
            .next_file()
            .map_err(|err| anyhow::anyhow!("YM LZH next entry failed: {err}"))?;
        if !has_more {
            break;
        }
    }

    anyhow::bail!("YM LZH archive has no decodable file entry");
}

fn decompress_lzh_stsound_compat(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < 22 || data.get(2..7) != Some(b"-lh5-") {
        anyhow::bail!("Not an LH5 stream");
    }

    let header_size = data[0];
    if header_size == 0 {
        anyhow::bail!("Not compressed");
    }

    let packed_size = u32::from_le_bytes([data[7], data[8], data[9], data[10]]) as usize;
    let original_size = u32::from_le_bytes([data[11], data[12], data[13], data[14]]) as usize;
    let level = data[20];
    let name_len = data[21] as usize;
    if original_size == 0 {
        anyhow::bail!("Empty LH5 output");
    }
    if level > 1 {
        anyhow::bail!("Unsupported LH5 header level");
    }

    let mut ptr = 22usize
        .checked_add(name_len)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| anyhow::anyhow!("LH5 header offset overflow"))?;
    if level == 1 {
        ptr = ptr
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("LH5 header offset overflow"))?;
        loop {
            let size_bytes = data
                .get(ptr..ptr + 2)
                .ok_or_else(|| anyhow::anyhow!("Truncated LH5 extended header"))?;
            ptr += 2;
            let next_header_size = u16::from_le_bytes([size_bytes[0], size_bytes[1]]) as usize;
            if next_header_size == 0 {
                break;
            }
            ptr = ptr
                .checked_add(next_header_size)
                .ok_or_else(|| anyhow::anyhow!("LH5 extended header offset overflow"))?;
            if ptr > data.len() {
                anyhow::bail!("LH5 extended header exceeds file size");
            }
        }
    }

    if ptr >= data.len() {
        anyhow::bail!("LH5 payload is missing");
    }
    let available = data.len() - ptr;
    let packed_size = packed_size.min(available);
    let mut out = vec![0; original_size];
    let mut decoder = Lh5Decoder::new(&data[ptr..ptr + packed_size]);
    decoder.fill_buffer(&mut out)?;
    Ok(out)
}

fn repair_lzh_level0_header_checksum(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 22 || data.get(2..5) != Some(b"-lh") || data.get(6) != Some(&b'-') {
        return None;
    }
    let header_size = data[0] as usize;
    let end = 2usize.checked_add(header_size)?;
    if end > data.len() {
        return None;
    }

    let checksum = data[2..end]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    let mut repaired = data.to_vec();
    repaired[1] = checksum;
    Some(repaired)
}

fn read_be_u32(data: &[u8], ptr: &mut usize) -> anyhow::Result<u32> {
    let end = ptr.saturating_add(4);
    let bytes = data
        .get(*ptr..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated YM header"))?;
    *ptr = end;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_be_u16(data: &[u8], ptr: &mut usize) -> anyhow::Result<u16> {
    let end = ptr.saturating_add(2);
    let bytes = data
        .get(*ptr..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated YM header"))?;
    *ptr = end;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_nt_string(data: &[u8], ptr: &mut usize) -> anyhow::Result<String> {
    let start = *ptr;
    let mut len = 0usize;
    while start + len < data.len() {
        if data[start + len] == 0 {
            let out = String::from_utf8_lossy(&data[start..start + len]).to_string();
            *ptr = start + len + 1;
            return Ok(out);
        }
        if len > 4096 {
            anyhow::bail!("YM string too long or missing terminator");
        }
        len += 1;
    }
    anyhow::bail!("YM string extends beyond file end");
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

fn sqlite_lines(data: &[u8]) -> Vec<String> {
    if data.len() < 100 {
        return Vec::new();
    }

    let page_size = match u16::from_be_bytes([data[16], data[17]]) {
        1 => 65_536,
        value => value as u32,
    };
    let write_version = sqlite_journal_mode(data[18]);
    let read_version = sqlite_journal_mode(data[19]);
    let page_count = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);
    let schema_version = u32::from_be_bytes([data[40], data[41], data[42], data[43]]);
    let user_version = u32::from_be_bytes([data[60], data[61], data[62], data[63]]);

    let mut out = vec![format!(" Page size {} bytes", page_size)];
    if page_count > 0 {
        out.push(format!(" {} page(s)", page_count));
    }
    out.push(format!(
        " Journal write/read {}/{}",
        write_version, read_version
    ));
    if schema_version > 0 {
        out.push(format!(" Schema version {}", schema_version));
    }
    if user_version > 0 {
        out.push(format!(" User version {}", user_version));
    }
    out
}

fn sqlite_journal_mode(value: u8) -> &'static str {
    match value {
        1 => "legacy",
        2 => "wal",
        _ => "?",
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

fn read_le_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn is_webshots_wbc(data: &[u8], file_len: usize) -> bool {
    // Webshots WBC header (Setaou, 2006):
    // offset 0: int32 file tag = -1778772309 (AB 16 FA 95)
    if !data.starts_with(&[0xAB, 0x16, 0xFA, 0x95]) {
        return false;
    }

    // Header size field at offset 4 is documented as 8606 in typical files.
    // Keep this permissive to avoid false negatives across WBC variants.
    let Some(header_size) = read_le_u32(data, 4) else {
        return false;
    };
    if header_size < 2196 || header_size as usize > file_len {
        return false;
    }

    true
}

fn webshots_title(data: &[u8]) -> Option<String> {
    // offset 12, char[256] file title
    let raw = data.get(12..(12 + 256))?;
    fixed_text(raw)
}

fn webshots_wbc_lines(data: &[u8], file_len: usize) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(header_size) = read_le_u32(data, 4) {
        out.push(format!(" Header size: {}", header_size));
    }

    // offset 2196, int32 unit count
    if let Some(unit_count) = read_le_u32(data, 2196) {
        out.push(format!(" Units: {}", unit_count));

        // First index item starts at 2200 with a unit offset int32.
        if unit_count > 0 {
            if let Some(first_unit_offset) = read_le_u32(data, 2200) {
                out.push(format!(" First unit offset: {}", first_unit_offset));

                if first_unit_offset as usize + 4 <= data.len() {
                    let looks_unit = data
                        [first_unit_offset as usize..first_unit_offset as usize + 4]
                        == [0xE2, 0xCD, 0x71, 0xF0];
                    if looks_unit {
                        out.push(" Unit tag: E2 CD 71 F0".to_string());
                    }
                } else if (first_unit_offset as usize) < file_len {
                    out.push(" Unit tag: outside probe window".to_string());
                }
            }
        }
    }

    out
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

#[derive(Default)]
struct Id3Tags {
    version: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    year: Option<String>,
    track: Option<String>,
    genre: Option<String>,
}

fn mp3_info(path: &Path, data: &[u8], ext: &str) -> Option<IdInfo> {
    let has_id3v2 = data.starts_with(b"ID3");
    let tags_v1 = read_id3v1_tags(path);
    let has_id3v1 = tags_v1.is_some();
    let is_mp3_ext = matches!(ext, "mp3" | "mp2" | "mp1" | "mpeg" | "mpga");
    let has_frame = has_mpeg_audio_frame(data);

    // Require multiple hints before classifying as MP3 to avoid false positives
    // on arbitrary binary data that happens to contain a frame-like sync word.
    let mut evidence = 0u8;
    if has_id3v2 {
        evidence += 2;
    }
    if has_id3v1 {
        evidence += 1;
    }
    if is_mp3_ext {
        evidence += 1;
    }
    if has_frame {
        evidence += 1;
    }

    if evidence < 2 {
        return None;
    }

    let tags_v2 = parse_id3v2_tags(data);
    let merged = merge_id3_tags(tags_v2, tags_v1);

    let mut extra = Vec::new();
    if let Some(version) = merged.version.as_ref() {
        extra.push(format!("ID3: {version}"));
    } else if has_frame {
        extra.push("MPEG audio stream".into());
    }
    if let Some(album) = merged.album.as_ref() {
        extra.push(format!("Album: {album}"));
    }
    if let Some(year) = merged.year.as_ref() {
        extra.push(format!("Year: {year}"));
    }
    if let Some(track) = merged.track.as_ref() {
        extra.push(format!("Track: {track}"));
    }
    if let Some(genre) = merged.genre.as_ref() {
        extra.push(format!("Genre: {genre}"));
    }

    Some(info(
        "audio/mpeg",
        path,
        IdfKind::Sample,
        merged.title,
        merged.artist,
        extra,
    ))
}

fn has_mpeg_audio_frame(data: &[u8]) -> bool {
    let limit = data.len().min(4096);
    for i in 0..limit.saturating_sub(4) {
        if is_valid_mpeg_header(&data[i..i + 4]) {
            return true;
        }
    }
    false
}

fn is_valid_mpeg_header(hdr: &[u8]) -> bool {
    if hdr.len() < 4 {
        return false;
    }
    let h = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let sync = (h >> 21) & 0x7ff;
    if sync != 0x7ff {
        return false;
    }
    let version = (h >> 19) & 0x3;
    if version == 0x1 {
        return false;
    }
    let layer = (h >> 17) & 0x3;
    if layer == 0x0 {
        return false;
    }
    let bitrate_idx = (h >> 12) & 0x0f;
    if bitrate_idx == 0 || bitrate_idx == 0x0f {
        return false;
    }
    let sample_idx = (h >> 10) & 0x03;
    if sample_idx == 0x03 {
        return false;
    }
    true
}

fn merge_id3_tags(v2: Option<Id3Tags>, v1: Option<Id3Tags>) -> Id3Tags {
    let mut out = v2.unwrap_or_default();
    if let Some(v1) = v1 {
        if out.version.is_none() {
            out.version = v1.version;
        }
        if out.title.is_none() {
            out.title = v1.title;
        }
        if out.artist.is_none() {
            out.artist = v1.artist;
        }
        if out.album.is_none() {
            out.album = v1.album;
        }
        if out.year.is_none() {
            out.year = v1.year;
        }
        if out.track.is_none() {
            out.track = v1.track;
        }
        if out.genre.is_none() {
            out.genre = v1.genre;
        }
    }
    out
}

fn parse_id3v2_tags(data: &[u8]) -> Option<Id3Tags> {
    if data.len() < 10 || !data.starts_with(b"ID3") {
        return None;
    }
    let major = data[3];
    if major == 0 || major > 4 {
        return None;
    }
    let size = synchsafe_u32(&data[6..10]) as usize;
    let tag_end = (10usize).saturating_add(size).min(data.len());
    let mut pos = 10usize;
    let mut tags = Id3Tags {
        version: Some(format!("ID3v2.{}", major)),
        ..Id3Tags::default()
    };

    while pos + 10 <= tag_end {
        let id = &data[pos..pos + 4];
        if id == b"\0\0\0\0" {
            break;
        }
        if !id
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            break;
        }
        let frame_size = if major == 4 {
            synchsafe_u32(&data[pos + 4..pos + 8]) as usize
        } else {
            u32::from_be_bytes(
                data[pos + 4..pos + 8]
                    .try_into()
                    .expect("ID3 frame size bytes"),
            ) as usize
        };
        pos += 10;
        if frame_size == 0 || pos + frame_size > tag_end {
            break;
        }
        let payload = &data[pos..pos + frame_size];
        match id {
            b"TIT2" => tags.title = decode_id3_text(payload),
            b"TPE1" => tags.artist = decode_id3_text(payload),
            b"TALB" => tags.album = decode_id3_text(payload),
            b"TYER" | b"TDRC" => tags.year = decode_id3_text(payload),
            b"TRCK" => tags.track = decode_id3_text(payload),
            b"TCON" => tags.genre = decode_id3_text(payload),
            _ => {}
        }
        pos += frame_size;
    }

    Some(tags)
}

fn synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 21)
        | ((bytes[1] as u32) << 14)
        | ((bytes[2] as u32) << 7)
        | (bytes[3] as u32)
}

fn decode_id3_text(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let enc = payload[0];
    let raw = &payload[1..];
    let text = match enc {
        0 => decode_latin1(raw),
        1 => decode_utf16_with_bom(raw)?,
        2 => decode_utf16_be(raw)?,
        3 => String::from_utf8(raw.to_vec()).ok()?,
        _ => return None,
    };
    let trimmed = text.trim_matches(char::from(0)).trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn decode_latin1(raw: &[u8]) -> String {
    raw.iter().map(|b| *b as char).collect::<String>()
}

fn decode_utf16_with_bom(raw: &[u8]) -> Option<String> {
    if raw.len() < 2 {
        return None;
    }
    if raw.starts_with(&[0xff, 0xfe]) {
        decode_utf16_bytes(&raw[2..], true)
    } else if raw.starts_with(&[0xfe, 0xff]) {
        decode_utf16_bytes(&raw[2..], false)
    } else {
        decode_utf16_bytes(raw, true)
    }
}

fn decode_utf16_be(raw: &[u8]) -> Option<String> {
    decode_utf16_bytes(raw, false)
}

fn decode_utf16_bytes(raw: &[u8], little_endian: bool) -> Option<String> {
    if raw.len() < 2 {
        return None;
    }
    let mut units = Vec::with_capacity(raw.len() / 2);
    for chunk in raw.chunks_exact(2) {
        let u = if little_endian {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], chunk[1]])
        };
        if u == 0 {
            break;
        }
        units.push(u);
    }
    String::from_utf16(&units).ok()
}

fn read_id3v1_tags(path: &Path) -> Option<Id3Tags> {
    let tail = crate::file_cache::read_tail(path, 128).ok()?;
    if tail.file_len < 128 {
        return None;
    }
    parse_id3v1_tags(&tail.bytes)
}

fn parse_id3v1_tags(tag: &[u8]) -> Option<Id3Tags> {
    if tag.len() != 128 || &tag[0..3] != b"TAG" {
        return None;
    }
    let title = fixed_text(&tag[3..33]);
    let artist = fixed_text(&tag[33..63]);
    let album = fixed_text(&tag[63..93]);
    let year = fixed_text(&tag[93..97]);
    let track = if tag[125] == 0 && tag[126] != 0 {
        Some(tag[126].to_string())
    } else {
        None
    };
    let genre = if tag[127] != 255 {
        Some(tag[127].to_string())
    } else {
        None
    };

    Some(Id3Tags {
        version: Some("ID3v1".into()),
        title,
        artist,
        album,
        year,
        track,
        genre,
    })
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

fn amiga_adf_mime_type(data: &[u8], file_len: usize, ext: &str) -> Option<&'static str> {
    if ext != "adf" || file_len < 1024 || file_len % 512 != 0 || data.get(..3) != Some(b"DOS") {
        return None;
    }

    match data.get(3).copied() {
        Some(0) | Some(2) | Some(4) => Some("application/x-amiga-adf-ofs"),
        Some(1) | Some(3) | Some(5) => Some("application/x-amiga-adf-ffs"),
        _ => None,
    }
}

fn powerpacker_lines(data: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    if data.len() < 8 {
        return lines;
    }
    let version = if data.starts_with(b"PP20") {
        "PowerPacker 2.0"
    } else {
        "PowerPacker 1.1"
    };
    lines.push(version.to_string());
    if data.len() >= 8 {
        let efficiency = match (data[4], data[5], data[6], data[7]) {
            (9, 9, 9, 9) => "Efficiency: Fast",
            (9, 10, 10, 10) => "Efficiency: Mediocre",
            (9, 10, 11, 11) => "Efficiency: Good",
            (9, 10, 12, 12) => "Efficiency: Very Good",
            (9, 10, 12, 13) => "Efficiency: Best",
            _ => "Efficiency: Unknown",
        };
        lines.push(efficiency.to_string());
    }
    lines
}

fn xpk_lines(data: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    if data.len() >= 12 {
        // XPK packer ID is at offset 8, 4 bytes
        if let Ok(name) = std::str::from_utf8(&data[8..12]) {
            if name.chars().all(|c| c.is_ascii_alphanumeric()) {
                lines.push(format!("XPK sub-packer: {}", name));
            }
        }
    }
    lines
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
    if let Ok(reader) = delharc::LhaDecodeReader::new(data) {
        let path = reader.header().parse_pathname();
        let raw_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string();
        let trimmed = raw_name
            .trim_matches(|c: char| c == '\0' || c.is_control() || c.is_whitespace())
            .to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    // Fallback parser for truncated probes or malformed headers.
    if data.len() < 22 {
        return None;
    }
    let name_len = data[19] as usize;
    if name_len == 0 || 20 + name_len > data.len() {
        return None;
    }
    let name = &data[20..20 + name_len];
    let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
    let s = String::from_utf8_lossy(name)
        .trim_matches(|c: char| c == '\0' || c.is_control() || c.is_whitespace())
        .to_string();
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

fn is_sid(data: &[u8]) -> bool {
    data.len() >= 0x76 && (data.starts_with(b"PSID") || data.starts_with(b"RSID"))
}

fn sid_title(data: &[u8]) -> Option<String> {
    fixed_text(data.get(0x16..0x36)?)
}

fn sid_author(data: &[u8]) -> Option<String> {
    fixed_text(data.get(0x36..0x56)?)
}

fn sid_lines(data: &[u8]) -> Vec<String> {
    if data.len() < 0x76 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let format = if data.starts_with(b"RSID") {
        "RSID"
    } else {
        "PSID"
    };
    let version = u16::from_be_bytes([data[4], data[5]]);
    let data_offset = u16::from_be_bytes([data[6], data[7]]);
    let load_address = u16::from_be_bytes([data[8], data[9]]);
    let init_address = u16::from_be_bytes([data[10], data[11]]);
    let play_address = u16::from_be_bytes([data[12], data[13]]);
    let songs = u16::from_be_bytes([data[14], data[15]]);
    let start_song = u16::from_be_bytes([data[16], data[17]]);

    out.push(format!(" {} v{}", format, version));
    out.push(format!(" {} song(s), start song {}", songs, start_song));
    out.push(format!(" Data offset: 0x{:04X}", data_offset));
    out.push(format!(" Load address: 0x{:04X}", load_address));
    out.push(format!(" Init address: 0x{:04X}", init_address));
    out.push(format!(" Play address: 0x{:04X}", play_address));
    if let Some(released) = fixed_text(&data[0x56..0x76]) {
        out.push(format!(" Released: {}", released));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_test_fixed_text(target: &mut [u8], value: &str) {
        let bytes = value.as_bytes();
        let len = bytes.len().min(target.len());
        target[..len].copy_from_slice(&bytes[..len]);
    }

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
    fn pdf_is_not_misdetected_as_mp3() {
        let path = std::env::temp_dir().join(format!("kkc-idf-pdf-mp3-{}.pdf", std::process::id()));
        let mut data = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nstream\n".to_vec();
        // MPEG-like header bytes inside PDF payload should not trigger MP3 detection.
        data.extend([0xff, 0xfb, 0x90, 0x64, 0x00, 0x01, 0x02, 0x03]);
        data.extend(b"\nendstream\nendobj\n");
        fs::write(&path, data).expect("write pdf");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("pdf should be detected");
        assert_eq!(info.mime_types[0], "application/pdf");
        assert_eq!(info.format, "PDF document");

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
    fn mp3_with_id3v1_tags_is_reported() {
        let path = std::env::temp_dir().join(format!("kkc-idf-mp3-v1-{}.mp3", std::process::id()));
        let mut data = vec![0u8; 256];
        data.extend([0xff, 0xfb, 0x90, 0x64]);
        data.extend(id3v1_tag(
            "Demo Song",
            "Demo Artist",
            "Demo Album",
            "1999",
            7,
            13,
        ));
        fs::write(&path, data).expect("write mp3");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("mp3 should be detected");
        assert_eq!(info.mime_types[0], "audio/mpeg");
        assert_eq!(info.title.as_deref(), Some("Demo Song"));
        assert_eq!(info.composer.as_deref(), Some("Demo Artist"));
        assert!(info.extra.iter().any(|line| line.contains("ID3: ID3v1")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("Album: Demo Album"))
        );
        assert!(info.extra.iter().any(|line| line.contains("Year: 1999")));
        assert!(info.extra.iter().any(|line| line.contains("Track: 7")));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn mp3_detected_by_frame_header_without_id3() {
        let path =
            std::env::temp_dir().join(format!("kkc-idf-mp3-frame-{}.mp3", std::process::id()));
        fs::write(&path, [0xff, 0xfb, 0x90, 0x64, 0, 1, 2, 3, 4]).expect("write mp3");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("mp3 should be detected");
        assert_eq!(info.mime_types[0], "audio/mpeg");
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("MPEG audio stream"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn sid_files_are_detected() {
        let path = std::env::temp_dir().join(format!("kkc-idf-sid-{}.sid", std::process::id()));
        let mut data = vec![0u8; 0x76];
        data[0..4].copy_from_slice(b"PSID");
        data[4..6].copy_from_slice(&2u16.to_be_bytes());
        data[6..8].copy_from_slice(&0x007cu16.to_be_bytes());
        data[8..10].copy_from_slice(&0x1000u16.to_be_bytes());
        data[10..12].copy_from_slice(&0x1000u16.to_be_bytes());
        data[12..14].copy_from_slice(&0x1003u16.to_be_bytes());
        data[14..16].copy_from_slice(&3u16.to_be_bytes());
        data[16..18].copy_from_slice(&1u16.to_be_bytes());
        write_test_fixed_text(&mut data[0x16..0x36], "Demo SID");
        write_test_fixed_text(&mut data[0x36..0x56], "Demo Composer");
        write_test_fixed_text(&mut data[0x56..0x76], "2026 KKC");
        fs::write(&path, data).expect("write sid");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("sid should be detected");
        assert_eq!(info.mime_types[0], "audio/x-sid");
        assert_eq!(info.kind, IdfKind::Module);
        assert_eq!(info.title.as_deref(), Some("Demo SID"));
        assert_eq!(info.composer.as_deref(), Some("Demo Composer"));
        assert!(info.extra.iter().any(|line| line.contains("PSID v2")));
        assert!(
            info.extra
                .iter()
                .any(|line| line.contains("3 song(s), start song 1"))
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

    #[test]
    fn woff_fonts_are_detected() {
        let path = std::env::temp_dir().join(format!("kkc-idf-woff-{}.woff", std::process::id()));
        fs::write(&path, b"wOFF\x00\x01\x00\x00\x00\x00\x00\x2c").expect("write woff");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("woff should be detected");
        assert_eq!(info.mime_types[0], "font/woff");
        assert_eq!(info.format, "WOFF font");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn woff2_fonts_are_detected() {
        let path = std::env::temp_dir().join(format!("kkc-idf-woff2-{}.woff2", std::process::id()));
        fs::write(&path, b"wOF2\x00\x01\x00\x00\x00\x00\x00\x2c").expect("write woff2");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("woff2 should be detected");
        assert_eq!(info.mime_types[0], "font/woff2");
        assert_eq!(info.format, "WOFF2 font");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn sqlite_databases_are_detected() {
        let path =
            std::env::temp_dir().join(format!("kkc-idf-sqlite-{}.sqlite", std::process::id()));
        let mut data = vec![0u8; 100];
        data[..16].copy_from_slice(b"SQLite format 3\0");
        data[16..18].copy_from_slice(&4096u16.to_be_bytes());
        data[18] = 1;
        data[19] = 1;
        data[28..32].copy_from_slice(&7u32.to_be_bytes());
        data[40..44].copy_from_slice(&3u32.to_be_bytes());
        data[60..64].copy_from_slice(&42u32.to_be_bytes());
        fs::write(&path, data).expect("write sqlite");

        let info = probe_file(&path)
            .expect("probe should not fail")
            .expect("sqlite should be detected");
        assert_eq!(info.mime_types[0], "application/x-sqlite3");
        assert_eq!(info.format, "SQLite database");
        assert_eq!(info.kind, IdfKind::Other);
        assert!(info.extra.iter().any(|line| line.contains("4096 bytes")));
        assert!(info.extra.iter().any(|line| line.contains("7 page(s)")));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn affinity_files_are_detected_by_extension() {
        let root = std::env::temp_dir().join(format!("kkc-idf-affinity-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create temp dir");

        let cases = [
            (
                "sample.afphoto",
                "application/x-affinity-photo",
                "Affinity Photo document",
            ),
            (
                "sample.afdesign",
                "application/x-affinity-designer",
                "Affinity Designer document",
            ),
            (
                "sample.afpub",
                "application/x-affinity-publisher",
                "Affinity Publisher document",
            ),
        ];

        for (name, mime, format) in cases {
            let path = root.join(name);
            fs::write(&path, b"AFFINITY").expect("write sample");
            let info = probe_file(&path)
                .expect("probe should not fail")
                .expect("affinity file should be detected");
            assert_eq!(info.mime_types[0], mime);
            assert_eq!(info.format, format);
        }

        let _ = fs::remove_dir_all(root);
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

    fn id3v1_tag(
        title: &str,
        artist: &str,
        album: &str,
        year: &str,
        track: u8,
        genre: u8,
    ) -> [u8; 128] {
        fn put(dst: &mut [u8], value: &str) {
            let bytes = value.as_bytes();
            let n = bytes.len().min(dst.len());
            dst[..n].copy_from_slice(&bytes[..n]);
        }

        let mut tag = [0u8; 128];
        tag[0..3].copy_from_slice(b"TAG");
        put(&mut tag[3..33], title);
        put(&mut tag[33..63], artist);
        put(&mut tag[63..93], album);
        put(&mut tag[93..97], year);
        tag[125] = 0;
        tag[126] = track;
        tag[127] = genre;
        tag
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

        let adf_ofs_path = root.join("disk-ofs.adf");
        let mut adf_ofs = vec![0u8; 901_120];
        adf_ofs[..4].copy_from_slice(b"DOS\0");
        fs::write(&adf_ofs_path, adf_ofs).expect("write adf ofs");
        let adf_ofs_info = probe_file(&adf_ofs_path)
            .expect("probe adf ofs should not fail")
            .expect("adf ofs should be detected");
        assert_eq!(adf_ofs_info.mime_types[0], "application/x-amiga-adf-ofs");
        assert_eq!(adf_ofs_info.format, "Amiga Disk Format (OFS) image");

        let adf_ffs_path = root.join("disk-ffs.adf");
        let mut adf_ffs = vec![0u8; 901_120];
        adf_ffs[..4].copy_from_slice(b"DOS\x01");
        fs::write(&adf_ffs_path, adf_ffs).expect("write adf ffs");
        let adf_ffs_info = probe_file(&adf_ffs_path)
            .expect("probe adf ffs should not fail")
            .expect("adf ffs should be detected");
        assert_eq!(adf_ffs_info.mime_types[0], "application/x-amiga-adf-ffs");
        assert_eq!(adf_ffs_info.format, "Amiga Disk Format (FFS) image");

        let _ = fs::remove_dir_all(root);
    }
}
