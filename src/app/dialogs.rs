use super::*;

#[derive(Debug)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug)]
pub enum ConfirmAction {
    Message,
    /// Show message, then switch to this mode on dismiss.
    MessageThen(Box<AppMode>),
    Quit,
    Delete(Vec<PathBuf>),
    DeleteRemote(Vec<RemoteDeleteTarget>),
}

#[derive(Debug, Clone)]
pub struct RemoteDeleteTarget {
    pub profile: RemoteProfile,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct InputDialog {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor: usize,
    pub action: InputAction,
}

#[derive(Debug, Clone)]
pub enum InputAction {
    Rename(PathBuf),
    Mkdir(PathBuf),
    RemoteRename {
        profile: RemoteProfile,
        path: String,
    },
    RemoteMkdir {
        profile: RemoteProfile,
        parent: String,
    },
    /// Wildcard select (+)
    SelectPattern,
    /// Wildcard deselect (-)
    DeselectPattern,
    /// Navigate active panel to typed path
    GoToPath,
    /// Step 1 of adding an association: user typed the extension
    AssocAddExt,
    /// Step 2 of adding/editing: user typed the openers (comma-separated)
    AssocAddOpeners {
        ext: String,
        /// Some(idx) = editing existing row, None = new
        edit_index: Option<usize>,
    },
    PluginAction {
        plugin: String,
        id: String,
        cwd: PathBuf,
    },
}

impl InputDialog {
    pub fn insert_char(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous char boundary
            let mut prev = self.cursor - 1;
            while prev > 0 && !self.value.is_char_boundary(prev) {
                prev -= 1;
            }
            self.value.remove(prev);
            self.cursor = prev;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let mut p = self.cursor - 1;
            while p > 0 && !self.value.is_char_boundary(p) {
                p -= 1;
            }
            self.cursor = p;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.len() {
            let mut p = self.cursor + 1;
            while p < self.value.len() && !self.value.is_char_boundary(p) {
                p += 1;
            }
            self.cursor = p;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.value.len();
    }
}

// ---------------------------------------------------------------------------
// Search state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SearchState {
    pub query: String,
    pub content_query: String,
    pub dir_query: String,
    pub input_field: usize, // 0=name 1=content 2=dir 3=results
    pub results: Vec<SearchResult>,
    pub cursor: usize,
    pub scroll: usize,
    pub running: bool,
    pub start_dir: PathBuf,
    pub backend: SearchBackend,
    pub follow_links: bool,
    /// Background search thread sends results here.
    pub search_rx: Option<std::sync::mpsc::Receiver<SearchResult>>,
    /// Set to `true` to ask the background thread to stop early.
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// Number of directories visited so far (for progress display).
    pub dirs_visited: usize,
}
