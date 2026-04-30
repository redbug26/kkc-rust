use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::convert::TryFrom;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    #[serde(skip, default)]
    pub search_key: String,
}

#[derive(Debug, Clone)]
pub struct TreeProgressLevel {
    pub depth: usize,
    pub ratio: f64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TreeCache {
    root: PathBuf,
    scanned_at: String,
    entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactTreeCache {
    root: PathBuf,
    scanned_at: String,
    entries: Vec<CompactTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactTreeEntry(String, bool);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactTreeNode {
    parent: Option<u32>,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CompactTreeCacheV2 {
    root: PathBuf,
    scanned_at: String,
    entries: Vec<CompactTreeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CompactTreeCacheAny {
    V3(CompactTreeCache),
    V2(CompactTreeCacheV2),
}

#[derive(Debug)]
pub enum TreeScanMessage {
    Progress {
        visited: usize,
        progress: f64,
        levels: Vec<TreeProgressLevel>,
        current: PathBuf,
    },
    Finished {
        entries: Vec<TreeEntry>,
        scanned_at: String,
    },
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
pub struct TreeScanTask {
    pub rx: Receiver<TreeScanMessage>,
    pub cancel: Arc<AtomicBool>,
}

/// An item in the display list. `Context` entries are ancestor dirs shown
/// dimmed for tree context; `Match` entries are the actual filter results
/// and the only ones that are navigable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayItem {
    Context(usize),
    Match(usize),
}

#[derive(Debug)]
pub struct TreeViewState {
    pub root: PathBuf,
    pub query: String,
    pub filtered: Vec<usize>,
    pub display: Vec<DisplayItem>,
    /// For each display item, pipe-continuation flags for visible ancestor columns
    /// (depths 1..entry.depth-1). Root depth (0) is intentionally skipped to keep
    /// connector alignment compact in the popup.
    pub display_prefixes: Vec<Vec<bool>>,
    /// For each display item, whether it is the last sibling at its depth in the display list.
    pub display_is_last: Vec<bool>,
    pub match_pos: usize,
    pub scroll: usize,
    pub entries: Vec<TreeEntry>,
    pub scanned_at: Option<String>,
    pub scanning: bool,
    pub visited: usize,
    pub progress: f64,
    pub progress_levels: Vec<TreeProgressLevel>,
    pub current: Option<PathBuf>,
    pub scan_rx: Option<Receiver<TreeScanMessage>>,
    pub cancel_flag: Option<Arc<AtomicBool>>,
}

impl TreeViewState {
    pub fn load_or_scan(root: PathBuf) -> Self {
        let mut state = Self::empty(root);
        if let Ok((entries, scanned_at)) = load_cache(&state.root) {
            state.set_entries(entries, Some(scanned_at));
        } else {
            state.start_scan();
        }
        state
    }

    pub fn empty(root: PathBuf) -> Self {
        Self {
            root,
            query: String::new(),
            filtered: Vec::new(),
            display: Vec::new(),
            display_prefixes: Vec::new(),
            display_is_last: Vec::new(),
            match_pos: 0,
            scroll: 0,
            entries: Vec::new(),
            scanned_at: None,
            scanning: false,
            visited: 0,
            progress: 0.0,
            progress_levels: Vec::new(),
            current: None,
            scan_rx: None,
            cancel_flag: None,
        }
    }

    pub fn start_scan(&mut self) {
        self.cancel_scan();
        let task = spawn_tree_scan(self.root.clone());
        self.scanning = true;
        self.visited = 0;
        self.progress = 0.0;
        self.progress_levels = vec![TreeProgressLevel {
            depth: 0,
            ratio: 0.0,
            path: self.root.clone(),
        }];
        self.current = Some(self.root.clone());
        self.scan_rx = Some(task.rx);
        self.cancel_flag = Some(task.cancel);
    }

    pub fn cancel_scan(&mut self) {
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        self.scanning = false;
        self.scan_rx = None;
        self.cancel_flag = None;
    }

    pub fn rebuild_filter(&mut self) {
        let query = self.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
            self.display = self.filtered.iter().map(|&i| DisplayItem::Match(i)).collect();
        } else {
            let tokens: Vec<&str> = query.split_whitespace().collect();
            let n = self.entries.len();
            let mut matched = vec![false; n];
            let mut is_ancestor = vec![false; n];

            // Filter: all tokens appear in full path AND at least one token appears in the
            // entry's own basename — this prevents ancestors from appearing as matches.
            self.filtered = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    if !tokens.iter().all(|t| entry.search_key.contains(*t)) {
                        return false;
                    }
                    let name_lc = entry.name.to_ascii_lowercase();
                    tokens.iter().any(|t| name_lc.contains(*t))
                })
                .map(|(idx, _)| {
                    matched[idx] = true;
                    idx
                })
                .collect();

            // Build ancestor context set via depth-stack (O(n), no HashSet).
            // depth_stack[d] = index of the most recent entry seen at depth d.
            let mut depth_stack: Vec<usize> = Vec::with_capacity(32);
            for (idx, entry) in self.entries.iter().enumerate() {
                // Truncate to keep only true ancestors (depths 0..entry.depth-1).
                depth_stack.truncate(entry.depth);
                if matched[idx] {
                    for &anc_idx in &depth_stack {
                        is_ancestor[anc_idx] = true;
                    }
                }
                // Push self at depth; depth_stack[entry.depth] = idx.
                if depth_stack.len() == entry.depth {
                    depth_stack.push(idx);
                } else {
                    depth_stack[entry.depth] = idx;
                }
            }

            self.display = self
                .entries
                .iter()
                .enumerate()
                .filter(|(idx, _)| matched[*idx] || is_ancestor[*idx])
                .map(|(idx, _)| {
                    if matched[idx] {
                        DisplayItem::Match(idx)
                    } else {
                        DisplayItem::Context(idx)
                    }
                })
                .collect();
        }
        self.compute_display_connectors();
        self.sync_match_pos();
    }

    /// Pre-compute tree connector data for the current `display` list so the renderer can
    /// draw proper ├─ / └─ / │ connectors without per-frame work.
    fn compute_display_connectors(&mut self) {
        let n = self.display.len();
        if n == 0 {
            self.display_prefixes = Vec::new();
            self.display_is_last = Vec::new();
            return;
        }

        let depths: Vec<usize> = self
            .display
            .iter()
            .map(|item| {
                let idx = match item {
                    DisplayItem::Match(i) | DisplayItem::Context(i) => *i,
                };
                self.entries[idx].depth
            })
            .collect();

        // ── Step 1: is_last via monotone stack (forward, O(n)) ──────────────────
        //
        // is_last[i] = true iff there is no sibling j > i at the same depth before
        // any ancestor closes the block (i.e. the next item with depth <= d[i] is
        // strictly shallower, or there is no such item).
        //
        // Equivalently: find next_sos[i] = first j > i with depths[j] <= depths[i].
        // is_last[i] = (next_sos[i] == n) || (depths[next_sos[i]] < depths[i]).
        let mut next_sos = vec![n; n]; // next same-or-shallower index
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..n {
            while let Some(&top) = stack.last() {
                if depths[top] >= depths[i] {
                    next_sos[top] = i;
                    stack.pop();
                } else {
                    break;
                }
            }
            stack.push(i);
        }
        let display_is_last: Vec<bool> = (0..n)
            .map(|i| {
                let j = next_sos[i];
                j == n || depths[j] < depths[i]
            })
            .collect();

        // ── Step 2: prefix pipe flags (forward, O(n × max_depth)) ───────────────
        //
        // For item i at depth d, column c (0 <= c < d-1) shows │ iff the item's
        // ancestor at depth c+1 is NOT the last in its sibling group.
        // We skip depth 0 (root) to avoid an extra leading spacer column.
        // prefix_flags[i][c] = !is_last[ ancestor_of_i_at_depth_(c+1) ]
        //
        // Track the current ancestor at each depth using a slot array updated as
        // we sweep forward in DFS order.
        let max_depth = depths.iter().copied().max().unwrap_or(0);
        let mut current_ancestor: Vec<Option<usize>> = vec![None; max_depth + 1];
        let mut display_prefixes: Vec<Vec<bool>> = vec![Vec::new(); n];
        for i in 0..n {
            let d = depths[i];
            // Items at depth >= d are no longer ancestors of i.
            for slot in current_ancestor.iter_mut().skip(d) {
                *slot = None;
            }
            // Build prefix flags from ancestor chain.
            display_prefixes[i] = (1..d)
                .map(|c| match current_ancestor.get(c).and_then(|&o| o) {
                    Some(a) => !display_is_last[a],
                    None => false,
                })
                .collect();
            // Record self as the current occupant at depth d.
            if current_ancestor.len() <= d {
                current_ancestor.resize(d + 1, None);
            }
            current_ancestor[d] = Some(i);
        }

        self.display_prefixes = display_prefixes;
        self.display_is_last = display_is_last;
    }

    /// Position of the currently selected match within `display`.
    pub fn selected_display_pos(&self) -> usize {
        let Some(&target_idx) = self.filtered.get(self.match_pos) else {
            return 0;
        };
        self.display
            .iter()
            .position(|item| *item == DisplayItem::Match(target_idx))
            .unwrap_or(0)
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered
    }

    pub fn set_entries(&mut self, entries: Vec<TreeEntry>, scanned_at: Option<String>) {
        self.entries = entries;
        self.scanned_at = scanned_at;
        self.match_pos = 0;
        self.scroll = 0;
        self.rebuild_filter();
    }

    pub fn push_query(&mut self, ch: char) {
        self.query.push(ch);
        self.match_pos = 0;
        self.scroll = 0;
        self.rebuild_filter();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.match_pos = 0;
        self.scroll = 0;
        self.rebuild_filter();
    }

    pub fn selected_entry(&self) -> Option<&TreeEntry> {
        self.filtered
            .get(self.match_pos.min(self.filtered.len().saturating_sub(1)))
            .and_then(|idx| self.entries.get(*idx))
    }

    pub fn move_prev(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else if self.match_pos == 0 {
            self.match_pos = len - 1;
        } else {
            self.match_pos -= 1;
        }
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = (self.match_pos + 1) % len;
        }
    }

    pub fn sync_match_pos(&mut self) {
        let len = self.filtered_indices().len();
        self.match_pos = self.match_pos.min(len.saturating_sub(1));
    }
}

pub fn spawn_tree_scan(root: PathBuf) -> TreeScanTask {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        let result = scan_tree(&root, &cancel_bg, |visited, progress, current, levels| {
            let _ = tx.send(TreeScanMessage::Progress {
                visited,
                progress,
                levels: levels.to_vec(),
                current: current.to_path_buf(),
            });
        });
        match result {
            Ok(Some(entries)) => {
                let scanned_at = DateTime::<Local>::from(std::time::SystemTime::now())
                    .format("%Y-%m-%d %H:%M")
                    .to_string();
                let _ = save_cache(&root, &entries, &scanned_at);
                let _ = tx.send(TreeScanMessage::Finished {
                    entries,
                    scanned_at,
                });
            }
            Ok(None) => {
                let _ = tx.send(TreeScanMessage::Cancelled);
            }
            Err(err) => {
                let _ = tx.send(TreeScanMessage::Failed(err.to_string()));
            }
        }
    });
    TreeScanTask { rx, cancel }
}

fn scan_tree<F>(root: &Path, cancel: &AtomicBool, mut progress: F) -> Result<Option<Vec<TreeEntry>>>
where
    F: FnMut(usize, f64, &Path, &[TreeProgressLevel]),
{
    let mut entries = Vec::new();
    let mut visited = 0usize;
    let mut levels = Vec::new();
    scan_children(
        root,
        root,
        0,
        0.0,
        1.0,
        cancel,
        &mut entries,
        &mut visited,
        &mut levels,
        &mut progress,
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    levels.clear();
    levels.push(TreeProgressLevel {
        depth: 0,
        ratio: 1.0,
        path: root.to_path_buf(),
    });
    progress(visited, 1.0, root, &levels);
    Ok(Some(entries))
}

fn scan_children<F>(
    root: &Path,
    dir: &Path,
    depth: usize,
    base: f64,
    span: f64,
    cancel: &AtomicBool,
    entries: &mut Vec<TreeEntry>,
    visited: &mut usize,
    levels: &mut Vec<TreeProgressLevel>,
    progress: &mut F,
) -> Result<()>
where
    F: FnMut(usize, f64, &Path, &[TreeProgressLevel]),
{
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    let children = read_children(dir, depth).unwrap_or_default();
    let child_dir_count = children.iter().filter(|(_, _, _, is_symlink)| !*is_symlink).count();
    let child_span = if child_dir_count > 0 {
        span / child_dir_count as f64
    } else {
        span
    };
    let mut child_dir_idx = 0usize;
    set_progress_level(
        levels,
        depth,
        dir,
        if child_dir_count == 0 { 1.0 } else { 0.0 },
    );
    progress(*visited, base.clamp(0.0, 1.0), dir, levels);

    for (path, name, entry_depth, is_symlink) in children {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        entries.push(TreeEntry {
            search_key: rel_search_key(root, &path),
            path: path.clone(),
            name,
            depth: entry_depth,
            is_dir: true,
        });

        if !is_symlink {
            let child_base = base + child_span * child_dir_idx as f64;
            let parent_ratio = child_dir_idx as f64 / child_dir_count.max(1) as f64;
            child_dir_idx += 1;
            *visited += 1;
            set_progress_level(levels, depth, dir, parent_ratio);
            set_progress_level(levels, depth + 1, &path, 0.0);
            progress(*visited, child_base.clamp(0.0, 1.0), &path, levels);
            scan_children(
                root,
                &path,
                entry_depth + 1,
                child_base,
                child_span,
                cancel,
                entries,
                visited,
                levels,
                progress,
            )?;
            let done_ratio = child_dir_idx as f64 / child_dir_count.max(1) as f64;
            set_progress_level(levels, depth, dir, done_ratio);
            levels.truncate(depth + 1);
            progress(
                *visited,
                (child_base + child_span).clamp(0.0, 1.0),
                &path,
                levels,
            );
        }
    }

    Ok(())
}

fn set_progress_level(levels: &mut Vec<TreeProgressLevel>, depth: usize, path: &Path, ratio: f64) {
    levels.truncate(depth);
    levels.push(TreeProgressLevel {
        depth,
        ratio: ratio.clamp(0.0, 1.0),
        path: path.to_path_buf(),
    });
}

type PendingTreeEntry = (PathBuf, String, usize, bool);

fn read_children(dir: &Path, depth: usize) -> Result<Vec<PendingTreeEntry>> {
    let children = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            let is_symlink = file_type.is_symlink();
            // For symlinks, follow them to check if they point to a directory.
            if !file_type.is_dir() && !(is_symlink && path.is_dir()) {
                return None;
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            if should_prune_dir(&path, &name) {
                return None;
            }
            Some((path, name, depth, is_symlink))
        })
        .collect::<Vec<_>>();
    Ok(children)
}

fn cache_dir() -> Result<PathBuf> {
    let dirs = crate::config::project_dirs()?;
    let dir = dirs.cache_dir().join("tree");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn cache_key(root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Path for the fast binary cache (V3).
fn bin_cache_path(root: &Path) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{}.v3.bin", cache_key(root))))
}

/// Path for the JSON cache (V2), used as fallback when the binary cache is absent.
fn cache_path(root: &Path) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{}.v2.json", cache_key(root))))
}

fn legacy_cache_path(root: &Path) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{}.json", cache_key(root))))
}

fn load_cache(root: &Path) -> Result<(Vec<TreeEntry>, String)> {
    // Fast path: binary V3 cache.
    if let Ok(result) = load_cache_bin(root) {
        return Ok(result);
    }

    // Fallback: JSON V2/V3 cache.
    let path = cache_path(root)?;
    if let Ok(file) = fs::File::open(&path) {
        let any: CompactTreeCacheAny = serde_json::from_reader(BufReader::new(file))?;
        let (entries, scanned_at) = match any {
            CompactTreeCacheAny::V3(cache) => {
                if cache.root != root {
                    anyhow::bail!("tree cache root mismatch");
                }
                let entries = compact_nodes_to_tree_entries(root, cache.entries);
                (entries, cache.scanned_at)
            }
            CompactTreeCacheAny::V2(cache) => {
                if cache.root != root {
                    anyhow::bail!("tree cache root mismatch");
                }
                let original_len = cache.entries.len();
                let entries: Vec<_> = cache
                    .entries
                    .into_iter()
                    .filter(|entry| entry.1)
                    .filter_map(|entry| compact_entry_to_tree_entry(root, entry))
                    .collect();
                if entries.len() != original_len {
                    // drop — we'll re-save as binary below
                }
                (entries, cache.scanned_at)
            }
        };
        // Promote JSON cache to binary for faster future loads.
        let _ = save_cache(root, &entries, &scanned_at);
        return Ok((entries, scanned_at));
    }

    // Legacy V1 full-struct JSON cache.
    let path = legacy_cache_path(root)?;
    if fs::metadata(&path)
        .map(|meta| meta.len())
        .unwrap_or_default()
        > 128 * 1024 * 1024
    {
        anyhow::bail!("legacy tree cache too large, rescan required");
    }
    let file =
        fs::File::open(&path).with_context(|| format!("reading tree cache {}", path.display()))?;
    let mut cache: TreeCache = serde_json::from_reader(BufReader::new(file))?;
    if cache.root != root {
        anyhow::bail!("tree cache root mismatch");
    }
    cache.entries.retain(|entry| entry.is_dir);
    enrich_entries(root, &mut cache.entries);
    let _ = save_cache(root, &cache.entries, &cache.scanned_at);
    Ok((cache.entries, cache.scanned_at))
}

fn save_cache(root: &Path, entries: &[TreeEntry], scanned_at: &str) -> Result<()> {
    let nodes = tree_entries_to_compact_nodes(entries)?;
    // Primary: fast binary format.
    save_cache_bin(root, &nodes, scanned_at)?;
    // Remove stale JSON caches so disk space is reclaimed.
    if let Ok(p) = cache_path(root) {
        let _ = fs::remove_file(p);
    }
    if let Ok(p) = legacy_cache_path(root) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Binary cache (V3) — custom format, no extra dependencies
// Magic: b"KKCT" + version u8=1 + root (u32 LE len + bytes) + scanned_at
// (u32 LE len + bytes) + count u32 LE + per-node: parent u32 LE
// (0xFFFFFFFF = root) + name_len u16 LE + name bytes
// ---------------------------------------------------------------------------

fn save_cache_bin(root: &Path, nodes: &[CompactTreeNode], scanned_at: &str) -> Result<()> {
    use std::io::Write;
    let path = bin_cache_path(root)?;
    let mut f = BufWriter::new(fs::File::create(&path)?);
    let root_bytes = root.to_string_lossy();
    let root_bytes = root_bytes.as_bytes();
    let at_bytes = scanned_at.as_bytes();
    f.write_all(b"KKCT")?;
    f.write_all(&[1u8])?;
    f.write_all(&(root_bytes.len() as u32).to_le_bytes())?;
    f.write_all(root_bytes)?;
    f.write_all(&(at_bytes.len() as u32).to_le_bytes())?;
    f.write_all(at_bytes)?;
    f.write_all(&(nodes.len() as u32).to_le_bytes())?;
    for node in nodes {
        let parent = node.parent.unwrap_or(0xFFFF_FFFF);
        let name = node.name.as_bytes();
        f.write_all(&parent.to_le_bytes())?;
        f.write_all(&(name.len() as u16).to_le_bytes())?;
        f.write_all(name)?;
    }
    Ok(())
}

fn load_cache_bin(root: &Path) -> Result<(Vec<TreeEntry>, String)> {
    use std::io::Read;
    let path = bin_cache_path(root)?;
    let mut f = BufReader::new(fs::File::open(&path)?);
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    anyhow::ensure!(&magic == b"KKCT", "not a binary tree cache");
    let mut ver = [0u8; 1];
    f.read_exact(&mut ver)?;
    anyhow::ensure!(ver[0] == 1, "unsupported binary cache version");
    let cache_root = PathBuf::from(bin_read_str(&mut f)?);
    anyhow::ensure!(cache_root == root, "binary cache root mismatch");
    let scanned_at = bin_read_str(&mut f)?;
    let mut buf4 = [0u8; 4];
    f.read_exact(&mut buf4)?;
    let count = u32::from_le_bytes(buf4) as usize;
    let mut nodes = Vec::with_capacity(count);
    let mut buf2 = [0u8; 2];
    for _ in 0..count {
        f.read_exact(&mut buf4)?;
        let parent_raw = u32::from_le_bytes(buf4);
        let parent = if parent_raw == 0xFFFF_FFFF { None } else { Some(parent_raw) };
        f.read_exact(&mut buf2)?;
        let name_len = u16::from_le_bytes(buf2) as usize;
        let mut name_bytes = vec![0u8; name_len];
        f.read_exact(&mut name_bytes)?;
        let name = String::from_utf8(name_bytes).context("invalid UTF-8 in binary cache")?;
        nodes.push(CompactTreeNode { parent, name });
    }
    Ok((compact_nodes_to_tree_entries(root, nodes), scanned_at))
}

fn bin_read_str(f: &mut impl std::io::Read) -> Result<String> {
    let mut buf = [0u8; 4];
    f.read_exact(&mut buf)?;
    let len = u32::from_le_bytes(buf) as usize;
    let mut bytes = vec![0u8; len];
    f.read_exact(&mut bytes)?;
    String::from_utf8(bytes).context("invalid UTF-8 in binary cache string")
}



fn enrich_entries(root: &Path, entries: &mut [TreeEntry]) {
    for entry in entries {
        entry.search_key = rel_search_key(root, &entry.path);
    }
}

fn tree_entries_to_compact_nodes(entries: &[TreeEntry]) -> Result<Vec<CompactTreeNode>> {
    let mut nodes = Vec::new();
    let mut depth_stack: Vec<u32> = Vec::new();

    for entry in entries.iter().filter(|entry| entry.is_dir) {
        let parent = if entry.depth == 0 {
            None
        } else {
            depth_stack.get(entry.depth - 1).copied()
        };
        nodes.push(CompactTreeNode {
            parent,
            name: entry.name.clone(),
        });
        let idx = u32::try_from(nodes.len() - 1).context("tree cache too large")?;
        if depth_stack.len() <= entry.depth {
            depth_stack.resize(entry.depth + 1, 0);
        }
        depth_stack[entry.depth] = idx;
        depth_stack.truncate(entry.depth + 1);
    }

    Ok(nodes)
}

fn compact_nodes_to_tree_entries(root: &Path, nodes: Vec<CompactTreeNode>) -> Vec<TreeEntry> {
    let mut entries = Vec::with_capacity(nodes.len());
    let mut paths: Vec<PathBuf> = Vec::with_capacity(nodes.len());
    let mut depths: Vec<usize> = Vec::with_capacity(nodes.len());

    for node in nodes {
        let parent_idx = node.parent.and_then(|idx| usize::try_from(idx).ok());
        let (depth, path) = match parent_idx {
            Some(parent) if parent < paths.len() => {
                let depth = depths[parent] + 1;
                (depth, paths[parent].join(&node.name))
            }
            Some(_) => {
                continue;
            }
            None => (0, root.join(&node.name)),
        };

        paths.push(path.clone());
        depths.push(depth);
        entries.push(TreeEntry {
            search_key: rel_search_key(root, &path),
            path,
            name: node.name,
            depth,
            is_dir: true,
        });
    }

    entries
}

fn compact_entry_to_tree_entry(root: &Path, entry: CompactTreeEntry) -> Option<TreeEntry> {
    if !entry.1 {
        return None;
    }
    let rel = PathBuf::from(entry.0);
    let path = root.join(&rel);
    let name = rel.file_name()?.to_string_lossy().into_owned();
    let depth = rel.components().count().saturating_sub(1);
    Some(TreeEntry {
        search_key: rel_search_key(root, &path),
        path,
        name,
        depth,
        is_dir: entry.1,
    })
}

fn rel_search_key(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    normalize_search_value(&rel.to_string_lossy())
}

fn should_prune_dir(path: &Path, name: &str) -> bool {
    if matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | ".cache"
            | "__pycache__"
            | ".venv"
            | ".mypy_cache"
            | ".pytest_cache"
    ) {
        return true;
    }

    path.ends_with("Library/Developer/Xcode/DerivedData")
        || path.ends_with("Library/Developer/CoreSimulator/Devices")
}

fn normalize_search_value(value: &str) -> String {
    value.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_filter_matches_substring_per_token() {
        let root = Path::new("/home/user");
        let key = |p: &str| rel_search_key(root, &root.join(p));
        // single token substring
        assert!(key("kkc-rust").contains("kkc"));
        assert!(key("sources/kkc-rust").contains("sources"));
        // AND via rebuild_filter: test tokens directly
        let tokens = ["kkc", "rust"];
        let k = key("sources/kkc-rust");
        assert!(tokens.iter().all(|t| k.contains(t)));
        // parent path participates in match
        let k2 = key("projects/kkc/src");
        assert!(k2.contains("projects"));
        assert!(k2.contains("kkc"));
        // consecutive only — no subsequence across tokens
        assert!(!key("projects").contains("pjts"));
    }

    #[test]
    fn scan_tree_collects_nested_directories_only() {
        let root = std::env::temp_dir().join(format!("kkc-tree-scan-{}", std::process::id()));
        let nested = root.join("src").join("inner");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::write(root.join("README.md"), b"readme").expect("write file");
        fs::write(nested.join("mod.rs"), b"mod").expect("write file");

        let cancel = AtomicBool::new(false);
        let entries = scan_tree(&root, &cancel, |_, _, _, _| {})
            .expect("scan should succeed")
            .expect("scan should not be cancelled");

        assert!(
            entries
                .iter()
                .any(|entry| entry.name == "src" && entry.is_dir)
        );
        assert!(entries.iter().all(|entry| entry.is_dir));
        assert!(!entries.iter().any(|entry| entry.name == "README.md"));
        assert!(!entries.iter().any(|entry| entry.name == "mod.rs"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_tree_progress_subdivides_directory_weight() {
        let root = std::env::temp_dir().join(format!("kkc-tree-progress-{}", std::process::id()));
        fs::create_dir_all(root.join("a").join("aa")).expect("create nested dir");
        fs::create_dir_all(root.join("a").join("ab")).expect("create nested dir");
        fs::create_dir_all(root.join("b")).expect("create second root dir");

        let cancel = AtomicBool::new(false);
        let mut progress = Vec::new();
        let mut nested_levels_seen = false;
        let _ = scan_tree(&root, &cancel, |_, ratio, path, levels| {
            progress.push((
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                ratio,
            ));
            nested_levels_seen |= levels
                .iter()
                .any(|level| level.depth == 0 && level.path == root)
                && levels.iter().any(|level| {
                    level.depth == 1
                        && level
                            .path
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy() == "a")
                });
        })
        .expect("scan should succeed");

        assert!(progress.iter().any(|(_, ratio)| (*ratio - 0.25).abs() < f64::EPSILON));
        assert!(progress.iter().any(|(_, ratio)| (*ratio - 0.5).abs() < f64::EPSILON));
        assert!(
            progress
                .iter()
                .any(|(_, ratio)| (*ratio - 1.0).abs() < f64::EPSILON)
        );
        assert!(nested_levels_seen);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compact_cache_entry_drops_files() {
        let root = Path::new("/tmp/kkc-tree-root");
        assert!(compact_entry_to_tree_entry(
            root,
            CompactTreeEntry("file.txt".to_string(), false)
        )
        .is_none());
    }

    #[test]
    fn rebuild_filter_includes_ancestor_context() {
        let root = std::env::temp_dir().join(format!("kkc-ctx-{}", std::process::id()));
        // Tree: root/a/b/myproj  (myproj is the match, a and b are context)
        let target = root.join("a").join("b").join("myproj");
        fs::create_dir_all(&target).unwrap();

        let cancel = AtomicBool::new(false);
        let entries = scan_tree(&root, &cancel, |_, _, _, _| {})
            .unwrap()
            .unwrap();

        let mut state = TreeViewState::empty(root.clone());
        state.set_entries(entries, None);
        state.query = "myproj".into();
        state.rebuild_filter();

        // Only "myproj" is a real match.
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(state.entries[state.filtered[0]].name, "myproj");

        // Display must contain context ancestors "a" and "b" plus the match.
        assert_eq!(state.display.len(), 3);
        let names: Vec<&str> = state.display.iter().map(|item| {
            let idx = match item { DisplayItem::Context(i) | DisplayItem::Match(i) => *i };
            state.entries[idx].name.as_str()
        }).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"myproj"));

        // Context items are not navigable (display contains exactly one Match).
        assert_eq!(
            state.display.iter().filter(|i| matches!(i, DisplayItem::Match(_))).count(),
            1
        );

        let _ = fs::remove_dir_all(root);
    }
}
