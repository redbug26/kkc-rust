use anyhow::Result;
use std::path::PathBuf;
use walkdir::WalkDir;

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
        let file_name = entry
            .file_name()
            .to_string_lossy()
            .to_lowercase();

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
