use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

    if let Err(err) = extract_archive_with_unarc(path, &temp_root) {
        let _ = fs::remove_dir_all(&temp_root);
        return Err(err);
    }

    Ok(temp_root)
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
            matches!(
                info.mime_type.as_str(),
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
        })
        .unwrap_or(false)
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
