use std::ffi::OsStr;
use std::path::Path;

/// Broad file-category used for colour coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Directory,
    Executable,
    Archive,
    Audio,
    Image,
    Video,
    Document,
    Source,
    Text,
    Data,
    Unknown,
}

impl FileCategory {
    pub fn from_entry(is_dir: bool, is_symlink: bool, name: &str) -> Self {
        if is_dir {
            return FileCategory::Directory;
        }
        if is_symlink {
            return FileCategory::Unknown;
        }
        let path = Path::new(name);
        let ext = path
            .extension()
            .and_then(OsStr::to_str)
            .map(|s| s.to_ascii_lowercase());

        match ext.as_deref() {
            // --- Executables ---
            Some("exe" | "com" | "bat" | "sh" | "bin" | "app" | "run" | "msi" | "dmg") => {
                FileCategory::Executable
            }
            // also detect Unix executables by absence of extension (handled by caller)

            // --- Archives ---
            Some(
                "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "lha" | "lzh" | "arj"
                | "cab" | "iso" | "tgz" | "tbz2" | "txz" | "deb" | "rpm" | "pkg",
            ) => FileCategory::Archive,

            // --- Audio ---
            Some(
                "mp3" | "ogg" | "flac" | "wav" | "aiff" | "aif" | "m4a" | "wma" | "au" | "voc"
                | "mod" | "xm" | "it" | "s3m" | "ayt" | "mid" | "midi" | "opus" | "ape" | "mpc",
            ) => FileCategory::Audio,

            // --- Images ---
            Some(
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "svg" | "ico"
                | "pcx" | "pnm" | "ppm" | "pgm" | "pbm" | "tga" | "xcf" | "psd" | "avif",
            ) => FileCategory::Image,

            // --- Video ---
            Some(
                "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "mpeg" | "mpg" | "m4v"
                | "ogv" | "3gp" | "vob",
            ) => FileCategory::Video,

            // --- Documents ---
            Some(
                "pdf" | "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx"
                | "odp" | "epub" | "mobi",
            ) => FileCategory::Document,

            // --- Source code ---
            Some(
                "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "py" | "js" | "ts"
                | "java" | "cs" | "go" | "rb" | "php" | "swift" | "kt" | "asm" | "s" | "lua" | "pl"
                | "pm" | "ex" | "exs" | "hs" | "elm" | "clj" | "scala" | "dart" | "zig" | "nim"
                | "v" | "vhd" | "vhdl" | "verilog" | "sv",
            ) => FileCategory::Source,

            // --- Data / config ---
            Some(
                "toml" | "yaml" | "yml" | "json" | "json5" | "xml" | "csv" | "tsv" | "ini" | "cfg"
                | "conf" | "env" | "properties" | "lock" | "sql" | "db" | "sqlite",
            ) => FileCategory::Data,

            // --- Text ---
            Some(
                "txt" | "md" | "rst" | "log" | "readme" | "changelog" | "license" | "nfo" | "diz"
                | "ans" | "htm" | "html" | "css",
            ) => FileCategory::Text,

            _ => FileCategory::Unknown,
        }
    }
}
