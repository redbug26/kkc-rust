use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

static ARCHIVE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn supports_archive_navigation(path: &Path) -> bool {
    is_builtin_archive(path) || crate::plugins::supports_archive_navigation(path)
}

pub fn extract_archive_to_temp(path: &Path) -> Result<PathBuf> {
    if !supports_archive_navigation(path) {
        bail!("Archive format not supported for internal navigation");
    }

    let temp_root = make_temp_dir(path)?;
    if crate::plugins::extract_archive_to_temp(path, &temp_root)? {
        return Ok(temp_root);
    }

    let result = if is_builtin_zip_container(path) {
        extract_archive_with_zip(path, &temp_root)
    } else {
        extract_archive_with_unarc(path, &temp_root)
    };

    if let Err(err) = result {
        let _ = fs::remove_dir_all(&temp_root);
        return Err(err);
    }

    Ok(temp_root)
}

fn extract_archive_with_zip(path: &Path, temp_root: &Path) -> Result<()> {
    let file =
        File::open(path).with_context(|| format!("Opening ZIP archive {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("Reading ZIP archive {}", path.display()))?;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .with_context(|| format!("Reading ZIP entry #{} from {}", idx, path.display()))?;
        let name = entry.name().to_string();
        let rel_path = sanitize_archive_entry_path(&name).ok_or_else(|| {
            anyhow::anyhow!("Unsafe path in archive {}: {}", path.display(), name)
        })?;

        if rel_path.as_os_str().is_empty() {
            continue;
        }

        let out_path = temp_root.join(&rel_path);
        if entry.is_dir() || name.ends_with('/') || name.ends_with('\\') {
            ensure_directory_path(&out_path)
                .with_context(|| format!("Creating directory {}", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            ensure_directory_path(parent)
                .with_context(|| format!("Creating parent directory {}", parent.display()))?;
        }

        if out_path.is_dir() {
            fs::remove_dir_all(&out_path).with_context(|| {
                format!("Removing conflicting directory {}", out_path.display())
            })?;
        }

        let mut out_file = File::create(&out_path)
            .with_context(|| format!("Creating extracted file {}", out_path.display()))?;
        io::copy(&mut entry, &mut out_file).with_context(|| format!("Extracting {}", name))?;
    }

    Ok(())
}

fn extract_archive_with_unarc(path: &Path, temp_root: &Path) -> Result<()> {
    let mut archive = unarc_rs::unified::ArchiveFormat::open_path(path)
        .with_context(|| format!("Opening archive {} with unarc-rs", path.display()))?;

    while let Some(entry) = archive
        .next_entry()
        .with_context(|| format!("Reading entries from {}", path.display()))?
    {
        let rel_path = sanitize_archive_entry_path(entry.name()).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsafe path in archive {}: {}",
                path.display(),
                entry.name()
            )
        })?;

        if rel_path.as_os_str().is_empty() {
            archive
                .skip(&entry)
                .with_context(|| format!("Skipping empty entry in {}", path.display()))?;
            continue;
        }

        let out_path = temp_root.join(&rel_path);
        if entry.name().ends_with('/') || entry.name().ends_with('\\') {
            ensure_directory_path(&out_path)
                .with_context(|| format!("Creating directory {}", out_path.display()))?;
            archive
                .skip(&entry)
                .with_context(|| format!("Skipping directory entry {}", entry.name()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            ensure_directory_path(parent)
                .with_context(|| format!("Creating parent directory {}", parent.display()))?;
        }

        if out_path.is_dir() {
            fs::remove_dir_all(&out_path).with_context(|| {
                format!("Removing conflicting directory {}", out_path.display())
            })?;
        }

        let data = archive
            .read(&entry)
            .with_context(|| format!("Extracting {}", entry.name()))?;
        fs::write(&out_path, &data)
            .with_context(|| format!("Writing extracted file {}", out_path.display()))?;
    }

    Ok(())
}

fn ensure_directory_path(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if current.exists() {
            let meta = fs::metadata(&current)
                .with_context(|| format!("Reading metadata for {}", current.display()))?;
            if meta.is_file() {
                fs::remove_file(&current)
                    .with_context(|| format!("Removing conflicting file {}", current.display()))?;
                fs::create_dir(&current)
                    .with_context(|| format!("Creating directory {}", current.display()))?;
            }
        } else {
            fs::create_dir(&current)
                .with_context(|| format!("Creating directory {}", current.display()))?;
        }
    }
    Ok(())
}

fn sanitize_archive_entry_path(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

fn is_builtin_archive(path: &Path) -> bool {
    crate::idf::probe_path(path)
        .map(|info| {
            info.mime_types
                .iter()
                .any(|mime_type| is_builtin_archive_mime_type(mime_type))
        })
        .unwrap_or(false)
}

fn is_builtin_zip_container(path: &Path) -> bool {
    crate::idf::probe_path(path)
        .map(|info| info.mime_types.iter().any(|mime| mime == "application/zip"))
        .unwrap_or(false)
}

fn is_builtin_archive_mime_type(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "application/zip"
            | "application/vnd.rar"
            | "application/x-arj"
            | "application/x-ace-compressed"
            | "application/x-arc"
            | "application/x-zoo"
            | "application/x-sq"
            | "application/x-sqz"
            | "application/x-ha"
            | "application/x-hyp"
            | "application/x-uc2"
            | "application/x-unix-compress"
            | "application/x-ice-compressed"
            | "application/x-packice"
            | "application/x-tar"
            | "application/x-compressed-tar"
            | "application/x-bzip-compressed-tar"
            | "application/x-tarz"
            | "application/gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/zstd"
            | "application/x-7z-compressed"
            | "application/x-lzh-compressed"
    )
}

fn make_temp_dir(path: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = ARCHIVE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("archive");
    let dir = std::env::temp_dir().join(format!(
        "kkc-archive-{}-{}-{}",
        base,
        std::process::id(),
        stamp + seq as u128
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unique_path(ext: &str) -> PathBuf {
        let seq = ARCHIVE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kkc-archive-test-{}-{}.{}",
            std::process::id(),
            seq,
            ext
        ))
    }

    fn create_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create zip container");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(contents).expect("write zip entry");
        }
        zip.finish().expect("finish zip container");
    }

    #[test]
    fn xlsx_can_be_entered_as_archive() {
        let path = unique_path("xlsx");
        create_zip(
            &path,
            &[
                ("[Content_Types].xml", b"<Types></Types>"),
                ("xl/workbook.xml", b"<workbook/>"),
                ("xl/worksheets/sheet1.xml", b"<worksheet/>"),
            ],
        );

        assert!(supports_archive_navigation(&path));
        let temp_root = extract_archive_to_temp(&path).expect("extract xlsx");
        assert!(temp_root.join("xl/workbook.xml").is_file());
        assert!(temp_root.join("xl/worksheets/sheet1.xml").is_file());

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn epub_can_be_entered_as_archive() {
        let path = unique_path("epub");
        create_zip(
            &path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", b"<container/>"),
                ("OEBPS/content.opf", b"<package/>"),
            ],
        );

        assert!(supports_archive_navigation(&path));
        let temp_root = extract_archive_to_temp(&path).expect("extract epub");
        assert!(temp_root.join("META-INF/container.xml").is_file());
        assert!(temp_root.join("OEBPS/content.opf").is_file());

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(temp_root);
    }
}
