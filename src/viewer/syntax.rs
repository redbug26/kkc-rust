//! Pure in-process syntax highlighting for the file viewer.
//!
//! No external crates — hand-rolled tokenisers for each supported language
//! inspired by the highlight.js grammar definitions.

use super::MaskKind;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

// ── Token colour palette  (VS Code Dark+ inspired) ───────────────────────
const CLR_KEYWORD:  Color = Color::Rgb(86,  156, 214); // blue
const CLR_TYPE:     Color = Color::Rgb(78,  201, 176); // teal
const CLR_STRING:   Color = Color::Rgb(206, 145, 120); // salmon / orange
const CLR_COMMENT:  Color = Color::Rgb(106, 153, 85);  // green
const CLR_NUMBER:   Color = Color::Rgb(181, 206, 168); // pale green
const CLR_PREPROC:  Color = Color::Rgb(197, 134, 192); // violet / pink
const CLR_FUNC:     Color = Color::Rgb(220, 220, 170); // pale yellow
const CLR_OPERATOR: Color = Color::Rgb(180, 200, 240); // light blue-gray
const CLR_PLAIN:    Color = Color::Rgb(212, 212, 212); // light gray
const CLR_KETCHUP:  Color = Color::Yellow;

#[inline] fn kw()    -> Style { Style::default().fg(CLR_KEYWORD).add_modifier(Modifier::BOLD) }
#[inline] fn ty()    -> Style { Style::default().fg(CLR_TYPE) }
#[inline] fn str_s() -> Style { Style::default().fg(CLR_STRING) }
#[inline] fn cmt()   -> Style { Style::default().fg(CLR_COMMENT).add_modifier(Modifier::DIM) }
#[inline] fn num()   -> Style { Style::default().fg(CLR_NUMBER) }
#[inline] fn pre()   -> Style { Style::default().fg(CLR_PREPROC) }
#[inline] fn func()  -> Style { Style::default().fg(CLR_FUNC) }
#[inline] fn op()    -> Style { Style::default().fg(CLR_OPERATOR) }
#[inline] fn pl()    -> Style { Style::default().fg(CLR_PLAIN) }

// ── Language detection ────────────────────────────────────────────────────

/// Resolve `Auto` to a concrete language via the file extension.
/// Returns `None` for unknown extensions → render plain.
pub(super) fn effective_lang(mask: MaskKind, path: &Path) -> Option<MaskKind> {
    match mask {
        MaskKind::Auto => detect_lang(path),
        other => Some(other),
    }
}

fn detect_lang(path: &Path) -> Option<MaskKind> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "inl" | "c++" => MaskKind::C,
        "rs" => MaskKind::Rust,
        "js" | "ts" | "mjs" | "cjs" | "jsx" | "tsx" | "vue" => MaskKind::JavaScript,
        "py" | "pyw" | "pyi" => MaskKind::Python,
        "php" | "php3" | "php4" | "php5" | "phtml" | "phps" => MaskKind::Php,
        "html" | "htm" | "xhtml" | "xml" | "svg" | "xsl" => MaskKind::Html,
        "css" | "scss" | "less" | "sass" => MaskKind::Css,
        "sql" | "ddl" | "dml" => MaskKind::Sql,
        "sh" | "bash" | "zsh" | "fish" | "ksh" | "csh" => MaskKind::Shell,
        "pas" | "pp" | "dpr" | "lpr" => MaskKind::Pascal,
        "asm" | "s" | "a86" | "a51" => MaskKind::Assembler,
        _ => return None,
    })
}

// ── Public API ────────────────────────────────────────────────────────────

/// Highlight one (display) line.
/// `block_comment` carries multi-line block-comment state across calls.
pub(super) fn highlight_line(
    line: &str,
    lang: MaskKind,
    block_comment: &mut bool,
) -> Line<'static> {
    let spans: Vec<Span<'static>> = match lang {
        MaskKind::Auto => unreachable!("Auto must be resolved before calling highlight_line"),
        MaskKind::C => Tok::c(line).run(block_comment),
        MaskKind::Rust => tokenize_rust(line, block_comment),
        MaskKind::JavaScript => Tok::js(line).run(block_comment),
        MaskKind::Python => tokenize_python(line),
        MaskKind::Php => Tok::php(line).run(block_comment),
        MaskKind::Html => tokenize_html(line, block_comment),
        MaskKind::Css => Tok::css(line).run(block_comment),
        MaskKind::Sql => Tok::sql(line).run(block_comment),
        MaskKind::Shell => tokenize_shell(line),
        MaskKind::Pascal => tokenize_pascal(line, block_comment),
        MaskKind::Assembler => tokenize_asm(line),
        MaskKind::Ketchup => tokenize_ketchup(line),
    };
    Line::from(spans)
}

/// Advance `block_comment` state for a line WITHOUT rendering.
/// Used to pre-scan lines before the visible viewport.
pub(super) fn scan_line_state(line: &str, lang: MaskKind, bc: &mut bool) {
    match lang {
        MaskKind::Auto => {}
        MaskKind::C
        | MaskKind::Rust
        | MaskKind::JavaScript
        | MaskKind::Php
        | MaskKind::Css
        | MaskKind::Sql => scan_c_block(line, "/*", "*/", bc),
        MaskKind::Html => scan_html_block(line, bc),
        MaskKind::Pascal => scan_pascal_block(line, bc),
        // Python / Shell / Asm / Ketchup: no multi-line block comments
        _ => {}
    }
}

// ── Generic C-family tokenizer ────────────────────────────────────────────

/// Generic tokeniser for C-family languages.
struct Tok<'a> {
    src:        &'a str,
    pos:        usize,
    spans:      Vec<Span<'static>>,
    // Config
    kws:        &'static [&'static str],
    types:      &'static [&'static str],
    line_cmt:   &'static str,
    line_cmt2:  &'static str,
    blk_open:   &'static str,
    blk_close:  &'static str,
    preproc:    bool,  // '#' at line start = preprocessor (C)
    dollar_var: bool,  // $ident as variable (PHP, Shell)
    dquote:     bool,  // "string"
    squote:     bool,  // 'string' / 'char'
    backtick:   bool,  // `template` (JS)
}

impl<'a> Tok<'a> {
    // ── Language presets ──────────────────────────────────────────────────
    fn c(src: &'a str) -> Self {
        Self::new(src, C_KW, C_TY, "//", "", "/*", "*/", true, false, true, true, false)
    }
    fn js(src: &'a str) -> Self {
        Self::new(src, JS_KW, JS_TY, "//", "", "/*", "*/", false, false, true, true, true)
    }
    fn php(src: &'a str) -> Self {
        Self::new(src, PHP_KW, PHP_TY, "//", "#", "/*", "*/", false, true, true, true, false)
    }
    fn css(src: &'a str) -> Self {
        Self::new(src, CSS_KW, CSS_TY, "", "", "/*", "*/", false, false, true, true, false)
    }
    fn sql(src: &'a str) -> Self {
        Self::new(src, SQL_KW, &[], "--", "", "/*", "*/", false, false, true, true, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        src: &'a str,
        kws: &'static [&'static str],
        types: &'static [&'static str],
        line_cmt: &'static str,
        line_cmt2: &'static str,
        blk_open: &'static str,
        blk_close: &'static str,
        preproc: bool,
        dollar_var: bool,
        dquote: bool,
        squote: bool,
        backtick: bool,
    ) -> Self {
        Self {
            src, pos: 0, spans: Vec::new(),
            kws, types, line_cmt, line_cmt2,
            blk_open, blk_close,
            preproc, dollar_var, dquote, squote, backtick,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────
    fn rem(&self) -> &str { &self.src[self.pos..] }
    fn peek(&self) -> Option<char> { self.rem().chars().next() }
    fn sw(&self, s: &str) -> bool { !s.is_empty() && self.rem().starts_with(s) }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }
    fn advance_bytes(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.src.len());
    }
    fn push(&mut self, text: String, style: Style) {
        if !text.is_empty() {
            self.spans.push(Span::styled(text, style));
        }
    }

    // ── Token consumers ───────────────────────────────────────────────────
    fn eat_string(&mut self, delim: char) {
        let start = self.pos;
        self.advance(); // opening quote
        loop {
            match self.peek() {
                None | Some('\n') => break,
                Some('\\') => { self.advance(); self.advance(); }
                Some(c) if c == delim => { self.advance(); break; }
                _ => { self.advance(); }
            }
        }
        self.push(self.src[start..self.pos].to_owned(), str_s());
    }

    fn eat_number(&mut self) {
        let start = self.pos;
        if self.peek() == Some('0') {
            self.advance();
            match self.peek() {
                Some('x') | Some('X') => {
                    self.advance();
                    while matches!(self.peek(), Some('0'..='9'|'a'..='f'|'A'..='F'|'_')) { self.advance(); }
                }
                Some('b') | Some('B') => {
                    self.advance();
                    while matches!(self.peek(), Some('0'|'1'|'_')) { self.advance(); }
                }
                Some('o') | Some('O') => {
                    self.advance();
                    while matches!(self.peek(), Some('0'..='7'|'_')) { self.advance(); }
                }
                _ => self.eat_decimal_tail(),
            }
        } else {
            self.eat_decimal_tail();
        }
        self.push(self.src[start..self.pos].to_owned(), num());
    }

    fn eat_decimal_tail(&mut self) {
        while matches!(self.peek(), Some('0'..='9'|'.'|'e'|'E'|'_'|'f'|'F'|'u'|'U'|'l'|'L'|'i'|'s')) {
            self.advance();
        }
    }

    fn eat_ident(&mut self) {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            self.advance();
        }
        let token = &self.src[start..self.pos];
        let is_call = self.peek() == Some('(');
        let is_macro = self.peek() == Some('!');
        let style = if self.kws.iter().any(|k| k.eq_ignore_ascii_case(token)) {
            kw()
        } else if self.types.iter().any(|t| t.eq_ignore_ascii_case(token)) {
            ty()
        } else if is_call || is_macro {
            func()
        } else {
            pl()
        };
        self.push(token.to_owned(), style);
    }

    fn eat_block_comment(&mut self, in_block: &mut bool) {
        let start = self.pos;
        self.advance_bytes(self.blk_open.len());
        *in_block = true;
        loop {
            if self.pos >= self.src.len() { break; }
            if self.sw(self.blk_close) {
                self.advance_bytes(self.blk_close.len());
                *in_block = false;
                break;
            }
            self.advance();
        }
        self.push(self.src[start..self.pos].to_owned(), cmt());
    }

    fn continue_block_comment(&mut self, in_block: &mut bool) {
        let start = self.pos;
        loop {
            if self.pos >= self.src.len() { break; }
            if self.sw(self.blk_close) {
                self.advance_bytes(self.blk_close.len());
                *in_block = false;
                break;
            }
            self.advance();
        }
        self.push(self.src[start..self.pos].to_owned(), cmt());
    }

    // ── Main tokenisation loop ─────────────────────────────────────────────
    fn run(mut self, in_block: &mut bool) -> Vec<Span<'static>> {
        // Continue a block comment from a preceding line
        if *in_block {
            self.continue_block_comment(in_block);
            if self.pos >= self.src.len() { return self.spans; }
        }

        // C-style preprocessor: line beginning with optional whitespace then '#'
        if self.preproc {
            let rem = self.rem();
            let trimmed = rem.trim_start();
            if trimmed.starts_with('#') {
                let ws_len = rem.len() - trimmed.len();
                let ws = rem[..ws_len].to_owned();
                let rest = trimmed.to_owned();
                self.push(ws, pl());
                self.push(rest, pre());
                return self.spans;
            }
        }

        while self.pos < self.src.len() {
            // Line comments
            if self.sw(self.line_cmt) {
                self.push(self.rem().to_owned(), cmt()); return self.spans;
            }
            if !self.line_cmt2.is_empty() && self.sw(self.line_cmt2) {
                self.push(self.rem().to_owned(), cmt()); return self.spans;
            }
            // Block comment open
            if self.sw(self.blk_open) {
                self.eat_block_comment(in_block); continue;
            }

            let ch = self.peek().unwrap();

            // Dollar variable ($ident — PHP, Shell)
            if self.dollar_var && ch == '$' {
                let start = self.pos;
                self.advance();
                if matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                    while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                        self.advance();
                    }
                    self.push(self.src[start..self.pos].to_owned(), ty());
                } else {
                    self.push("$".to_owned(), op());
                }
                continue;
            }

            // String literals
            if self.dquote && ch == '"'  { self.eat_string('"');  continue; }
            if self.squote && ch == '\'' { self.eat_string('\''); continue; }
            if self.backtick && ch == '`'{ self.eat_string('`');  continue; }

            // Numbers
            if ch.is_ascii_digit() { self.eat_number(); continue; }

            // Identifiers / keywords
            if ch.is_ascii_alphabetic() || ch == '_' { self.eat_ident(); continue; }

            // Operators / punctuation
            let s = ch.to_string();
            let style = if "+-*/%=<>!&|^~?:;.,@".contains(ch) { op() } else { pl() };
            self.push(s, style);
            self.advance();
        }
        self.spans
    }
}

// ── State-only scanners (used for pre-scanning before the viewport) ───────

fn scan_c_block(line: &str, o: &str, c: &str, bc: &mut bool) {
    let mut pos = 0usize;
    if *bc {
        // consume until close
        while pos < line.len() {
            if line[pos..].starts_with(c) { pos += c.len(); *bc = false; break; }
            pos += line[pos..].chars().next().map_or(1, |ch| ch.len_utf8());
        }
        if *bc { return; }
    }
    while pos < line.len() {
        if line[pos..].starts_with(o) {
            pos += o.len(); *bc = true;
            while pos < line.len() {
                if line[pos..].starts_with(c) { pos += c.len(); *bc = false; break; }
                pos += line[pos..].chars().next().map_or(1, |ch| ch.len_utf8());
            }
            continue;
        }
        pos += line[pos..].chars().next().map_or(1, |ch| ch.len_utf8());
    }
}

fn scan_html_block(line: &str, bc: &mut bool) {
    scan_c_block(line, "<!--", "-->", bc);
}

fn scan_pascal_block(line: &str, bc: &mut bool) {
    // Pascal uses `{...}` or `(*...*)`.  We track only `{...}` here.
    scan_c_block(line, "{", "}", bc);
}

// ── Rust tokeniser ────────────────────────────────────────────────────────

fn tokenize_rust(line: &str, in_block: &mut bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;

    // Continue block comment from previous line
    if *in_block {
        let (end, closed) = consume_block_comment_end(line, 0, "/*", "*/");
        spans.push(Span::styled(line[..end].to_owned(), cmt()));
        pos = end;
        if closed { *in_block = false; } else { return spans; }
    }

    // Attribute or outer-doc-comment line: starts with `#[` or `#![`
    {
        let tr = line[pos..].trim_start();
        if tr.starts_with("#[") || tr.starts_with("#![") {
            let ws_len = line[pos..].len() - tr.len();
            if ws_len > 0 { spans.push(Span::styled(line[pos..pos+ws_len].to_owned(), pl())); }
            spans.push(Span::styled(tr.to_owned(), pre()));
            return spans;
        }
    }

    let src = line;
    while pos < src.len() {
        let rem = &src[pos..];

        // Line comment (doc comment too)
        if rem.starts_with("//") {
            spans.push(Span::styled(rem.to_owned(), cmt())); break;
        }
        // Block comment open
        if rem.starts_with("/*") {
            let (end, closed) = find_block_comment_close(&src[pos+2..], "*/");
            let abs_end = pos + 2 + end;
            spans.push(Span::styled(src[pos..abs_end].to_owned(), cmt()));
            pos = abs_end;
            if !closed { *in_block = true; break; }
            continue;
        }

        // Raw string r"..." or r#"..."#
        if rem.starts_with("r\"") || rem.starts_with("r#") {
            let start = pos;
            pos += 1; // 'r'
            let mut hashes = 0usize;
            while pos < src.len() && src.as_bytes()[pos] == b'#' { pos += 1; hashes += 1; }
            if pos < src.len() && src.as_bytes()[pos] == b'"' {
                pos += 1; // opening "
                // eat until `"` followed by `hashes` `#`
                loop {
                    if pos >= src.len() { break; }
                    if src.as_bytes()[pos] == b'"' {
                        pos += 1;
                        let mut hc = 0usize;
                        while hc < hashes && pos < src.len() && src.as_bytes()[pos] == b'#' {
                            pos += 1; hc += 1;
                        }
                        if hc == hashes { break; }
                    } else {
                        pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                    }
                }
                spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
                continue;
            }
            // Not a valid raw string — back-track and push 'r' as identifier
            pos = start;
        }

        let ch = rem.chars().next().unwrap();

        // Double-quoted string
        if ch == '"' {
            let (text, new_pos) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(text, str_s())); pos = new_pos; continue;
        }
        // Single quote: lifetime vs char literal
        if ch == '\'' {
            let start = pos; pos += 1; // '
            match src[pos..].chars().next() {
                Some('\\') => {
                    // Char literal with escape
                    pos += 1;
                    pos += src[pos..].chars().next().map_or(0, |c| c.len_utf8());
                    if pos < src.len() && src.as_bytes()[pos] == b'\'' { pos += 1; }
                    spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
                }
                Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                    let id_start = pos;
                    while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                        pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                    }
                    if pos < src.len() && src.as_bytes()[pos] == b'\'' {
                        // char literal: 'x'
                        pos += 1;
                        spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
                    } else {
                        // lifetime: 'a
                        spans.push(Span::styled("'".to_owned(), ty()));
                        let id_tok = &src[id_start..pos];
                        if RUST_KW.iter().any(|k| *k == id_tok) {
                            spans.push(Span::styled(id_tok.to_owned(), kw()));
                        } else {
                            spans.push(Span::styled(id_tok.to_owned(), ty()));
                        }
                    }
                }
                _ => { spans.push(Span::styled("'".to_owned(), pl())); }
            }
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let (text, new_pos) = eat_number_from(src, pos);
            spans.push(Span::styled(text, num())); pos = new_pos; continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next = src[pos..].chars().next();
            let style = if RUST_KW.iter().any(|k| *k == token) {
                kw()
            } else if RUST_TY.iter().any(|t| *t == token) {
                ty()
            } else if next == Some('!') {
                func() // macro call: println! vec! etc.
            } else if next == Some('(') || next == Some('<') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        // Operators / punctuation
        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,@#".contains(ch) { op() } else { pl() };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── Python tokeniser ──────────────────────────────────────────────────────

fn tokenize_python(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let src = line;

    while pos < src.len() {
        let rem = &src[pos..];

        // Comment
        if rem.starts_with('#') {
            spans.push(Span::styled(rem.to_owned(), cmt())); break;
        }

        let ch = rem.chars().next().unwrap();

        // Triple-quoted string (simplified: consume whole line as string start)
        if rem.starts_with("\"\"\"") || rem.starts_with("'''") {
            let delim = &rem[..3];
            let start = pos; pos += 3;
            if let Some(end_off) = src[pos..].find(delim) {
                pos += end_off + 3;
            } else {
                // Spans to end of line (multiline — no state tracking in this simplified version)
                pos = src.len();
            }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s())); continue;
        }
        // String literals
        if ch == '"' {
            let (text, new_pos) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(text, str_s())); pos = new_pos; continue;
        }
        if ch == '\'' {
            let (text, new_pos) = eat_string_from(src, pos, '\'');
            spans.push(Span::styled(text, str_s())); pos = new_pos; continue;
        }
        // Numbers
        if ch.is_ascii_digit() {
            let (text, new_pos) = eat_number_from(src, pos);
            spans.push(Span::styled(text, num())); pos = new_pos; continue;
        }
        // Decorator
        if ch == '@' {
            let start = pos; pos += 1;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), pre())); continue;
        }
        // Identifiers
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next = src[pos..].chars().next();
            let style = if PY_KW.iter().any(|k| *k == token) {
                kw()
            } else if PY_TY.iter().any(|t| *t == token) {
                ty()
            } else if next == Some('(') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style)); continue;
        }
        // Operators
        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,".contains(ch) { op() } else { pl() };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── Shell tokeniser ───────────────────────────────────────────────────────

fn tokenize_shell(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let src = line;

    while pos < src.len() {
        let rem = &src[pos..];

        // Comment
        if rem.starts_with('#') {
            spans.push(Span::styled(rem.to_owned(), cmt())); break;
        }

        let ch = rem.chars().next().unwrap();

        // Strings
        if ch == '"'  { let (t, p) = eat_string_from(src, pos, '"');  spans.push(Span::styled(t, str_s())); pos = p; continue; }
        if ch == '\'' { let (t, p) = eat_string_from(src, pos, '\''); spans.push(Span::styled(t, str_s())); pos = p; continue; }
        if ch == '`'  { let (t, p) = eat_string_from(src, pos, '`');  spans.push(Span::styled(t, str_s())); pos = p; continue; }

        // Variable $VAR or ${VAR}
        if ch == '$' {
            let start = pos; pos += 1;
            if src[pos..].starts_with('{') {
                pos += 1;
                while pos < src.len() && src.as_bytes()[pos] != b'}' { pos += 1; }
                if pos < src.len() { pos += 1; }
                spans.push(Span::styled(src[start..pos].to_owned(), ty())); continue;
            }
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), ty())); continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let (t, p) = eat_number_from(src, pos);
            spans.push(Span::styled(t, num())); pos = p; continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let style = if SH_KW.iter().any(|k| k.eq_ignore_ascii_case(token)) { kw() } else { pl() };
            spans.push(Span::styled(token.to_owned(), style)); continue;
        }

        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,".contains(ch) { op() } else { pl() };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── Pascal tokeniser ──────────────────────────────────────────────────────

fn tokenize_pascal(line: &str, in_block: &mut bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let src = line;

    // Continue block comment `{...}` from previous line
    if *in_block {
        let start = pos;
        while pos < src.len() {
            if src[pos..].starts_with('}') { pos += 1; *in_block = false; break; }
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
        spans.push(Span::styled(src[start..pos].to_owned(), cmt()));
        if *in_block { return spans; }
    }

    while pos < src.len() {
        let rem = &src[pos..];

        // Line comment //
        if rem.starts_with("//") {
            spans.push(Span::styled(rem.to_owned(), cmt())); break;
        }
        // Block comment (* ... *)
        if rem.starts_with("(*") {
            let start = pos; pos += 2;
            let mut found = false;
            while pos + 1 < src.len() {
                if src[pos..].starts_with("*)") { pos += 2; found = true; break; }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if !found && pos + 1 >= src.len() { pos = src.len(); }
            spans.push(Span::styled(src[start..pos].to_owned(), cmt())); continue;
        }
        // Block comment { ... }
        if rem.starts_with('{') {
            let start = pos; pos += 1; *in_block = true;
            while pos < src.len() {
                if src[pos..].starts_with('}') { pos += 1; *in_block = false; break; }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), cmt())); continue;
        }

        let ch = rem.chars().next().unwrap();

        // String: 'hello''world' (Pascal string uses '' as escape for ' inside)
        if ch == '\'' {
            let start = pos; pos += 1;
            loop {
                if pos >= src.len() { break; }
                if src.as_bytes()[pos] == b'\'' {
                    pos += 1;
                    // doubled quote inside string: ''
                    if pos < src.len() && src.as_bytes()[pos] == b'\'' { pos += 1; continue; }
                    break;
                }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s())); continue;
        }

        // Numbers
        if ch.is_ascii_digit() || (ch == '$' && matches!(src[pos+1..].chars().next(), Some('0'..='9'|'A'..='F'|'a'..='f'))) {
            let start = pos;
            if ch == '$' { pos += 1; } // hex prefix $FF
            while matches!(src[pos..].chars().next(), Some('0'..='9'|'a'..='f'|'A'..='F'|'.'|'e'|'E')) {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), num())); continue;
        }

        // Identifiers
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next = src[pos..].chars().next();
            let style = if PAS_KW.iter().any(|k| k.eq_ignore_ascii_case(token)) {
                kw()
            } else if PAS_TY.iter().any(|t| t.eq_ignore_ascii_case(token)) {
                ty()
            } else if next == Some('(') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style)); continue;
        }

        let s = ch.to_string();
        let style = if "+-*/:=<>@^.,;()[]".contains(ch) { op() } else { pl() };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── Assembler tokeniser ───────────────────────────────────────────────────

fn tokenize_asm(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let src = line;

    while pos < src.len() {
        let rem = &src[pos..];

        // Comment: ; or //
        if rem.starts_with(';') || rem.starts_with("//") {
            spans.push(Span::styled(rem.to_owned(), cmt())); break;
        }

        let ch = rem.chars().next().unwrap();

        // String
        if ch == '"'  { let (t, p) = eat_string_from(src, pos, '"');  spans.push(Span::styled(t, str_s())); pos = p; continue; }
        if ch == '\'' { let (t, p) = eat_string_from(src, pos, '\''); spans.push(Span::styled(t, str_s())); pos = p; continue; }

        // Hex literals: 0x... or ...h
        if ch.is_ascii_digit() || (ch == '$' && matches!(src[pos+1..].chars().next(), Some('0'..='9'|'A'..='F'|'a'..='f'))) {
            let (t, p) = eat_number_from(src, pos);
            spans.push(Span::styled(t, num())); pos = p; continue;
        }

        // Identifiers / instructions / registers
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '.' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let style = if ASM_KW.iter().any(|k| k.eq_ignore_ascii_case(token)) {
                kw()
            } else if ASM_REG.iter().any(|r| r.eq_ignore_ascii_case(token)) {
                ty()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style)); continue;
        }

        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~:,[]()".contains(ch) { op() } else { pl() };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── HTML tokeniser ────────────────────────────────────────────────────────

fn tokenize_html(line: &str, in_block: &mut bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let src = line;

    // Continue HTML comment <!-- ... -->
    if *in_block {
        let (end, closed) = consume_block_comment_end(src, 0, "<!--", "-->");
        spans.push(Span::styled(src[..end].to_owned(), cmt()));
        pos = end;
        if closed { *in_block = false; } else { return spans; }
    }

    while pos < src.len() {
        let rem = &src[pos..];

        // HTML comment <!-- ... -->
        if rem.starts_with("<!--") {
            let (end, closed) = find_block_comment_close(&src[pos+4..], "-->");
            let abs_end = pos + 4 + end;
            spans.push(Span::styled(src[pos..abs_end].to_owned(), cmt()));
            pos = abs_end;
            if !closed { *in_block = true; break; }
            continue;
        }

        // HTML tag <tag> or </tag> or <tag ...attr...>
        if rem.starts_with('<') {
            let start = pos; pos += 1;
            // Consume tag: either in_tag state or self-close
            let style_bracket = op();
            // Emit '<'
            spans.push(Span::styled("<".to_owned(), style_bracket.clone()));

            // Optional '/' for close tag
            if pos < src.len() && src.as_bytes()[pos] == b'/' {
                spans.push(Span::styled("/".to_owned(), op())); pos += 1;
            }
            // Tag name
            let tag_start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_') {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if pos > tag_start {
                spans.push(Span::styled(src[tag_start..pos].to_owned(), kw()));
            }
            // Attributes and closing >
            while pos < src.len() {
                let ch = src[pos..].chars().next().unwrap();
                if ch == '>' {
                    spans.push(Span::styled(">".to_owned(), op())); pos += 1; break;
                }
                if ch == '/' && src[pos+1..].starts_with('>') {
                    spans.push(Span::styled("/>".to_owned(), op())); pos += 2; break;
                }
                // Attribute name
                if ch.is_ascii_alphabetic() || ch == '_' || ch == ':' {
                    let a_start = pos;
                    while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.') {
                        pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                    }
                    spans.push(Span::styled(src[a_start..pos].to_owned(), ty()));
                    continue;
                }
                // Attribute value
                if (ch == '"' || ch == '\'') {
                    let (t, p) = eat_string_from(src, pos, ch);
                    spans.push(Span::styled(t, str_s())); pos = p; continue;
                }
                // = and whitespace
                let s = ch.to_string();
                let style = if ch == '=' { op() } else { pl() };
                spans.push(Span::styled(s, style));
                pos += ch.len_utf8();
            }
            let _ = start;
            continue;
        }

        // HTML entity &amp; etc.
        if rem.starts_with('&') {
            let start = pos; pos += 1;
            while pos < src.len() && src.as_bytes()[pos] != b';' && src.as_bytes()[pos] != b' ' {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if pos < src.len() && src.as_bytes()[pos] == b';' { pos += 1; }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s())); continue;
        }

        // Text content: emit as plain up to next '<' or '&'
        let ch = src[pos..].chars().next().unwrap();
        spans.push(Span::styled(ch.to_string(), pl()));
        pos += ch.len_utf8();
    }
    spans
}

// ── Ketchup tokeniser  (legacy word-highlight) ────────────────────────────

fn tokenize_ketchup(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            let start = i; i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
            let token: String = chars[start..i].iter().collect();
            let style = if KETCHUP_KW.iter().any(|kw| kw.eq_ignore_ascii_case(&token)) {
                Style::default().fg(CLR_KETCHUP).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(token, style));
        } else {
            spans.push(Span::styled(ch.to_string(), Style::default().fg(Color::White)));
            i += 1;
        }
    }
    spans
}

// ── Shared tokeniser helpers ──────────────────────────────────────────────

/// Eat a simple quoted string with backslash escaping.
/// Returns (owned text, new pos).
fn eat_string_from(src: &str, start: usize, delim: char) -> (String, usize) {
    let mut pos = start + delim.len_utf8(); // skip opening quote
    loop {
        match src[pos..].chars().next() {
            None | Some('\n') => break,
            Some('\\') => {
                pos += 1;
                pos += src[pos..].chars().next().map_or(0, |c| c.len_utf8());
            }
            Some(c) if c == delim => { pos += delim.len_utf8(); break; }
            Some(c) => { pos += c.len_utf8(); }
        }
    }
    (src[start..pos].to_owned(), pos)
}

/// Eat a number literal from `pos`.
/// Returns (owned text, new pos).
fn eat_number_from(src: &str, start: usize) -> (String, usize) {
    let mut pos = start;
    if src[pos..].starts_with("0x") || src[pos..].starts_with("0X") {
        pos += 2;
        while matches!(src[pos..].chars().next(), Some('0'..='9'|'a'..='f'|'A'..='F'|'_')) {
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    } else if src[pos..].starts_with("0b") || src[pos..].starts_with("0B") {
        pos += 2;
        while matches!(src[pos..].chars().next(), Some('0'|'1'|'_')) {
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    } else {
        while matches!(src[pos..].chars().next(), Some('0'..='9'|'.'|'e'|'E'|'_'|'f'|'F'|'u'|'U'|'l'|'L'|'i'|'s')) {
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    }
    (src[start..pos].to_owned(), pos)
}

/// Find the end of a block comment starting AFTER the opening delimiter.
/// Returns (bytes consumed including close delimiter, found_close).
fn find_block_comment_close(src: &str, close: &str) -> (usize, bool) {
    if let Some(idx) = src.find(close) {
        (idx + close.len(), true)
    } else {
        (src.len(), false)
    }
}

/// Consume from `pos` through a block-comment close delimiter.
/// If `in_block` was true at entry, we're already inside; `open` is unused.
/// Returns (new_pos, found_close).
fn consume_block_comment_end(src: &str, pos: usize, _open: &str, close: &str) -> (usize, bool) {
    let (consumed, found) = find_block_comment_close(&src[pos..], close);
    (pos + consumed, found)
}

// ── Keyword / type tables ─────────────────────────────────────────────────

static C_KW: &[&str] = &[
    "asm", "auto", "break", "case", "char", "const", "continue", "default", "do",
    "double", "else", "enum", "extern", "float", "for", "goto", "if", "inline",
    "int", "long", "register", "restrict", "return", "short", "signed", "sizeof",
    "static", "struct", "switch", "typedef", "union", "unsigned", "void", "volatile",
    "while", "_Bool", "_Complex", "_Imaginary", "nullptr", "constexpr", "thread_local",
    "static_assert", "noreturn", "alignas", "alignof",
];
static C_TY: &[&str] = &[
    "bool", "size_t", "ssize_t", "ptrdiff_t", "intptr_t", "uintptr_t",
    "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t",
    "int_fast8_t", "int_fast16_t", "int_fast32_t", "int_fast64_t",
    "uint_fast8_t", "uint_fast16_t", "uint_fast32_t", "uint_fast64_t",
    "int_least8_t", "int_least16_t", "int_least32_t", "int_least64_t",
    "uint_least8_t", "uint_least16_t", "uint_least32_t", "uint_least64_t",
    "intmax_t", "uintmax_t", "wchar_t", "FILE", "NULL", "true", "false",
    "string", "vector", "map", "set", "pair", "optional", "variant",  // C++ stdlib
    "shared_ptr", "unique_ptr", "weak_ptr",
];

static RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
    "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "union", "unsafe",
    "use", "where", "while", "yield", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual",
];
static RUST_TY: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64", "bool", "char", "str",
    "String", "Vec", "Box", "Arc", "Rc", "Mutex", "RwLock",
    "Option", "Result", "Some", "None", "Ok", "Err",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque", "BinaryHeap",
    "Cow", "Cell", "RefCell", "Pin", "PhantomData",
    "PathBuf", "Path", "OsStr", "OsString",
];

static JS_KW: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default",
    "delete", "do", "else", "export", "extends", "finally", "for", "function",
    "if", "import", "in", "instanceof", "let", "new", "of", "return", "switch",
    "this", "throw", "try", "typeof", "undefined", "var", "void", "while", "with",
    "yield", "async", "await", "from", "as", "static", "super",
];
static JS_TY: &[&str] = &[
    "true", "false", "null", "NaN", "Infinity", "globalThis",
    "Array", "Object", "String", "Number", "Boolean", "Function", "Symbol",
    "Promise", "Error", "TypeError", "RangeError", "SyntaxError",
    "Map", "Set", "WeakMap", "WeakSet",
    "Date", "RegExp", "JSON", "Math",
    "console", "document", "window", "navigator", "location",
    "parseInt", "parseFloat", "isNaN", "isFinite",
    "setTimeout", "setInterval", "clearTimeout", "clearInterval",
    "fetch", "URL", "URLSearchParams", "FormData", "Headers",
];

static PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "False", "finally", "for",
    "from", "global", "if", "import", "in", "is", "lambda", "None",
    "nonlocal", "not", "or", "pass", "raise", "return", "True", "try",
    "while", "with", "yield",
];
static PY_TY: &[&str] = &[
    "int", "float", "str", "bool", "bytes", "list", "dict", "tuple", "set",
    "frozenset", "bytearray", "memoryview", "complex",
    "object", "type", "super",
    "print", "len", "range", "enumerate", "zip", "map", "filter",
    "sorted", "reversed", "any", "all", "sum", "min", "max", "abs",
    "isinstance", "issubclass", "hasattr", "getattr", "setattr", "delattr",
    "open", "input", "repr", "id", "hash", "vars", "dir",
    "property", "staticmethod", "classmethod",
    "Exception", "ValueError", "TypeError", "KeyError", "IndexError",
    "AttributeError", "RuntimeError", "StopIteration", "OSError",
    "Optional", "List", "Dict", "Tuple", "Set", "Union", "Any",
];

static PHP_KW: &[&str] = &[
    "abstract", "and", "array", "as", "break", "callable", "case", "catch",
    "class", "clone", "const", "continue", "declare", "default", "do", "echo",
    "else", "elseif", "empty", "enddeclare", "endfor", "endforeach", "endif",
    "endswitch", "endwhile", "enum", "extends", "final", "finally", "fn",
    "for", "foreach", "function", "global", "goto", "if", "implements",
    "include", "include_once", "instanceof", "interface", "isset", "list",
    "match", "namespace", "new", "or", "print", "require", "require_once",
    "return", "static", "switch", "throw", "trait", "try", "unset", "use",
    "var", "while", "xor", "yield",
];
static PHP_TY: &[&str] = &[
    "null", "true", "false", "NULL", "TRUE", "FALSE",
    "int", "float", "string", "bool", "array", "object", "void", "mixed",
    "self", "parent", "static",
    "Exception", "Error", "Throwable", "Iterator", "Countable", "Closure",
];

static CSS_KW: &[&str] = &[
    "important", "inherit", "initial", "unset", "revert",
    "none", "auto", "normal", "bold", "italic", "underline", "block",
    "inline", "flex", "grid", "relative", "absolute", "fixed", "sticky",
    "center", "left", "right", "top", "bottom", "solid", "dashed", "dotted",
    "hidden", "visible", "scroll", "clip", "ellipsis",
    "color", "background", "border", "margin", "padding", "font", "text",
    "width", "height", "display", "position", "float", "clear",
    "overflow", "opacity", "transform", "transition", "animation",
    "cursor", "pointer", "default",
];
static CSS_TY: &[&str] = &[
    "px", "em", "rem", "vh", "vw", "vmin", "vmax", "pt", "pc", "cm", "mm",
    "rgb", "rgba", "hsl", "hsla", "var", "calc", "url", "linear-gradient",
    "radial-gradient",
];

static SQL_KW: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
    "DELETE", "CREATE", "DROP", "ALTER", "TABLE", "VIEW", "INDEX", "DATABASE",
    "SCHEMA", "COLUMN", "CONSTRAINT", "JOIN", "INNER", "LEFT", "RIGHT",
    "FULL", "OUTER", "CROSS", "ON", "AND", "OR", "NOT", "IN", "LIKE",
    "BETWEEN", "IS", "NULL", "AS", "WITH", "CASE", "WHEN", "THEN", "ELSE",
    "END", "ORDER", "BY", "GROUP", "HAVING", "DISTINCT", "UNION", "ALL",
    "EXISTS", "LIMIT", "OFFSET", "RETURNING", "TRIGGER", "FUNCTION",
    "PROCEDURE", "BEGIN", "COMMIT", "ROLLBACK", "TRANSACTION", "PRIMARY",
    "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "CHECK", "DEFAULT",
    "AUTO_INCREMENT", "SERIAL", "TRUNCATE", "REPLACE", "EXPLAIN",
    "select", "from", "where", "insert", "into", "values", "update", "set",
    "delete", "create", "drop", "alter", "table", "view", "index",
    "join", "inner", "left", "right", "full", "outer", "cross", "on",
    "and", "or", "not", "in", "like", "between", "is", "null", "as",
    "with", "case", "when", "then", "else", "end", "order", "by", "group",
    "having", "distinct", "union", "all", "exists", "limit", "offset",
    "primary", "key", "foreign", "references", "unique", "check", "default",
];

static SH_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "do", "done", "while",
    "until", "case", "esac", "in", "function", "return", "exit",
    "break", "continue", "local", "readonly", "export", "unset",
    "eval", "exec", "source", "alias", "declare", "typeset",
    "echo", "printf", "read", "true", "false", "test", "shift",
    "set", "unset", "trap", "wait", "kill", "jobs", "fg", "bg",
];

static PAS_KW: &[&str] = &[
    "absolute", "and", "array", "asm", "begin", "break", "case", "class",
    "const", "constructor", "continue", "destructor", "div", "do",
    "downto", "else", "end", "except", "exit", "exports", "file",
    "finalization", "finally", "for", "forward", "function", "goto", "if",
    "implementation", "in", "inherited", "initialization", "inline",
    "interface", "label", "library", "mod", "nil", "not", "object",
    "of", "on", "operator", "or", "packed", "procedure", "program",
    "property", "raise", "record", "repeat", "resourcestring", "self",
    "set", "shl", "shr", "string", "then", "threadvar", "to", "try",
    "type", "unit", "until", "uses", "var", "virtual", "while", "with", "xor",
];
static PAS_TY: &[&str] = &[
    "integer", "cardinal", "word", "byte", "shortint", "smallint", "longint",
    "int64", "qword", "dword", "real", "single", "double", "extended",
    "boolean", "char", "widechar", "pchar", "pwidechar",
    "ansistring", "widestring", "unicodestring", "shortstring", "pointer",
    "variant", "olevariant",
];

static ASM_KW: &[&str] = &[
    "mov", "movs", "movz", "push", "pop", "pusha", "popa", "pushad", "popad",
    "call", "ret", "retn", "retf", "leave", "enter",
    "cmp", "test", "jmp", "je", "jne", "jz", "jnz", "ja", "jb", "jg", "jl",
    "jae", "jbe", "jge", "jle", "jo", "jno", "js", "jns", "jc", "jnc",
    "add", "sub", "mul", "imul", "div", "idiv", "inc", "dec", "neg",
    "xor", "or", "and", "not", "shl", "shr", "sar", "sal", "rol", "ror",
    "lea", "int", "nop", "hlt", "cli", "sti", "clc", "stc", "cld", "std",
    "db", "dw", "dd", "dq", "dt", "resb", "resw", "resd", "resq",
    "equ", "org", "align", "section", "segment", "proc", "endp", "macro",
    "endm", "local", "global", "extern", "public", "extrn",
    "assume", "end", "ends", "xlatb", "xlat",
    "loop", "loope", "loopne", "rep", "repe", "repne", "repnz", "repz",
];
static ASM_REG: &[&str] = &[
    "al", "bl", "cl", "dl", "ah", "bh", "ch", "dh",
    "ax", "bx", "cx", "dx", "si", "di", "sp", "bp",
    "eax", "ebx", "ecx", "edx", "esi", "edi", "esp", "ebp",
    "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rsp", "rbp",
    "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
    "r8d", "r9d", "r10d", "r11d", "r12d", "r13d", "r14d", "r15d",
    "r8w", "r9w", "r10w", "r11w", "r12w", "r13w", "r14w", "r15w",
    "r8b", "r9b", "r10b", "r11b", "r12b", "r13b", "r14b", "r15b",
    "cs", "ds", "es", "fs", "gs", "ss",
    "cr0", "cr2", "cr3", "cr4", "dr0", "dr1", "dr2", "dr3", "dr6", "dr7",
    "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
    "ymm0", "ymm1", "ymm2", "ymm3", "ymm4", "ymm5", "ymm6", "ymm7",
];

static KETCHUP_KW: &[&str] = &[
    "blackward", "ketchup", "killers", "redbug", "access", "darkangel",
    "off", "topy", "kennet", "typeone", "pulpe", "tyby", "djamm", "vatin",
    "marjorie", "katana", "ecstasy", "cray", "magicfred", "cobra", "z",
];
