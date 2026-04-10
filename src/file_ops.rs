use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Progress callback type
// ---------------------------------------------------------------------------

pub type ProgressFn<'a> = &'a mut dyn FnMut(u64, u64);

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------

/// Recursively copy `src` to `dst_dir / src.file_name()`.
/// `progress` is called with (bytes_done, total_bytes).
pub fn copy_entry(src: &Path, dst_dir: &Path, progress: Option<ProgressFn>) -> Result<()> {
    let name = src.file_name().context("source has no name")?;
    let dst = dst_dir.join(name);

    if src.is_dir() {
        copy_dir_recursive(src, &dst, progress)
    } else {
        copy_file(src, &dst, progress)
    }
}

fn copy_file(src: &Path, dst: &Path, _progress: Option<ProgressFn>) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)
        .with_context(|| format!("copy {} → {}", src.display(), dst.display()))?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path, _progress: Option<ProgressFn>) -> Result<()> {
    for entry in WalkDir::new(src).follow_links(false) {
        let entry = entry.with_context(|| format!("walking {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Move
// ---------------------------------------------------------------------------

/// Move `src` into `dst_dir`.  Tries `rename` first, falls back to copy+delete.
pub fn move_entry(src: &Path, dst_dir: &Path) -> Result<()> {
    let name = src.file_name().context("source has no name")?;
    let dst = dst_dir.join(name);

    if dst.exists() {
        bail!("Destination already exists: {}", dst.display());
    }

    // Try atomic rename first (works within the same filesystem)
    if fs::rename(src, &dst).is_ok() {
        return Ok(());
    }

    // Cross-device: copy then delete
    copy_entry(src, dst_dir, None)?;
    delete_entry(src)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

pub fn delete_entry(path: &Path) -> Result<()> {
    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path)
            .with_context(|| format!("remove_dir_all: {}", path.display()))
    } else {
        fs::remove_file(path)
            .with_context(|| format!("remove_file: {}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

pub fn rename_entry(src: &Path, new_name: &str) -> Result<PathBuf> {
    if new_name.is_empty() {
        bail!("New name is empty");
    }
    if new_name.contains('/') || new_name.contains('\0') {
        bail!("Illegal characters in name");
    }
    let parent = src.parent().context("source has no parent dir")?;
    let dst = parent.join(new_name);
    if dst.exists() {
        bail!("A file named '{}' already exists", new_name);
    }
    fs::rename(src, &dst)
        .with_context(|| format!("rename {} → {}", src.display(), dst.display()))?;
    Ok(dst)
}

// ---------------------------------------------------------------------------
// Mkdir
// ---------------------------------------------------------------------------

pub fn make_dir(parent: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() {
        bail!("Directory name is empty");
    }
    if name.contains('/') || name.contains('\0') {
        bail!("Illegal characters in directory name");
    }
    let path = parent.join(name);
    fs::create_dir_all(&path)
        .with_context(|| format!("mkdir {}", path.display()))?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Compute directory size
// ---------------------------------------------------------------------------
#[allow(dead_code)]pub fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

// ---------------------------------------------------------------------------
// Format helpers
// ---------------------------------------------------------------------------

/// Human-readable size (e.g. "1.4 MB").
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1_024;
    const MB: u64 = KB * 1_024;
    const GB: u64 = MB * 1_024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
