use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ARCHIVE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn supports_archive_navigation(path: &Path) -> bool {
    let builtin = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "zip" | "rar"))
        .unwrap_or(false);

    builtin || crate::plugins::supports_archive_navigation(path)
}

pub fn extract_archive_to_temp(path: &Path) -> Result<PathBuf> {
    if !supports_archive_navigation(path) {
        bail!("Archive format not supported for internal navigation");
    }

    let temp_root = make_temp_dir(path)?;
    if crate::plugins::extract_archive_to_temp(path, &temp_root)? {
        return Ok(temp_root);
    }

    let status = Command::new("bsdtar")
        .arg("-xf")
        .arg(path)
        .arg("-C")
        .arg(&temp_root)
        .status()
        .with_context(|| format!("Running bsdtar on {}", path.display()))?;

    if !status.success() {
        let _ = fs::remove_dir_all(&temp_root);
        bail!("bsdtar could not open {}", path.display());
    }

    Ok(temp_root)
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
