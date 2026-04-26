use anyhow::Result;
use std::path::PathBuf;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Search backend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBackend {
    /// Recursive walkdir scan (cross-platform, respects start_dir strictly).
    Walk,
    /// macOS Spotlight via `mdfind` (fast, indexed, macOS only).
    Spotlight,
    /// POSIX `locate` command (fast, system-wide db, filters by start dir prefix).
    Locate,
}

impl SearchBackend {
    /// Returns `true` when Spotlight (`mdfind`) is available on this OS.
    pub fn spotlight_available() -> bool {
        cfg!(target_os = "macos")
            && std::process::Command::new("mdfind")
                .arg("-count")
                .arg("kMDItemFSName == '*'")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }

    /// Returns `true` when `locate` is on PATH and functional.
    pub fn locate_available() -> bool {
        std::process::Command::new("locate")
            .arg("-l")
            .arg("1")
            .arg("*")
            .output()
            .map(|_| true)
            .unwrap_or(false)
    }

    /// Returns the next available backend in cycle order: Walk → Spotlight → Locate → Walk.
    pub fn next_available(self) -> Self {
        let cycle = [
            SearchBackend::Walk,
            SearchBackend::Spotlight,
            SearchBackend::Locate,
        ];
        let pos = cycle.iter().position(|&b| b == self).unwrap_or(0);
        for i in 1..=cycle.len() {
            let next = cycle[(pos + i) % cycle.len()];
            if next == SearchBackend::Walk
                || (next == SearchBackend::Spotlight && Self::spotlight_available())
                || (next == SearchBackend::Locate && Self::locate_available())
            {
                return next;
            }
        }
        SearchBackend::Walk
    }

    /// Best default backend: Spotlight on macOS if available, else Locate if available, else Walk.
    pub fn best_default() -> Self {
        if Self::spotlight_available() {
            SearchBackend::Spotlight
        } else if Self::locate_available() {
            SearchBackend::Locate
        } else {
            SearchBackend::Walk
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SearchBackend::Walk => "WalkDir",
            SearchBackend::Spotlight => "Spotlight",
            SearchBackend::Locate => "Locate",
        }
    }
}

// ---------------------------------------------------------------------------
// Search query
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Glob pattern for filename (e.g. "*.rs").
    pub pattern: String,
    /// Optional substring to search inside file contents.
    pub content: Option<String>,
    /// Starting directory.
    pub start: PathBuf,
    /// Follow symlinks.
    pub follow_links: bool,
}

// ---------------------------------------------------------------------------
// Search result entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
}

// ---------------------------------------------------------------------------
// Synchronous search (runs to completion, reports via callback)
// ---------------------------------------------------------------------------

/// Search `query.start` recursively.
/// `on_result` is called for every match; return `false` to stop early.
pub fn search<F>(query: &SearchQuery, mut on_result: F) -> Result<()>
where
    F: FnMut(&SearchResult) -> bool,
{
    use crate::panel::glob_match;

    let pat = query.pattern.to_lowercase();
    let content_needle = query.content.as_deref().map(|s| s.as_bytes().to_vec());

    for entry in WalkDir::new(&query.start)
        .follow_links(query.follow_links)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_lowercase();

        if !glob_match(&pat, &file_name) {
            continue;
        }

        // Content search (optional)
        if let Some(ref needle) = content_needle {
            match std::fs::read(entry.path()) {
                Ok(bytes) => {
                    if !contains_bytes(&bytes, needle) {
                        continue;
                    }
                }
                Err(_) => continue, // skip unreadable files
            }
        }

        let meta = entry.metadata().ok();
        let result = SearchResult {
            path: entry.path().to_path_buf(),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
        };

        if !on_result(&result) {
            break;
        }
    }
    Ok(())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Spotlight search (macOS mdfind)
// ---------------------------------------------------------------------------

/// Run `mdfind` with the given query, restricted to `start_dir`.
/// Results are post-filtered by `content_needle` if provided (mdfind does
/// full-text search natively when `content` is in the query).
/// Returns up to `limit` results.
pub fn search_spotlight(query: &SearchQuery, limit: usize) -> Vec<SearchResult> {
    use crate::panel::glob_match;

    // Build mdfind query
    // Name pattern: strip leading/trailing * for a simpler kMDItemFSName contains match
    let name_core = query.pattern.trim_matches('*');
    let mdfind_query = if name_core.is_empty() {
        // wildcard only — find everything
        if let Some(ref content) = query.content {
            // use full-text search
            format!("\"{}\"", content.replace('"', " "))
        } else {
            "kMDItemFSName == '*'".into()
        }
    } else if let Some(ref content) = query.content {
        // name + content
        format!(
            "kMDItemFSName == '*{}*'cd && \"{}\"",
            name_core.replace('"', " "),
            content.replace('"', " ")
        )
    } else {
        format!("kMDItemFSName == '*{}*'cd", name_core.replace('"', " "))
    };

    let output = match std::process::Command::new("mdfind")
        .arg("-onlyin")
        .arg(&query.start)
        .arg(&mdfind_query)
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    if !output.status.success() {
        return vec![];
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let pat = query.pattern.to_lowercase();
    let content_needle = query.content.as_deref().map(|s| s.as_bytes().to_vec());

    text.lines()
        .filter_map(|line| {
            let path = std::path::Path::new(line.trim());
            if path.is_dir() {
                return None;
            }
            // Apply glob filter on the filename so that e.g. "*.rs" is respected
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if !glob_match(&pat, &file_name) {
                return None;
            }
            // Secondary content check when mdfind was called without it
            if let Some(ref needle) = content_needle {
                match std::fs::read(path) {
                    Ok(bytes) => {
                        if !contains_bytes(&bytes, needle) {
                            return None;
                        }
                    }
                    Err(_) => return None,
                }
            }
            let meta = std::fs::metadata(path).ok();
            Some(SearchResult {
                path: path.to_path_buf(),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
            })
        })
        .take(limit)
        .collect()
}

// ---------------------------------------------------------------------------
// Locate search (locate / mlocate / plocate)
// ---------------------------------------------------------------------------

/// Run `locate <pattern>`, post-filter by `start` prefix and content needle.
pub fn search_locate(query: &SearchQuery, limit: usize) -> Vec<SearchResult> {
    use crate::panel::glob_match;

    // locate pattern: wrap with * so partial names match
    let name_core = query.pattern.trim_matches('*');
    let locate_pat = if name_core.is_empty() {
        "*".to_string()
    } else {
        format!("*{name_core}*")
    };

    let output = match std::process::Command::new("locate")
        .arg("-i") // case-insensitive
        .arg(&locate_pat)
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let start_prefix = query.start.to_string_lossy().into_owned();
    let pat = query.pattern.to_lowercase();
    let content_needle = query.content.as_deref().map(|s| s.as_bytes().to_vec());

    text.lines()
        .filter_map(|line| {
            let path = std::path::Path::new(line.trim());
            // Restrict to start directory
            if !path.starts_with(&query.start) {
                return None;
            }
            if path.is_dir() || !path.exists() {
                return None;
            }
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if !glob_match(&pat, &file_name) {
                return None;
            }
            if let Some(ref needle) = content_needle {
                match std::fs::read(path) {
                    Ok(bytes) => {
                        if !contains_bytes(&bytes, needle) {
                            return None;
                        }
                    }
                    Err(_) => return None,
                }
            }
            let meta = std::fs::metadata(path).ok();
            Some(SearchResult {
                path: path.to_path_buf(),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
            })
        })
        .filter(|_| {
            let _ = &start_prefix;
            true
        }) // suppress lint
        .take(limit)
        .collect()
}
