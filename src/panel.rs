use crate::archive;
use crate::config::SortMode;
use crate::file_types::FileCategory;
use crate::remote::{
    RemoteEntry, RemoteProfile, display_path as remote_display_path, list_dir,
    normalize_remote_cwd, resolve_initial_dir,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// DirEntry wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<DateTime<Local>>,
    pub category: FileCategory,
    pub selected: bool,
    /// Unix permission bits (0o755 etc.)
    pub mode: u32,
    pub cloud_only: bool,
    pub file_icon: Option<&'static str>,
}

impl Entry {
    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata =
            fs::symlink_metadata(path).with_context(|| format!("stat: {}", path.display()))?;

        let is_symlink = metadata.file_type().is_symlink();
        // For directories/size resolve through the symlink
        let resolved = if is_symlink {
            fs::metadata(path).ok()
        } else {
            None
        };
        let real_meta = resolved.as_ref().unwrap_or(&metadata);

        let is_dir = real_meta.is_dir();
        let size = if is_dir { 0 } else { real_meta.len() };
        let modified = real_meta
            .modified()
            .ok()
            .map(|t| DateTime::<Local>::from(t));
        let mode = metadata.permissions().mode();
        let cloud_only = crate::cloud_status::is_cloud_only(path, &metadata);

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let category = FileCategory::from_entry(is_dir, is_symlink, &name);
        let file_icon = if cloud_only {
            None
        } else {
            crate::file_icons::icon_for_entry(&name, is_dir)
        };

        Ok(Self {
            name,
            path: path.to_path_buf(),
            is_dir,
            is_symlink,
            size,
            modified,
            category,
            selected: false,
            mode,
            cloud_only,
            file_icon,
        })
    }

    /// A single-char type indicator, similar to `ls -l`.
    #[allow(dead_code)]
    pub fn type_char(&self) -> char {
        if self.is_symlink {
            'l'
        } else if self.is_dir {
            'd'
        } else {
            '-'
        }
    }
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    /// Index of the highlighted (cursor) entry.
    pub cursor: usize,
    /// Vertical scroll offset.
    pub scroll: usize,
    pub sort: SortMode,
    pub show_hidden: bool,
    /// Incremental name filter typed by the user.
    pub quicksearch: String,
    /// Which match inside the filtered list is highlighted (0-based).
    pub qs_match_pos: usize,
    archive: Option<ArchiveMount>,
    remote: Option<RemoteMount>,
    fallback_local_path: PathBuf,
}

#[derive(Debug)]
struct ArchiveMount {
    archive_path: PathBuf,
    temp_root: PathBuf,
}

#[derive(Debug, Clone)]
struct RemoteMount {
    profile: RemoteProfile,
    cwd: String,
}

impl Panel {
    fn prepare_remote_entries(
        &self,
        profile: &RemoteProfile,
        cwd: &str,
        remote_entries: Vec<RemoteEntry>,
    ) -> Vec<Entry> {
        let mut entries: Vec<Entry> = remote_entries
            .into_iter()
            .map(|e| self.entry_from_remote(cwd, e))
            .collect();
        let parent_entry = if cwd == "/" {
            None
        } else {
            Some(Entry {
                name: "..".into(),
                path: Path::new(cwd)
                    .parent()
                    .unwrap_or(Path::new("/"))
                    .to_path_buf(),
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
                category: FileCategory::Directory,
                selected: false,
                mode: 0o755,
                cloud_only: false,
                file_icon: crate::file_icons::icon_for_entry("..", true),
            })
        };
        self.sort_entries(&mut entries);
        entries.insert(0, self.remote_disconnect_entry());
        if let Some(pe) = parent_entry {
            entries.insert(1, pe);
        }
        let _ = profile;
        entries
    }

    pub fn new(path: PathBuf, sort: SortMode, show_hidden: bool) -> Self {
        let mut p = Self {
            path: path.clone(),
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            sort,
            show_hidden,
            quicksearch: String::new(),
            qs_match_pos: 0,
            archive: None,
            remote: None,
            fallback_local_path: path,
        };
        let _ = p.reload();
        p
    }

    /// Reload directory listing from disk.
    pub fn reload(&mut self) -> Result<()> {
        if self.remote.is_some() {
            return self.reload_remote();
        }
        let rd = fs::read_dir(&self.path)
            .with_context(|| format!("Reading dir: {}", self.path.display()))?;

        let mut entries: Vec<Entry> = rd
            .filter_map(|res| res.ok())
            .filter_map(|de| Entry::from_path(&de.path()).ok())
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .collect();

        // Always keep ".." at the top
        let parent_entry = self.parent_entry();

        self.sort_entries(&mut entries);

        // Prepend ".."
        if let Some(pe) = parent_entry {
            entries.insert(0, pe);
        }

        // Preserve cursor on the same filename
        let old_name = self.entries.get(self.cursor).map(|e| e.name.clone());
        self.entries = entries;

        if let Some(name) = old_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
            } else {
                self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
            }
        } else {
            self.cursor = 0;
        }

        Ok(())
    }

    fn reload_remote(&mut self) -> Result<()> {
        let mount = self
            .remote
            .as_ref()
            .context("Missing remote mount")?
            .clone();
        let mut entries: Vec<Entry> = list_dir(&mount.profile, &mount.cwd, self.show_hidden)?
            .into_iter()
            .map(|e| self.entry_from_remote(&mount.cwd, e))
            .collect();

        let parent_entry = self.parent_entry();
        self.sort_entries(&mut entries);
        entries.insert(0, self.remote_disconnect_entry());
        if let Some(pe) = parent_entry {
            entries.insert(1, pe);
        }

        let old_name = self.entries.get(self.cursor).map(|e| e.name.clone());
        self.entries = entries;
        if let Some(name) = old_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
            } else {
                self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
            }
        } else {
            self.cursor = 0;
        }
        Ok(())
    }

    fn parent_entry(&self) -> Option<Entry> {
        let parent = if self.is_remote_view() {
            let mount = self.remote.as_ref()?;
            let cwd = &mount.cwd;
            if cwd == "/" {
                return None;
            }
            let parent = Path::new(cwd).parent().unwrap_or(Path::new("/"));
            PathBuf::from(normalize_remote_cwd(
                &mount.profile,
                &parent.to_string_lossy(),
            ))
        } else if self.is_archive_root() {
            self.archive.as_ref()?.archive_path.parent()?.to_path_buf()
        } else {
            self.path.parent()?.to_path_buf()
        };
        Some(Entry {
            name: "..".into(),
            path: parent,
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
            category: FileCategory::Directory,
            selected: false,
            mode: 0o755,
            cloud_only: false,
            file_icon: crate::file_icons::icon_for_entry("..", true),
        })
    }

    fn remote_disconnect_entry(&self) -> Entry {
        Entry {
            name: "[disconnect]".into(),
            path: self.fallback_local_path.clone(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
            category: FileCategory::Directory,
            selected: false,
            mode: 0o755,
            cloud_only: false,
            file_icon: crate::file_icons::icon_for_entry("..", true),
        }
    }

    fn sort_entries(&self, entries: &mut Vec<Entry>) {
        match self.sort {
            SortMode::Name => {
                entries.sort_by(|a, b| {
                    // directories first
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            SortMode::Extension => {
                entries.sort_by(|a, b| {
                    let ext_a = ext_of(&a.name);
                    let ext_b = ext_of(&b.name);
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then(ext_a.cmp(&ext_b))
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            SortMode::Date => {
                entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(b.modified.cmp(&a.modified)));
            }
            SortMode::Size => {
                entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(b.size.cmp(&a.size)));
            }
            SortMode::Unsorted => {}
        }
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    pub fn move_page_up(&mut self, page_size: usize) {
        self.cursor = self.cursor.saturating_sub(page_size);
    }

    pub fn move_page_down(&mut self, page_size: usize) {
        let max = self.entries.len().saturating_sub(1);
        self.cursor = (self.cursor + page_size).min(max);
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
    }

    /// Update scroll so cursor stays visible in a window of `height` rows.
    pub fn clamp_scroll(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + height {
            self.scroll = self.cursor - height + 1;
        }
    }

    // -----------------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------------

    pub fn toggle_selected(&mut self) {
        if let Some(e) = self.entries.get_mut(self.cursor) {
            if e.name != ".." {
                e.selected = !e.selected;
            }
        }
    }

    #[allow(dead_code)]
    pub fn select_all(&mut self) {
        for e in &mut self.entries {
            if e.name != ".." {
                e.selected = true;
            }
        }
    }

    #[allow(dead_code)]
    pub fn deselect_all(&mut self) {
        for e in &mut self.entries {
            e.selected = false;
        }
    }

    pub fn invert_selection(&mut self) {
        for e in &mut self.entries {
            if e.name != ".." {
                e.selected = !e.selected;
            }
        }
    }

    /// Select entries whose name matches `pattern` (glob-style `*` and `?`).
    pub fn select_pattern(&mut self, pattern: &str, value: bool) {
        let pat = pattern.to_lowercase();
        for e in &mut self.entries {
            if e.name == ".." {
                continue;
            }
            if glob_match(&pat, &e.name.to_lowercase()) {
                e.selected = value;
            }
        }
    }

    /// Returns selected entries, or the current entry if none are selected.
    pub fn effective_selection(&self) -> Vec<&Entry> {
        let selected: Vec<&Entry> = self.entries.iter().filter(|e| e.selected).collect();
        if selected.is_empty() {
            if let Some(e) = self.entries.get(self.cursor) {
                if e.name != ".." {
                    return vec![e];
                }
            }
            vec![]
        } else {
            selected
        }
    }

    // -----------------------------------------------------------------------
    // Navigation into directories
    // -----------------------------------------------------------------------

    pub fn enter_dir(&mut self, path: PathBuf) -> Result<()> {
        crate::viewer::debug_log(&format!("[Panel::enter_dir] path={}, is_remote={}", path.display(), self.remote.is_some()));
        if self.remote.is_some() {
            if let Some(mount) = self.remote.as_mut() {
                let path_str = path.to_string_lossy();
                crate::viewer::debug_log(&format!("[Panel::enter_dir] remote: path_str={}, cwd_before={}", path_str, mount.cwd));
                mount.cwd = normalize_remote_cwd(&mount.profile, &path_str);
                crate::viewer::debug_log(&format!("[Panel::enter_dir] remote: cwd_after={}", mount.cwd));
                self.path = PathBuf::from(&mount.cwd);
            } else {
                self.path = path;
            }
            self.cursor = 0;
            self.scroll = 0;
            self.quicksearch.clear();
            self.entries.clear();
            return self.reload();
        }
        if self.archive.is_some() && !self.is_inside_archive_temp(&path) {
            self.clear_archive_mount();
        }
        self.path = path;
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        // Changing directory is a fresh listing; do not preserve a filename
        // from the previous directory when reloading.
        self.entries.clear();
        self.reload()
    }

    pub fn enter_archive(&mut self, archive_path: PathBuf) -> Result<()> {
        self.clear_archive_mount();
        self.clear_remote_mount();
        let temp_root = archive::extract_archive_to_temp(&archive_path)?;
        self.archive = Some(ArchiveMount {
            archive_path,
            temp_root: temp_root.clone(),
        });
        self.path = temp_root;
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        self.entries.clear();
        self.reload()
    }

    pub fn enter_remote(&mut self, profile: RemoteProfile) -> Result<()> {
        self.clear_archive_mount();
        self.clear_remote_mount();
        self.fallback_local_path = self.persisted_path();
        let cwd = resolve_initial_dir(&profile)?;
        self.remote = Some(RemoteMount {
            profile,
            cwd: cwd.clone(),
        });
        self.path = PathBuf::from(&cwd);
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        self.entries.clear();
        self.reload()
    }

    pub fn mount_remote_prefetched(
        &mut self,
        profile: RemoteProfile,
        cwd: String,
        remote_entries: Vec<RemoteEntry>,
    ) {
        self.clear_archive_mount();
        self.clear_remote_mount();
        self.fallback_local_path = self.persisted_path();
        let entries = self.prepare_remote_entries(&profile, &cwd, remote_entries);
        self.remote = Some(RemoteMount {
            profile,
            cwd: cwd.clone(),
        });
        self.path = PathBuf::from(&cwd);
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        self.entries = entries;
    }

    pub fn leave_archive(&mut self) -> Option<(PathBuf, String)> {
        let mount = self.archive.take()?;
        let parent = mount.archive_path.parent()?.to_path_buf();
        let archive_name = mount
            .archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.path = parent.clone();
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        self.entries.clear();
        let _ = self.reload();
        let _ = fs::remove_dir_all(mount.temp_root);
        Some((parent, archive_name))
    }

    pub fn display_path(&self) -> String {
        if let Some(mount) = &self.archive {
            let rel = self
                .path
                .strip_prefix(&mount.temp_root)
                .unwrap_or(Path::new(""));
            if rel.as_os_str().is_empty() {
                mount.archive_path.to_string_lossy().into_owned()
            } else {
                format!(
                    "{}{}",
                    mount.archive_path.display(),
                    format!("/{}", rel.display())
                )
            }
        } else if let Some(remote) = &self.remote {
            remote_display_path(&remote.profile, &remote.cwd)
        } else {
            self.path.to_string_lossy().into_owned()
        }
    }

    pub fn persisted_path(&self) -> PathBuf {
        if let Some(mount) = &self.archive {
            mount
                .archive_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.path.clone())
        } else if self.remote.is_some() {
            self.fallback_local_path.clone()
        } else {
            self.path.clone()
        }
    }

    pub fn is_archive_view(&self) -> bool {
        self.archive.is_some()
    }

    pub fn archive_path(&self) -> Option<&Path> {
        self.archive
            .as_ref()
            .map(|mount| mount.archive_path.as_path())
    }

    pub fn is_remote_view(&self) -> bool {
        self.remote.is_some()
    }

    pub fn remote_profile(&self) -> Option<RemoteProfile> {
        self.remote.as_ref().map(|m| m.profile.clone())
    }

    pub fn remote_cwd(&self) -> Option<&str> {
        self.remote.as_ref().map(|m| m.cwd.as_str())
    }

    pub fn is_archive_root(&self) -> bool {
        self.archive
            .as_ref()
            .map(|mount| self.path == mount.temp_root)
            .unwrap_or(false)
    }

    fn is_inside_archive_temp(&self, path: &Path) -> bool {
        self.archive
            .as_ref()
            .map(|mount| path.starts_with(&mount.temp_root))
            .unwrap_or(false)
    }

    fn clear_archive_mount(&mut self) {
        if let Some(mount) = self.archive.take() {
            let _ = fs::remove_dir_all(mount.temp_root);
        }
    }

    fn clear_remote_mount(&mut self) {
        self.remote = None;
    }

    pub fn disconnect(&mut self) {
        self.clear_remote_mount();
        self.path = self.fallback_local_path.clone();
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        self.entries.clear();
        let _ = self.reload();
    }

    // -----------------------------------------------------------------------
    // Quicksearch
    // -----------------------------------------------------------------------

    pub fn quicksearch_append(&mut self, ch: char) {
        self.quicksearch.push(ch);
    }

    pub fn quicksearch_pop(&mut self) {
        self.quicksearch.pop();
    }

    pub fn quicksearch_clear(&mut self) {
        self.quicksearch.clear();
    }

    /// Returns sorted indices of entries whose names contain ALL whitespace-separated
    /// tokens of `quicksearch` (case-insensitive, AND logic).
    /// Entries where the *first* token starts the name come first.
    pub fn quicksearch_matches(&self) -> Vec<usize> {
        if self.quicksearch.is_empty() {
            return vec![];
        }
        let tokens: Vec<String> = self
            .quicksearch
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .collect();
        if tokens.is_empty() {
            return vec![];
        }
        let first = &tokens[0];
        let rest = &tokens[1..];

        let matches_all = |name: &str| -> bool {
            let low = name.to_lowercase();
            rest.iter().all(|t| low.contains(t.as_str()))
        };

        // Priority 1: first token is a prefix of the name
        let mut starts: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let low = e.name.to_lowercase();
                low.starts_with(first.as_str()) && matches_all(&e.name)
            })
            .map(|(i, _)| i)
            .collect();

        // Priority 2: first token appears anywhere else
        let mut contains: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let low = e.name.to_lowercase();
                low.contains(first.as_str())
                    && !low.starts_with(first.as_str())
                    && matches_all(&e.name)
            })
            .map(|(i, _)| i)
            .collect();

        starts.append(&mut contains);
        starts
    }

    /// Returns the index of the first entry whose name starts with `quicksearch`.
    #[allow(dead_code)]
    pub fn quicksearch_index(&self) -> Option<usize> {
        self.quicksearch_matches().into_iter().next()
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub fn selected_count(&self) -> usize {
        self.entries.iter().filter(|e| e.selected).count()
    }

    pub fn selected_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.size)
            .sum()
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn restore_cursor_by_name(&mut self, name: &str) {
        if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
            self.cursor = idx;
        }
    }

    pub fn restore_selection_by_names(&mut self, names: &[String]) {
        if names.is_empty() {
            self.deselect_all();
            return;
        }
        let wanted: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
        for e in &mut self.entries {
            e.selected = e.name != ".." && wanted.contains(e.name.as_str());
        }
    }

    pub fn find_file_id_path(&self) -> Option<PathBuf> {
        if self.is_remote_view() {
            return None;
        }
        // Only look for file_id.diz when the cursor is on a directory entry;
        // for regular files we want the IDF card, not the folder's file_id.diz.
        let entry = self.current_entry()?;
        if !entry.is_dir {
            return None;
        }

        let mut bases = Vec::new();
        if entry.name != ".." {
            bases.push(entry.path.clone());
        }
        bases.push(self.path.clone());

        for base in bases {
            if let Some(found) = find_file_id_in_dir(&base) {
                return Some(found);
            }
        }
        None
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.clear_archive_mount();
        self.clear_remote_mount();
    }
}

impl Panel {
    fn entry_from_remote(&self, _cwd: &str, entry: RemoteEntry) -> Entry {
        let path = PathBuf::from(&entry.path);
        crate::viewer::debug_log(&format!("[entry_from_remote] name={}, path={}", entry.name, entry.path));
        let category = FileCategory::from_entry(entry.is_dir, entry.is_symlink, &entry.name);
        Entry {
            name: entry.name,
            path,
            is_dir: entry.is_dir,
            is_symlink: entry.is_symlink,
            size: if entry.is_dir { 0 } else { entry.size },
            modified: entry.modified,
            category,
            selected: false,
            mode: entry.mode,
            cloud_only: false,
            file_icon: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn find_file_id_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in [
        "FILE_ID.DIZ",
        "file_id.diz",
        "File_id.diz",
        "FILE_ID.ANS",
        "file_id.ans",
    ] {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    let rd = fs::read_dir(dir).ok()?;
    rd.filter_map(|res| res.ok()).find_map(|entry| {
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_ascii_lowercase();
        if lower == "file_id.diz" || lower == "file_id.ans" {
            Some(entry.path())
        } else {
            None
        }
    })
}

/// Very simple glob: `*` matches anything, `?` matches one char.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t, 0, 0)
}

fn glob_rec(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    match p[pi] {
        '*' => {
            // skip consecutive stars
            let mut next = pi + 1;
            while next < p.len() && p[next] == '*' {
                next += 1;
            }
            if next == p.len() {
                return true;
            }
            for i in ti..=t.len() {
                if glob_rec(p, t, next, i) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < t.len() {
                glob_rec(p, t, pi + 1, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < t.len() && t[ti] == c {
                glob_rec(p, t, pi + 1, ti + 1)
            } else {
                false
            }
        }
    }
}
