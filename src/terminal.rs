use crate::app::{App, AppMode, ActivePanel};
use crate::config;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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
        #[cfg(unix)]
        use std::os::unix::process::CommandExt;

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&raw)
            .current_dir(&dir)
            // Force color output: programs detect Stdio::piped() as non-TTY
            // and disable colors unless we tell them otherwise.
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("CLICOLOR_FORCE", "1")      // macOS/BSD ls and friends
            .env("FORCE_COLOR", "1")          // Node.js ecosystem
            .env("CARGO_TERM_COLOR", "always")// cargo
            .env("GIT_TERMINAL_PROMPT", "0")  // avoid git hanging on auth
            // Disconnect stdin so no child can block waiting for user input,
            // and so bash doesn't try to do TTY/job-control setup.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Put the child in its own process group so it cannot open /dev/tty
        // (the controlling terminal of kkc).  Without this, commands like
        // `clear` call `tput` which does ioctl on /dev/tty while crossterm
        // has raw mode active, corrupting the terminal state and appearing
        // to freeze kkc.
        #[cfg(unix)]
        cmd.process_group(0);

        let result = cmd.spawn();
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

        // Spawn stderr reader thread — MUST be a separate thread, not blocking
        // here, because reading stderr synchronously while stdout is also being
        // produced causes a classic pipe-buffer deadlock:
        //   child blocks on full stdout pipe → never closes stderr →
        //   our blocking stderr read never returns → child.wait() never called.
        let tx_err = tx.clone();
        if let Some(stderr) = stderr {
            thread::spawn(move || {
                for line in stderr.lines() {
                    match line {
                        Ok(l) => { let _ = tx_err.send(CmdLine::Err(l)); }
                        Err(_) => break,
                    }
                }
            });
        }

        // Wait for child to exit then signal completion.
        // tx_out / tx_err may still be alive in their threads after this point;
        // that is fine — Done is sent into an ordered channel, so any Out/Err
        // messages already queued will be received before Done.
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
// ANSI escape-code parser — converts a raw string to styled ratatui spans
// ---------------------------------------------------------------------------

/// Parse a string containing ANSI SGR escape sequences and return a styled
/// ratatui `Line`.  Only SGR (`ESC [ … m`) sequences are interpreted; other
/// escape sequences are silently skipped.
pub fn ansi_line_to_line(text: &str) -> Line<'static> {
    let bytes = text.as_bytes();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut chunk = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    // CSI sequence — flush pending text, parse SGR
                    if !chunk.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut chunk), style));
                    }
                    i += 2; // skip ESC [
                    let start = i;
                    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b';') {
                        i += 1;
                    }
                    if i >= bytes.len() { break; }
                    let cmd = bytes[i] as char;
                    i += 1; // consume command byte
                    if cmd == 'm' {
                        let param_str = std::str::from_utf8(&bytes[start..i - 1]).unwrap_or("");
                        style = apply_sgr(style, param_str);
                    }
                    // All other CSI commands (cursor movement, etc.) are dropped
                }
                b']' => {
                    // OSC sequence (e.g. hyperlinks: ESC ] 8 ; ; URL ESC \)
                    // Flush pending text, then skip the whole OSC until BEL or ESC \.
                    // The visible text between two OSC 8 sequences is NOT inside
                    // the sequence — it is plain text we keep.
                    if !chunk.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut chunk), style));
                    }
                    i += 2; // skip ESC ]
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            // BEL terminator
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            // ESC \ (String Terminator)
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'\\' => {
                    // Bare String Terminator (ESC \) without a preceding OSC — skip
                    i += 2;
                }
                _ => {
                    // Any other ESC sequence (ESC c, ESC M, …) — skip one byte
                    i += 2;
                }
            }
        } else if bytes[i] < 0x20 && bytes[i] != b'\t' {
            // Skip other control chars (CR, BEL, etc.)
            i += 1;
        } else {
            // Regular character — push to current chunk
            chunk.push(bytes[i] as char);
            i += 1;
        }
    }
    if !chunk.is_empty() {
        spans.push(Span::styled(chunk, style));
    }
    Line::from(spans)
}

/// Apply a single SGR parameter string to the current style.
fn apply_sgr(mut style: Style, params: &str) -> Style {
    if params.is_empty() {
        return Style::default();
    }
    let nums: Vec<u16> = params
        .split(';')
        .filter_map(|s| s.parse().ok())
        .collect();
    let mut idx = 0;
    while idx < nums.len() {
        let n = nums[idx];
        match n {
            0  => style = Style::default(),
            1  => style = style.add_modifier(Modifier::BOLD),
            2  => style = style.add_modifier(Modifier::DIM),
            3  => style = style.add_modifier(Modifier::ITALIC),
            4  => style = style.add_modifier(Modifier::UNDERLINED),
            5 | 6 => style = style.add_modifier(Modifier::SLOW_BLINK),
            7  => style = style.add_modifier(Modifier::REVERSED),
            9  => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            25 => style = style.remove_modifier(Modifier::SLOW_BLINK),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            // Foreground: standard 8 colors
            30 => style = style.fg(Color::Black),
            31 => style = style.fg(Color::Red),
            32 => style = style.fg(Color::Green),
            33 => style = style.fg(Color::Yellow),
            34 => style = style.fg(Color::Blue),
            35 => style = style.fg(Color::Magenta),
            36 => style = style.fg(Color::Cyan),
            37 => style = style.fg(Color::White),
            // 38: extended foreground
            38 => {
                if let Some(color) = parse_extended_color(&nums, &mut idx) {
                    style = style.fg(color);
                }
                continue; // idx already advanced in parse_extended_color
            }
            39 => style = style.fg(Color::Reset),
            // Background: standard 8 colors
            40 => style = style.bg(Color::Black),
            41 => style = style.bg(Color::Red),
            42 => style = style.bg(Color::Green),
            43 => style = style.bg(Color::Yellow),
            44 => style = style.bg(Color::Blue),
            45 => style = style.bg(Color::Magenta),
            46 => style = style.bg(Color::Cyan),
            47 => style = style.bg(Color::White),
            // 48: extended background
            48 => {
                if let Some(color) = parse_extended_color(&nums, &mut idx) {
                    style = style.bg(color);
                }
                continue; // idx already advanced
            }
            49 => style = style.bg(Color::Reset),
            // Bright foreground colors
            90 => style = style.fg(Color::DarkGray),
            91 => style = style.fg(Color::LightRed),
            92 => style = style.fg(Color::LightGreen),
            93 => style = style.fg(Color::LightYellow),
            94 => style = style.fg(Color::LightBlue),
            95 => style = style.fg(Color::LightMagenta),
            96 => style = style.fg(Color::LightCyan),
            97 => style = style.fg(Color::Gray),
            // Bright background colors
            100 => style = style.bg(Color::DarkGray),
            101 => style = style.bg(Color::LightRed),
            102 => style = style.bg(Color::LightGreen),
            103 => style = style.bg(Color::LightYellow),
            104 => style = style.bg(Color::LightBlue),
            105 => style = style.bg(Color::LightMagenta),
            106 => style = style.bg(Color::LightCyan),
            107 => style = style.bg(Color::Gray),
            _ => {}
        }
        idx += 1;
    }
    style
}

/// Parse `38;5;n`, `38;2;r;g;b` (and `48;…`) extended color sequences.
/// On entry `idx` points at the 38 or 48.  On return `idx` points at the
/// last consumed parameter so the caller's `idx += 1` lands correctly.
fn parse_extended_color(nums: &[u16], idx: &mut usize) -> Option<Color> {
    match nums.get(*idx + 1) {
        Some(&5) => {
            // 256-color palette
            if let Some(&n) = nums.get(*idx + 2) {
                *idx += 2;
                Some(Color::Indexed(n as u8))
            } else {
                None
            }
        }
        Some(&2) => {
            // True color
            if let (Some(&r), Some(&g), Some(&b)) =
                (nums.get(*idx + 2), nums.get(*idx + 3), nums.get(*idx + 4))
            {
                *idx += 4;
                Some(Color::Rgb(r as u8, g as u8, b as u8))
            } else {
                None
            }
        }
        _ => None,
    }
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
