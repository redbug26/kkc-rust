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
    CloseTextEditorUnsaved,
    /// Editor has unsaved changes and the user asked to quit (F10).
    /// Y/Enter = save + close + quit, N = discard + close + quit, Esc = cancel.
    SaveEditorBeforeQuit,
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
pub struct AssocInputDialog {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor: usize,
    pub action: AssocInputAction,
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
    PluginAction {
        plugin: String,
        id: String,
        cwd: PathBuf,
    },
    SaveSelectionSession,
    LoadSelectionSession,
}

#[derive(Debug, Clone)]
pub enum AssocInputAction {
    /// Step 1 of adding an association: user typed the MIME type.
    MimeType,
    /// Step 2 of adding/editing: user typed the openers (one command per line).
    Openers {
        ext: String,
        /// Some(idx) = editing existing row, None = new.
        edit_index: Option<usize>,
    },
}

pub trait TextInputState {
    fn value(&self) -> &String;
    fn value_mut(&mut self) -> &mut String;
    fn cursor(&self) -> usize;
    fn cursor_mut(&mut self) -> &mut usize;

    fn insert_char(&mut self, ch: char) {
        let cursor = self.cursor();
        self.value_mut().insert(cursor, ch);
        *self.cursor_mut() = cursor + ch.len_utf8();
    }

    fn backspace(&mut self) {
        let cursor = self.cursor();
        if cursor > 0 {
            let mut prev = cursor - 1;
            while prev > 0 && !self.value().is_char_boundary(prev) {
                prev -= 1;
            }
            self.value_mut().remove(prev);
            *self.cursor_mut() = prev;
        }
    }

    fn delete_char(&mut self) {
        let cursor = self.cursor();
        if cursor < self.value().len() {
            self.value_mut().remove(cursor);
        }
    }

    fn move_left(&mut self) {
        let cursor = self.cursor();
        if cursor > 0 {
            let mut pos = cursor - 1;
            while pos > 0 && !self.value().is_char_boundary(pos) {
                pos -= 1;
            }
            *self.cursor_mut() = pos;
        }
    }

    fn move_right(&mut self) {
        let cursor = self.cursor();
        if cursor < self.value().len() {
            let mut pos = cursor + 1;
            while pos < self.value().len() && !self.value().is_char_boundary(pos) {
                pos += 1;
            }
            *self.cursor_mut() = pos;
        }
    }

    fn home(&mut self) {
        *self.cursor_mut() = 0;
    }

    fn end(&mut self) {
        *self.cursor_mut() = self.value().len();
    }
}

impl TextInputState for InputDialog {
    fn value(&self) -> &String {
        &self.value
    }

    fn value_mut(&mut self) -> &mut String {
        &mut self.value
    }

    fn cursor(&self) -> usize {
        self.cursor
    }

    fn cursor_mut(&mut self) -> &mut usize {
        &mut self.cursor
    }
}

impl TextInputState for AssocInputDialog {
    fn value(&self) -> &String {
        &self.value
    }

    fn value_mut(&mut self) -> &mut String {
        &mut self.value
    }

    fn cursor(&self) -> usize {
        self.cursor
    }

    fn cursor_mut(&mut self) -> &mut usize {
        &mut self.cursor
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
