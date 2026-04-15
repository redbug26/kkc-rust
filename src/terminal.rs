use crate::app::{App, AppMode, ActivePanel};
use crate::config;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

// ---------------------------------------------------------------------------
// Running external command (for streaming output)
// ---------------------------------------------------------------------------

pub enum CmdLine {
    Out(String),
    Err(String),
    Done(Option<i32>),
}

pub struct RunningCmd {
    pub rx: Receiver<CmdLine>,
    pub done: bool,
}

/// Spawn a command, streaming its output through a channel.
pub fn spawn_cmd_streaming(raw: String, dir: PathBuf) -> RunningCmd {
    let (tx, rx) = mpsc::channel::<CmdLine>();
    thread::spawn(move || {
        use std::process::{Command, Stdio};
        let result = Command::new("sh")
            .arg("-c")
            .arg(&raw)
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(CmdLine::Err(format!("error: {}", e)));
                let _ = tx.send(CmdLine::Done(None));
                return;
            }
        };
        let stdout = child.stdout.take().map(std::io::BufReader::new);
        let stderr = child.stderr.take().map(std::io::BufReader::new);
        // Spawn stdout reader thread
        let tx_out = tx.clone();
        if let Some(stdout) = stdout {
            thread::spawn(move || {
                for line in stdout.lines() {
                    match line {
                        Ok(l) => { let _ = tx_out.send(CmdLine::Out(l)); }
                        Err(_) => break,
                    }
                }
            });
        }
        // Read stderr in current thread
        if let Some(stderr) = stderr {
            for line in stderr.lines() {
                match line {
                    Ok(l) => { let _ = tx.send(CmdLine::Err(l)); }
                    Err(_) => break,
                }
            }
        }
        let status = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(CmdLine::Done(status));
    });
    RunningCmd { rx, done: false }
}

// ---------------------------------------------------------------------------
// Terminal state (Ctrl-U pseudo-terminal)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TerminalState {
    /// Current command-line input.
    pub input: String,
    /// Cursor byte-offset inside `input`.
    pub cursor: usize,
    /// Scrollback output lines.
    pub output: Vec<String>,
    /// Command history.
    pub history: Vec<String>,
    /// Position in history while browsing (None = at live prompt).
    pub history_pos: Option<usize>,
    /// Saved live input before history navigation started.
    pub live_input: String,
    /// Current tab-completion candidates.
    pub tab_candidates: Vec<String>,
    /// Which candidate is selected (cycles on repeated Tab).
    pub tab_index: usize,
    /// Common prefix before the tab token (restored on cycling).
    pub tab_prefix: String,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            output: Vec::new(),
            history: Vec::new(),
            history_pos: None,
            live_input: String::new(),
            tab_candidates: Vec::new(),
            tab_index: 0,
            tab_prefix: String::new(),
        }
    }

    /// Append a line to the scrollback, capped at 500 lines.
    pub fn push_output(&mut self, line: impl Into<String>) {
        if self.output.len() >= 500 {
            self.output.remove(0);
        }
        self.output.push(line.into());
    }

    pub fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.reset_tab();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let mut prev = self.cursor - 1;
            while prev > 0 && !self.input.is_char_boundary(prev) {
                prev -= 1;
            }
            self.input.remove(prev);
            self.cursor = prev;
            self.reset_tab();
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
            self.reset_tab();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let mut p = self.cursor - 1;
            while p > 0 && !self.input.is_char_boundary(p) {
                p -= 1;
            }
            self.cursor = p;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            let mut p = self.cursor + 1;
            while p < self.input.len() && !self.input.is_char_boundary(p) {
                p += 1;
            }
            self.cursor = p;
        }
    }

    pub fn home(&mut self) { self.cursor = 0; }
    pub fn end(&mut self)  { self.cursor = self.input.len(); }

    /// Kill from cursor to end of line.
    #[allow(dead_code)]
    pub fn kill_line(&mut self) {
        self.input.truncate(self.cursor);
        self.reset_tab();
    }

    pub fn reset_tab(&mut self) {
        self.tab_candidates.clear();
        self.tab_index = 0;
        self.tab_prefix.clear();
    }
}

// ---------------------------------------------------------------------------
// Terminal cache  (history + scrollback persisted to cache directory)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Default)]
struct TerminalCache {
    #[serde(default)]
    history: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

/// Load terminal history and output from the cache directory.
/// Returns `(history, output)`, both empty on any error.
pub fn load_terminal_cache() -> (Vec<String>, Vec<String>) {
    let path = match config::terminal_cache_path() {
        Ok(p) => p,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    if !path.exists() {
        return (Vec::new(), Vec::new());
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    let cache: TerminalCache = toml::from_str(&text).unwrap_or_default();
    (cache.history, cache.output)
}

/// Save terminal history and output to the cache directory.
pub fn save_terminal_cache(history: &[String], output: &[String]) -> Result<()> {
    let path = config::terminal_cache_path()?;
    let cache = TerminalCache {
        history: history.to_vec(),
        output: output.to_vec(),
    };
    let text = toml::to_string(&cache)?;
    std::fs::write(&path, text)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Ctrl-U / Esc pseudo-terminal mode – event handler
// ---------------------------------------------------------------------------

pub fn handle_terminal(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // ── Ctrl chords ─────────────────────────────────────────────────────────
    if ctrl {
        match key.code {
            // Ctrl-U closes the terminal (toggle back to browse)
            KeyCode::Char('u') => {
                app.mode = AppMode::Browse;
                return Ok(false);
            }
            // Ctrl-K = kill from cursor to end of line
            KeyCode::Char('k') => {
                let ts = &mut app.terminal;
                ts.input.truncate(ts.cursor);
                ts.reset_tab();
                return Ok(false);
            }
            // Ctrl-A = go to beginning
            KeyCode::Char('a') => { app.terminal.home(); return Ok(false); }
            // Ctrl-E = go to end
            KeyCode::Char('e') => { app.terminal.end(); return Ok(false); }
            _ => {}
        }
    }

    match key.code {
        // ── Exit ──────────────────────────────────────────────────────────
        KeyCode::Esc | KeyCode::F(10) => {
            app.mode = AppMode::Browse;
        }

        // ── Execute command ───────────────────────────────────────────────
        KeyCode::Enter => {
            // Don't allow new commands while a command is running
            if app.running_cmd.is_some() {
                app.terminal.push_output("[command still running]");
                return Ok(false);
            }
            let quit = terminal_execute(app)?;
            if quit { return Ok(true); }
        }

        // ── Tab completion ────────────────────────────────────────────────
        KeyCode::Tab => {
            terminal_tab_complete(app);
        }

        // ── History navigation ────────────────────────────────────────────
        KeyCode::Up => {
            let ts = &mut app.terminal;
            if ts.history.is_empty() { return Ok(false); }
            let next_pos = match ts.history_pos {
                None => {
                    ts.live_input = ts.input.clone();
                    ts.history.len() - 1
                }
                Some(p) if p > 0 => p - 1,
                Some(p) => p,
            };
            let entry = ts.history[next_pos].clone();
            ts.input = entry;
            ts.cursor = ts.input.len();
            ts.history_pos = Some(next_pos);
            ts.reset_tab();
        }
        KeyCode::Down => {
            let ts = &mut app.terminal;
            match ts.history_pos {
                None => {}
                Some(p) if p + 1 < ts.history.len() => {
                    let next_pos = p + 1;
                    let entry = ts.history[next_pos].clone();
                    ts.input = entry;
                    ts.cursor = ts.input.len();
                    ts.history_pos = Some(next_pos);
                    ts.reset_tab();
                }
                Some(_) => {
                    let live = ts.live_input.clone();
                    ts.input = live;
                    ts.cursor = ts.input.len();
                    ts.history_pos = None;
                    ts.reset_tab();
                }
            }
        }

        // ── Line editing ──────────────────────────────────────────────────
        KeyCode::Left  => { app.terminal.move_left(); }
        KeyCode::Right => { app.terminal.move_right(); }
        KeyCode::Home  => { app.terminal.home(); }
        KeyCode::End   => { app.terminal.end(); }
        KeyCode::Backspace => { app.terminal.backspace(); }
        KeyCode::Delete    => { app.terminal.delete_char(); }

        KeyCode::Char(ch) if !ctrl => {
            app.terminal.insert_char(ch);
        }

        _ => {}
    }

    Ok(false)
}

/// Execute the current input line.  Returns `true` to quit the whole app.
fn terminal_execute(app: &mut App) -> Result<bool> {
    let raw = app.terminal.input.trim().to_string();
    app.terminal.push_output(format!("$ {}", raw));
    app.terminal.input.clear();
    app.terminal.cursor = 0;
    app.terminal.reset_tab();

    // Record in history (skip blanks, skip exact duplicate of last entry)
    if !raw.is_empty()
        && app.terminal.history.last().map(|s| s.as_str()) != Some(raw.as_str())
    {
        app.terminal.history.push(raw.clone());
    }
    app.terminal.history_pos = None;
    app.terminal.live_input.clear();

    if raw.is_empty() {
        return Ok(false);
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let cmd  = parts.next().unwrap_or("");
    let arg  = parts.next().map(str::trim).unwrap_or("");

    // ── Internal commands ────────────────────────────────────────────────
    match cmd {
        "exit" => {
            // exit quits kkc entirely
            return Ok(true);
        }

        "help" | "?" => {
            app.terminal.push_output("Built-in commands:");
            app.terminal.push_output("  cd [dir]       Change directory (panel + prompt)");
            app.terminal.push_output("  help           Show this message");
            app.terminal.push_output("  exit           Quit KKC");
            app.terminal.push_output("Key bindings:");
            app.terminal.push_output("  Tab            Cycle completions (dirs first for cd)");
            app.terminal.push_output("  Up/Down        Navigate history");
            app.terminal.push_output("  Ctrl-A/E       Start / end of line");
            app.terminal.push_output("  Ctrl-K         Kill to end");
            app.terminal.push_output("  Ctrl-U / Esc   Close terminal overlay");
            return Ok(false);
        }

        "cd" => {
            let panel_dir = match app.active {
                ActivePanel::Left  => app.left.path.clone(),
                ActivePanel::Right => app.right.path.clone(),
            };
            let target = if arg.is_empty() {
                directories::UserDirs::new()
                    .map(|u| u.home_dir().to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("/"))
            } else if arg.starts_with('/') || arg.starts_with('~') {
                let expanded = if let Some(rest) = arg.strip_prefix("~/") {
                    directories::UserDirs::new()
                        .map(|u| u.home_dir().join(rest))
                        .unwrap_or_else(|| std::path::PathBuf::from(arg))
                } else if arg == "~" {
                    directories::UserDirs::new()
                        .map(|u| u.home_dir().to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("/"))
                } else {
                    std::path::PathBuf::from(arg)
                };
                expanded
            } else {
                panel_dir.join(arg)
            };
            match std::fs::canonicalize(&target) {
                Ok(p) => {
                    if let Err(e) = app.active_panel_mut().enter_dir(p) {
                        app.terminal.push_output(format!("cd: {}", e));
                    }
                }
                Err(e) => {
                    app.terminal.push_output(format!("cd: {}: {}", arg, e));
                }
            }
            return Ok(false);
        }

        _ => {}
    }

    // ── External command – spawn thread, stream output ───────────────────
    let panel_dir = match app.active {
        ActivePanel::Left  => app.left.path.clone(),
        ActivePanel::Right => app.right.path.clone(),
    };
    app.running_cmd = Some(spawn_cmd_streaming(raw, panel_dir));
    Ok(false)
}

/// Bash-style tab completion from the filesystem.
///
/// * If the token starts with `./`, `/`, or `~/`, complete from that path.
/// * Otherwise complete relative to the active panel's directory.
/// * For `cd` as the first word, only directories are shown.
fn terminal_tab_complete(app: &mut App) {
    // Guard: don't complete while a command is running
    if app.running_cmd.is_some() { return; }

    // ── If candidates already exist, cycle ───────────────────────────────
    if !app.terminal.tab_candidates.is_empty() {
        let ts = &mut app.terminal;
        ts.tab_index = (ts.tab_index + 1) % ts.tab_candidates.len();
        let cand = ts.tab_candidates[ts.tab_index].clone();
        ts.input = format!("{}{}", ts.tab_prefix, cand);
        ts.cursor = ts.input.len();
        return;
    }

    // ── Parse the input up to the cursor ────────────────────────────────
    let before_cursor = app.terminal.input[..app.terminal.cursor].to_string();

    // Split into "prefix before the last word" and "the word being typed"
    let (cmd_prefix, token) = if let Some(pos) = before_cursor.rfind(|c: char| c.is_whitespace()) {
        (before_cursor[..=pos].to_string(), before_cursor[pos + 1..].to_string())
    } else {
        (String::new(), before_cursor.clone())
    };

    // Is the first word `cd`? (Only complete dirs.)
    let first_word = before_cursor
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let dirs_only = first_word == "cd" || cmd_prefix.is_empty() && token == "cd";

    // Resolve base dir and partial name for the token
    let panel_dir = match app.active {
        ActivePanel::Left  => app.left.path.clone(),
        ActivePanel::Right => app.right.path.clone(),
    };

    // Expand leading ~/
    let token_expanded = if let Some(rest) = token.strip_prefix("~/") {
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        home.join(rest).to_string_lossy().into_owned()
    } else {
        token.clone()
    };

    let (base_dir, partial_name, keep_prefix_in_candidate) = {
        let p = std::path::Path::new(&token_expanded);
        if let Some(parent) = p.parent().filter(|pp| !pp.as_os_str().is_empty()) {
            let base = if p.is_absolute() {
                parent.to_path_buf()
            } else {
                panel_dir.join(parent)
            };
            let partial = p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // We prepend the directory part back to each candidate
            let prefix_part = format!("{}/", parent.to_string_lossy().trim_end_matches('/'));
            (base, partial, prefix_part)
        } else {
            (panel_dir.clone(), token_expanded.clone(), String::new())
        }
    };

    // Read the directory
    let read = match std::fs::read_dir(&base_dir) {
        Ok(r) => r,
        Err(_) => return,
    };

    let partial_lc = partial_name.to_lowercase();
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            name_str.to_lowercase().starts_with(&partial_lc)
                && !(partial_name.is_empty() && name_str.starts_with('.'))
        })
        .filter(|e| {
            if dirs_only {
                e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                    || e.path().is_dir() // follow symlinks
            } else {
                true
            }
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                || e.path().is_dir();
            let suffix = if is_dir { "/" } else { "" };
            format!("{}{}{}", keep_prefix_in_candidate, name, suffix)
        })
        .collect();
    names.sort();

    if names.is_empty() { return; }

    // Apply first candidate
    let ts = &mut app.terminal;
    ts.tab_prefix = cmd_prefix;
    ts.tab_index = 0;
    let first = names[0].clone();
    ts.tab_candidates = names;
    ts.input = format!("{}{}", ts.tab_prefix, first);
    ts.cursor = ts.input.len();
}
