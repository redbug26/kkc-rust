//! Pure in-process syntax highlighting for the file viewer.
//!
//! Architecture inspired by highlight.js:
//!   · Keyword categories: keyword · built_in · literal · type · variable.language
//!   · title.function  — identifier immediately after fn / def / function / procedure
//!   · title.class     — identifier immediately after class / struct / enum / trait / type / impl
//!   · meta            — preprocessor directives, decorators, Rust attributes
//!   · string · number · comment · operator

use super::MaskKind;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

// ── Token colour palette  (VS Code Dark+ inspired) ────────────────────────────
const CLR_KEYWORD: Color = Color::Rgb(86, 156, 214); // blue       — keyword / literal
const CLR_TYPE: Color = Color::Rgb(78, 201, 176); // teal       — type / title.class
const CLR_STRING: Color = Color::Rgb(206, 145, 120); // salmon     — string
const CLR_COMMENT: Color = Color::Rgb(106, 153, 85); // green      — comment
const CLR_NUMBER: Color = Color::Rgb(181, 206, 168); // pale green — number
const CLR_PREPROC: Color = Color::Rgb(197, 134, 192); // violet     — meta / preprocessor
const CLR_FUNC: Color = Color::Rgb(220, 220, 170); // pale yel.  — title.function / built_in
const CLR_OPERATOR: Color = Color::Rgb(180, 200, 240); // lt blue-gr — operator
const CLR_PLAIN: Color = Color::Rgb(212, 212, 212); // light gray — plain text
const CLR_VAR_LANG: Color = Color::Rgb(156, 220, 254); // light cyan — variable.language
const CLR_KETCHUP: Color = Color::Yellow;

#[inline]
fn kw() -> Style {
    Style::default()
        .fg(CLR_KEYWORD)
        .add_modifier(Modifier::BOLD)
}
#[inline]
fn ty() -> Style {
    Style::default().fg(CLR_TYPE)
}
#[inline]
fn str_s() -> Style {
    Style::default().fg(CLR_STRING)
}
#[inline]
fn cmt() -> Style {
    Style::default().fg(CLR_COMMENT).add_modifier(Modifier::DIM)
}
#[inline]
fn num() -> Style {
    Style::default().fg(CLR_NUMBER)
}
#[inline]
fn pre() -> Style {
    Style::default().fg(CLR_PREPROC)
}
#[inline]
fn func() -> Style {
    Style::default().fg(CLR_FUNC)
}
#[inline]
fn op() -> Style {
    Style::default().fg(CLR_OPERATOR)
}
#[inline]
fn pl() -> Style {
    Style::default().fg(CLR_PLAIN)
}
#[inline]
fn var_lang() -> Style {
    Style::default().fg(CLR_VAR_LANG)
}

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

// ── Generic C-family tokenizer ─────────────────────────────────────────────────
//
// Implements highlight.js-style keyword categories:
//   keyword · built_in · literal · type · variable.language
//   title.function (ident after fn_decl_kws) · title.class (ident after class_decl_kws)

struct Tok<'a> {
    src: &'a str,
    pos: usize,
    spans: Vec<Span<'static>>,
    // Keyword category tables (hl.js-inspired)
    kws: &'static [&'static str], // keyword — control flow, declarations
    builtins: &'static [&'static str], // built_in — stdlib / standard functions
    literals: &'static [&'static str], // literal — true, false, null, None …
    types: &'static [&'static str], // type names
    var_langs: &'static [&'static str], // variable.language — this, super, self …
    fn_decl_kws: &'static [&'static str], // after these: next ident = title.function
    class_decl_kws: &'static [&'static str], // after these: next ident = title.class
    // Input config
    line_cmt: &'static str,
    line_cmt2: &'static str,
    blk_open: &'static str,
    blk_close: &'static str,
    preproc: bool,    // '#' at line start = meta (preprocessor)
    dollar_var: bool, // $ident as variable.language (PHP)
    dquote: bool,     // "string"
    squote: bool,     // 'string' / 'char'
    backtick: bool,   // `template literal` (JS)
    // Runtime state
    next_ident_style: Option<Style>, // pending title.function / title.class for next ident
}

impl<'a> Tok<'a> {
    // ── Base defaults ─────────────────────────────────────────────────────
    fn base(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            spans: Vec::new(),
            kws: &[],
            builtins: &[],
            literals: &[],
            types: &[],
            var_langs: &[],
            fn_decl_kws: &[],
            class_decl_kws: &[],
            line_cmt: "",
            line_cmt2: "",
            blk_open: "/*",
            blk_close: "*/",
            preproc: false,
            dollar_var: false,
            dquote: true,
            squote: true,
            backtick: false,
            next_ident_style: None,
        }
    }

    // ── Language presets ──────────────────────────────────────────────────
    fn c(src: &'a str) -> Self {
        Self {
            kws: C_KW,
            types: C_TY,
            literals: C_LIT,
            builtins: C_BUILTIN,
            line_cmt: "//",
            preproc: true,
            ..Self::base(src)
        }
    }
    fn js(src: &'a str) -> Self {
        Self {
            kws: JS_KW,
            types: JS_TY,
            literals: JS_LIT,
            builtins: JS_BUILTIN,
            var_langs: JS_VAR_LANG,
            fn_decl_kws: JS_FN_DECL,
            class_decl_kws: JS_CLASS_DECL,
            line_cmt: "//",
            backtick: true,
            ..Self::base(src)
        }
    }
    fn php(src: &'a str) -> Self {
        Self {
            kws: PHP_KW,
            types: PHP_TY,
            literals: PHP_LIT,
            fn_decl_kws: PHP_FN_DECL,
            class_decl_kws: PHP_CLASS_DECL,
            line_cmt: "//",
            line_cmt2: "#",
            dollar_var: true,
            ..Self::base(src)
        }
    }
    fn css(src: &'a str) -> Self {
        Self {
            kws: CSS_KW,
            types: CSS_TY,
            ..Self::base(src)
        }
    }
    fn sql(src: &'a str) -> Self {
        Self {
            kws: SQL_KW,
            line_cmt: "--",
            ..Self::base(src)
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────
    fn rem(&self) -> &str {
        &self.src[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.rem().chars().next()
    }
    fn sw(&self, s: &str) -> bool {
        !s.is_empty() && self.rem().starts_with(s)
    }

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
                Some('\\') => {
                    self.advance();
                    self.advance();
                }
                Some(c) if c == delim => {
                    self.advance();
                    break;
                }
                _ => {
                    self.advance();
                }
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
                    while matches!(self.peek(), Some('0'..='9' | 'a'..='f' | 'A'..='F' | '_')) {
                        self.advance();
                    }
                }
                Some('b') | Some('B') => {
                    self.advance();
                    while matches!(self.peek(), Some('0' | '1' | '_')) {
                        self.advance();
                    }
                }
                Some('o') | Some('O') => {
                    self.advance();
                    while matches!(self.peek(), Some('0'..='7' | '_')) {
                        self.advance();
                    }
                }
                _ => self.eat_decimal_tail(),
            }
        } else {
            self.eat_decimal_tail();
        }
        self.push(self.src[start..self.pos].to_owned(), num());
    }

    fn eat_decimal_tail(&mut self) {
        while matches!(self.peek(), Some('0'..='9' | '_')) {
            self.advance();
        }
        if self.peek() == Some('.') {
            let next2 = {
                let mut it = self.src[self.pos + 1..].chars();
                it.next()
            };
            if matches!(next2, Some('0'..='9') | None) {
                self.advance();
                while matches!(self.peek(), Some('0'..='9' | '_')) {
                    self.advance();
                }
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.advance();
            if matches!(self.peek(), Some('+' | '-')) {
                self.advance();
            }
            while matches!(self.peek(), Some('0'..='9' | '_')) {
                self.advance();
            }
        }
        // type suffix
        if matches!(
            self.peek(),
            Some('u' | 'i' | 'f' | 'U' | 'I' | 'F' | 'l' | 'L')
        ) {
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric()) {
                self.advance();
            }
        }
    }

    /// Eat an identifier — applies highlight.js-inspired category lookup.
    ///
    /// Priority (mirrors hl.js):
    ///   1. pending title.function / title.class (set by previous declaration keyword)
    ///   2. variable.language  (this, super, self …)
    ///   3. literal            (true, false, null …)  → keyword colour
    ///   4. keyword            → also sets next_ident_style for fn/class decl kws
    ///   5. type
    ///   6. built_in           → pale-yellow (same as title.function)
    ///   7. call / macro site  → pale-yellow
    ///   8. plain
    fn eat_ident(&mut self) {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            self.advance();
        }
        let token = &self.src[start..self.pos];
        let is_call = self.peek() == Some('(');
        let is_macro = self.peek() == Some('!');

        let style = if let Some(s) = self.next_ident_style.take() {
            s
        } else if self.var_langs.iter().any(|v| *v == token) {
            var_lang()
        } else if self.literals.iter().any(|l| l.eq_ignore_ascii_case(token)) {
            kw()
        } else if self.kws.iter().any(|k| k.eq_ignore_ascii_case(token)) {
            if self.fn_decl_kws.iter().any(|k| *k == token) {
                self.next_ident_style = Some(func());
            } else if self.class_decl_kws.iter().any(|k| *k == token) {
                self.next_ident_style = Some(ty());
            }
            kw()
        } else if self.types.iter().any(|t| t.eq_ignore_ascii_case(token)) {
            ty()
        } else if self.builtins.iter().any(|b| b.eq_ignore_ascii_case(token)) {
            func()
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
            if self.pos >= self.src.len() {
                break;
            }
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
            if self.pos >= self.src.len() {
                break;
            }
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
            if self.pos >= self.src.len() {
                return self.spans;
            }
        }

        // C-style preprocessor (#include, #define …) — entire line is meta colour
        if self.preproc {
            let rem = self.rem().to_owned();
            let trimmed_start = rem.len() - rem.trim_start().len();
            if rem.trim_start().starts_with('#') {
                let ws_len = trimmed_start;
                if ws_len > 0 {
                    self.push(rem[..ws_len].to_owned(), pl());
                }
                self.push(rem[ws_len..].to_owned(), pre());
                return self.spans;
            }
        }

        while self.pos < self.src.len() {
            // Line comments
            if !self.line_cmt.is_empty() && self.sw(self.line_cmt) {
                self.push(self.rem().to_owned(), cmt());
                return self.spans;
            }
            if !self.line_cmt2.is_empty() && self.sw(self.line_cmt2) {
                self.push(self.rem().to_owned(), cmt());
                return self.spans;
            }
            // Block comment open
            if !self.blk_open.is_empty() && self.sw(self.blk_open) {
                self.eat_block_comment(in_block);
                continue;
            }

            let ch = self.peek().unwrap();

            // Cancel pending fn/class name on '(' or ';' or '{'
            if matches!(ch, '(' | ';' | '{') {
                self.next_ident_style = None;
            }

            // Dollar variable ($ident — PHP): variable.language colour
            if self.dollar_var && ch == '$' {
                let start = self.pos;
                self.advance();
                if matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                    while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                        self.advance();
                    }
                    self.push(self.src[start..self.pos].to_owned(), var_lang());
                } else {
                    self.push("$".to_owned(), op());
                }
                continue;
            }

            // String literals
            if self.dquote && ch == '"' {
                self.eat_string('"');
                continue;
            }
            if self.squote && ch == '\'' {
                self.eat_string('\'');
                continue;
            }
            if self.backtick && ch == '`' {
                self.eat_string('`');
                continue;
            }

            // Numbers
            if ch.is_ascii_digit() {
                self.eat_number();
                continue;
            }

            // Identifiers / keywords (all categories)
            if ch.is_ascii_alphabetic() || ch == '_' {
                self.eat_ident();
                continue;
            }

            // Operators / punctuation
            let s = ch.to_string();
            let style = if "+-*/%=<>!&|^~?:;.,@".contains(ch) {
                op()
            } else {
                pl()
            };
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
            if line[pos..].starts_with(c) {
                pos += c.len();
                *bc = false;
                break;
            }
            pos += line[pos..].chars().next().map_or(1, |ch| ch.len_utf8());
        }
        if *bc {
            return;
        }
    }
    while pos < line.len() {
        if line[pos..].starts_with(o) {
            pos += o.len();
            *bc = true;
            while pos < line.len() {
                if line[pos..].starts_with(c) {
                    pos += c.len();
                    *bc = false;
                    break;
                }
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

// ── Rust tokeniser ─────────────────────────────────────────────────────────────
//
// Hand-rolled to handle Rust-specific constructs:
//   · r#"raw strings"# · 'lifetime vs 'char' · #[attributes] · // and /* */ comments
//   · title.function after `fn` · title.class after struct/enum/trait/type/impl/union
//   · variable.language: self / Self / super

fn tokenize_rust(line: &str, in_block: &mut bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let mut next_ident: Option<Style> = None; // pending title.function / title.class

    // Continue block comment from previous line
    if *in_block {
        let (end, closed) = find_block_comment_close(line, "*/");
        spans.push(Span::styled(line[..end].to_owned(), cmt()));
        pos = end;
        if closed {
            *in_block = false;
        } else {
            return spans;
        }
    }

    // Rust outer / inner attribute: lines starting with #[ or #![
    {
        let tr = line[pos..].trim_start();
        if tr.starts_with("#[") || tr.starts_with("#![") {
            let ws_len = line[pos..].len() - tr.len();
            if ws_len > 0 {
                spans.push(Span::styled(line[pos..pos + ws_len].to_owned(), pl()));
            }
            spans.push(Span::styled(tr.to_owned(), pre()));
            return spans;
        }
    }

    let src = line;
    while pos < src.len() {
        let rem = &src[pos..];

        // Line comment (doc comment too)
        if rem.starts_with("//") {
            spans.push(Span::styled(rem.to_owned(), cmt()));
            break;
        }
        // Block comment /* … */
        if rem.starts_with("/*") {
            let (end, closed) = find_block_comment_close(&src[pos + 2..], "*/");
            let abs_end = pos + 2 + end;
            spans.push(Span::styled(src[pos..abs_end].to_owned(), cmt()));
            pos = abs_end;
            if !closed {
                *in_block = true;
                break;
            }
            continue;
        }

        // Raw string r"..." or r#"..."#
        if rem.starts_with("r\"") || rem.starts_with("r#") {
            let start = pos;
            pos += 1; // 'r'
            let mut hashes = 0usize;
            while pos < src.len() && src.as_bytes()[pos] == b'#' {
                pos += 1;
                hashes += 1;
            }
            if pos < src.len() && src.as_bytes()[pos] == b'"' {
                pos += 1; // opening "
                // eat until `"` followed by `hashes` `#`
                loop {
                    if pos >= src.len() {
                        break;
                    }
                    if src.as_bytes()[pos] == b'"' {
                        pos += 1;
                        let mut hc = 0usize;
                        while hc < hashes && pos < src.len() && src.as_bytes()[pos] == b'#' {
                            pos += 1;
                            hc += 1;
                        }
                        if hc == hashes {
                            break;
                        }
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

        // Cancel pending title.* on '(' or ';' or '{'
        if matches!(ch, '(' | ';' | '{') {
            next_ident = None;
        }

        // Double-quoted string  "…"
        if ch == '"' {
            let (text, new_pos) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(text, str_s()));
            pos = new_pos;
            continue;
        }
        // Single quote: lifetime vs char literal
        if ch == '\'' {
            let start = pos;
            pos += 1; // '
            match src[pos..].chars().next() {
                Some('\\') => {
                    // Char literal with escape
                    pos += 1;
                    pos += src[pos..].chars().next().map_or(0, |c| c.len_utf8());
                    if pos < src.len() && src.as_bytes()[pos] == b'\'' {
                        pos += 1;
                    }
                    spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
                }
                Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                    let id_start = pos;
                    while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
                    {
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
                _ => {
                    spans.push(Span::styled("'".to_owned(), pl()));
                }
            }
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let (text, new_pos) = eat_number_from(src, pos);
            spans.push(Span::styled(text, num()));
            pos = new_pos;
            continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
            {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next_ch = src[pos..].chars().next();

            let style = if let Some(s) = next_ident.take() {
                s
            } else if token == "self" || token == "Self" || token == "super" {
                var_lang()
            } else if RUST_LIT.iter().any(|l| *l == token) {
                kw()
            } else if RUST_KW.iter().any(|k| *k == token) {
                if RUST_FN_DECL.iter().any(|k| *k == token) {
                    next_ident = Some(func());
                } else if RUST_CLASS_DECL.iter().any(|k| *k == token) {
                    next_ident = Some(ty());
                }
                kw()
            } else if RUST_TY.iter().any(|t| *t == token) {
                ty()
            } else if RUST_BUILTIN.iter().any(|b| *b == token) {
                func()
            } else if next_ch == Some('!') {
                func() // macro invocation: println! vec! format! etc.
            } else if next_ch == Some('(') || next_ch == Some('<') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        // Operators / punctuation
        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,@#".contains(ch) {
            op()
        } else {
            pl()
        };
        spans.push(Span::styled(s, style));
        pos += ch.len_utf8();
    }
    spans
}

// ── Python tokeniser ───────────────────────────────────────────────────────────
//
// Highlight.js python.js inspired:
//   · keyword / built_in / literal / type / variable.language (self, cls)
//   · title.function after `def` · title.class after `class`
//   · @decorator as meta · # comment · strings (with triple-quotes on same line)

fn tokenize_python(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize;
    let mut next_ident: Option<Style> = None;
    let src = line;

    while pos < src.len() {
        let rem = &src[pos..];

        // Comment
        if rem.starts_with('#') {
            spans.push(Span::styled(rem.to_owned(), cmt()));
            break;
        }

        let ch = rem.chars().next().unwrap();

        // Cancel pending title.* on '(' or ':'
        if matches!(ch, '(' | ';' | ':') {
            next_ident = None;
        }

        // Decorator @name
        if ch == '@' {
            let start = pos;
            pos += 1;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), pre()));
            continue;
        }

        // Triple-quoted string (simplified: no cross-line state)
        if rem.starts_with("\"\"\"") || rem.starts_with("'''") {
            let delim = &rem[..3];
            let start = pos;
            pos += 3;
            if let Some(end_off) = src[pos..].find(delim) {
                pos += end_off + 3;
            } else {
                pos = src.len();
            }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
            continue;
        }

        // String literals with optional prefix (f/b/r/u and two-char combos)
        if matches!(ch, 'f' | 'b' | 'r' | 'u' | 'F' | 'B' | 'R' | 'U') {
            let prefix_end = pos + 1;
            let next1 = src[prefix_end..].chars().next();
            let (skip, quote_pos) = if matches!(next1, Some('b' | 'r' | 'B' | 'R' | 'f' | 'F'))
                && matches!(src[prefix_end + 1..].chars().next(), Some('"' | '\''))
            {
                (2usize, prefix_end + 1)
            } else if matches!(next1, Some('"' | '\'')) {
                (1usize, prefix_end)
            } else {
                (0usize, pos)
            };
            if skip > 0 {
                let start = pos;
                pos = quote_pos;
                if src[pos..].starts_with("\"\"\"") || src[pos..].starts_with("'''") {
                    let delim = src[pos..pos + 3].to_owned();
                    pos += 3;
                    if let Some(off) = src[pos..].find(delim.as_str()) {
                        pos += off + 3;
                    } else {
                        pos = src.len();
                    }
                } else {
                    let q = src[pos..].chars().next().unwrap();
                    let (_, np) = eat_string_from(src, pos, q);
                    pos = np;
                }
                spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
                continue;
            }
        }
        if ch == '"' {
            let (t, p) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }
        if ch == '\'' {
            let (t, p) = eat_string_from(src, pos, '\'');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let (text, new_pos) = eat_number_from(src, pos);
            spans.push(Span::styled(text, num()));
            pos = new_pos;
            continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
            {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next_ch = src[pos..].chars().next();

            let style = if let Some(s) = next_ident.take() {
                s
            } else if token == "self" || token == "cls" {
                var_lang()
            } else if PY_LIT.iter().any(|l| *l == token) {
                kw()
            } else if PY_KW.iter().any(|k| *k == token) {
                if token == "def" {
                    next_ident = Some(func());
                } else if token == "class" {
                    next_ident = Some(ty());
                }
                kw()
            } else if PY_TY.iter().any(|t| *t == token) {
                ty()
            } else if PY_BUILTIN.iter().any(|b| *b == token) {
                func()
            } else if next_ch == Some('(') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        // Operators
        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,".contains(ch) {
            op()
        } else {
            pl()
        };
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
            spans.push(Span::styled(rem.to_owned(), cmt()));
            break;
        }

        let ch = rem.chars().next().unwrap();

        // Strings
        if ch == '"' {
            let (t, p) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }
        if ch == '\'' {
            let (t, p) = eat_string_from(src, pos, '\'');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }
        if ch == '`' {
            let (t, p) = eat_string_from(src, pos, '`');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }

        // Variable: $VAR  ${VAR}  $((expr))  $(cmd)
        if ch == '$' {
            let start = pos;
            pos += 1;
            if src[pos..].starts_with("((") {
                pos += 2;
                let mut depth = 1usize;
                while pos < src.len() && depth > 0 {
                    if src[pos..].starts_with("((") {
                        depth += 1;
                        pos += 2;
                    } else if src[pos..].starts_with("))") {
                        depth -= 1;
                        pos += 2;
                    } else {
                        pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                    }
                }
            } else if src[pos..].starts_with('(') {
                pos += 1;
                let mut depth = 1usize;
                while pos < src.len() && depth > 0 {
                    match src[pos..].chars().next() {
                        Some('(') => {
                            depth += 1;
                            pos += 1;
                        }
                        Some(')') => {
                            depth -= 1;
                            pos += 1;
                        }
                        Some(c) => {
                            pos += c.len_utf8();
                        }
                        None => break,
                    }
                }
            } else if src[pos..].starts_with('{') {
                pos += 1;
                while pos < src.len() && src.as_bytes()[pos] != b'}' {
                    pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                }
                if pos < src.len() {
                    pos += 1;
                }
            } else {
                while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
                {
                    pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                }
            }
            spans.push(Span::styled(src[start..pos].to_owned(), var_lang()));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let (t, p) = eat_number_from(src, pos);
            spans.push(Span::styled(t, num()));
            pos = p;
            continue;
        }

        // Identifiers / keywords / built-ins
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            let token = &src[start..pos];
            let next_ch = src[pos..].chars().next();
            let style = if SH_KW.iter().any(|k| k.eq_ignore_ascii_case(token)) {
                kw()
            } else if SH_BUILTIN.iter().any(|b| b.eq_ignore_ascii_case(token)) {
                func()
            } else if next_ch == Some('(') {
                func()
            } else {
                pl()
            };
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~?:;.,".contains(ch) {
            op()
        } else {
            pl()
        };
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
            if src[pos..].starts_with('}') {
                pos += 1;
                *in_block = false;
                break;
            }
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
        spans.push(Span::styled(src[start..pos].to_owned(), cmt()));
        if *in_block {
            return spans;
        }
    }

    while pos < src.len() {
        let rem = &src[pos..];

        // Line comment //
        if rem.starts_with("//") {
            spans.push(Span::styled(rem.to_owned(), cmt()));
            break;
        }
        // Block comment (* ... *)
        if rem.starts_with("(*") {
            let start = pos;
            pos += 2;
            let mut found = false;
            while pos + 1 < src.len() {
                if src[pos..].starts_with("*)") {
                    pos += 2;
                    found = true;
                    break;
                }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if !found && pos + 1 >= src.len() {
                pos = src.len();
            }
            spans.push(Span::styled(src[start..pos].to_owned(), cmt()));
            continue;
        }
        // Block comment { ... }
        if rem.starts_with('{') {
            let start = pos;
            pos += 1;
            *in_block = true;
            while pos < src.len() {
                if src[pos..].starts_with('}') {
                    pos += 1;
                    *in_block = false;
                    break;
                }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), cmt()));
            continue;
        }

        let ch = rem.chars().next().unwrap();

        // String: 'hello''world' (Pascal string uses '' as escape for ' inside)
        if ch == '\'' {
            let start = pos;
            pos += 1;
            loop {
                if pos >= src.len() {
                    break;
                }
                if src.as_bytes()[pos] == b'\'' {
                    pos += 1;
                    // doubled quote inside string: ''
                    if pos < src.len() && src.as_bytes()[pos] == b'\'' {
                        pos += 1;
                        continue;
                    }
                    break;
                }
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit()
            || (ch == '$'
                && matches!(
                    src[pos + 1..].chars().next(),
                    Some('0'..='9' | 'A'..='F' | 'a'..='f')
                ))
        {
            let start = pos;
            if ch == '$' {
                pos += 1;
            } // hex prefix $FF
            while matches!(
                src[pos..].chars().next(),
                Some('0'..='9' | 'a'..='f' | 'A'..='F' | '.')
            ) {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::styled(src[start..pos].to_owned(), num()));
            continue;
        }

        // Identifiers
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
            {
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
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        let s = ch.to_string();
        let style = if "+-*/:=<>@^.,;()[]".contains(ch) {
            op()
        } else {
            pl()
        };
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
            spans.push(Span::styled(rem.to_owned(), cmt()));
            break;
        }

        let ch = rem.chars().next().unwrap();

        // String
        if ch == '"' {
            let (t, p) = eat_string_from(src, pos, '"');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }
        if ch == '\'' {
            let (t, p) = eat_string_from(src, pos, '\'');
            spans.push(Span::styled(t, str_s()));
            pos = p;
            continue;
        }

        // Hex literals: 0x... or ...h
        if ch.is_ascii_digit()
            || (ch == '$'
                && matches!(
                    src[pos + 1..].chars().next(),
                    Some('0'..='9' | 'A'..='F' | 'a'..='f')
                ))
        {
            let (t, p) = eat_number_from(src, pos);
            spans.push(Span::styled(t, num()));
            pos = p;
            continue;
        }

        // Identifiers / instructions / registers
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '.' {
            let start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
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
            spans.push(Span::styled(token.to_owned(), style));
            continue;
        }

        let s = ch.to_string();
        let style = if "+-*/%=<>!&|^~:,[]()".contains(ch) {
            op()
        } else {
            pl()
        };
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

    // Continue HTML comment <!-- … -->
    if *in_block {
        let (end, closed) = find_block_comment_close(src, "-->");
        spans.push(Span::styled(src[..end].to_owned(), cmt()));
        pos = end;
        if closed {
            *in_block = false;
        } else {
            return spans;
        }
    }

    while pos < src.len() {
        let rem = &src[pos..];

        // HTML comment <!-- ... -->
        if rem.starts_with("<!--") {
            let (end, closed) = find_block_comment_close(&src[pos + 4..], "-->");
            let abs_end = pos + 4 + end;
            spans.push(Span::styled(src[pos..abs_end].to_owned(), cmt()));
            pos = abs_end;
            if !closed {
                *in_block = true;
                break;
            }
            continue;
        }

        // HTML tag <tag> or </tag> or <tag ...attr...>
        if rem.starts_with('<') {
            let start = pos;
            pos += 1;
            // Consume tag: either in_tag state or self-close
            let style_bracket = op();
            // Emit '<'
            spans.push(Span::styled("<".to_owned(), style_bracket.clone()));

            // Optional '/' for close tag
            if pos < src.len() && src.as_bytes()[pos] == b'/' {
                spans.push(Span::styled("/".to_owned(), op()));
                pos += 1;
            }
            // Tag name
            let tag_start = pos;
            while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_')
            {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if pos > tag_start {
                spans.push(Span::styled(src[tag_start..pos].to_owned(), kw()));
            }
            // Attributes and closing >
            while pos < src.len() {
                let ch = src[pos..].chars().next().unwrap();
                if ch == '>' {
                    spans.push(Span::styled(">".to_owned(), op()));
                    pos += 1;
                    break;
                }
                if ch == '/' && src[pos + 1..].starts_with('>') {
                    spans.push(Span::styled("/>".to_owned(), op()));
                    pos += 2;
                    break;
                }
                // Attribute name
                if ch.is_ascii_alphabetic() || ch == '_' || ch == ':' {
                    let a_start = pos;
                    while matches!(src[pos..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
                    {
                        pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
                    }
                    spans.push(Span::styled(src[a_start..pos].to_owned(), ty()));
                    continue;
                }
                // Attribute value
                if ch == '"' || ch == '\'' {
                    let (t, p) = eat_string_from(src, pos, ch);
                    spans.push(Span::styled(t, str_s()));
                    pos = p;
                    continue;
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
            let start = pos;
            pos += 1;
            while pos < src.len() && src.as_bytes()[pos] != b';' && src.as_bytes()[pos] != b' ' {
                pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
            }
            if pos < src.len() && src.as_bytes()[pos] == b';' {
                pos += 1;
            }
            spans.push(Span::styled(src[start..pos].to_owned(), str_s()));
            continue;
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
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let style = if KETCHUP_KW.iter().any(|kw| kw.eq_ignore_ascii_case(&token)) {
                Style::default()
                    .fg(CLR_KETCHUP)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(token, style));
        } else {
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(Color::White),
            ));
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
            Some(c) if c == delim => {
                pos += delim.len_utf8();
                break;
            }
            Some(c) => {
                pos += c.len_utf8();
            }
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
        while matches!(
            src[pos..].chars().next(),
            Some('0'..='9' | 'a'..='f' | 'A'..='F' | '_')
        ) {
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    } else if src[pos..].starts_with("0b") || src[pos..].starts_with("0B") {
        pos += 2;
        while matches!(src[pos..].chars().next(), Some('0' | '1' | '_')) {
            pos += src[pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    } else {
        while matches!(
            src[pos..].chars().next(),
            Some('0'..='9' | '.' | 'e' | 'E' | '_' | 'f' | 'F' | 'u' | 'U' | 'l' | 'L' | 'i' | 's')
        ) {
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

// ── Keyword / type tables ─────────────────────────────────────────────────

// ─── C / C++ ──────────────────────────────────────────────────────────────
static C_KW: &[&str] = &[
    "break",
    "case",
    "continue",
    "default",
    "do",
    "else",
    "for",
    "goto",
    "if",
    "return",
    "switch",
    "while",
    "asm",
    "auto",
    "const",
    "enum",
    "extern",
    "inline",
    "register",
    "restrict",
    "sizeof",
    "static",
    "struct",
    "typedef",
    "union",
    "volatile",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "_Pragma",
    "alignas",
    "alignof",
    "noreturn",
    "static_assert",
    "thread_local",
    "typeof",
    "typeof_unqual",
    "constexpr",
    "nullptr",
];
static C_TY: &[&str] = &[
    "bool",
    "char",
    "double",
    "float",
    "int",
    "long",
    "short",
    "signed",
    "unsigned",
    "void",
    "_Bool",
    "_BitInt",
    "_Complex",
    "_Imaginary",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "int_fast8_t",
    "int_fast16_t",
    "int_fast32_t",
    "int_fast64_t",
    "uint_fast8_t",
    "uint_fast16_t",
    "uint_fast32_t",
    "uint_fast64_t",
    "int_least8_t",
    "int_least16_t",
    "int_least32_t",
    "int_least64_t",
    "uint_least8_t",
    "uint_least16_t",
    "uint_least32_t",
    "uint_least64_t",
    "intmax_t",
    "uintmax_t",
    "intptr_t",
    "uintptr_t",
    "ptrdiff_t",
    "size_t",
    "ssize_t",
    "wchar_t",
    "FILE",
    "string",
    "wstring",
    "vector",
    "map",
    "set",
    "pair",
    "optional",
    "variant",
    "shared_ptr",
    "unique_ptr",
    "weak_ptr",
    "deque",
    "list",
    "queue",
    "stack",
    "array",
];
static C_LIT: &[&str] = &["true", "false", "NULL", "nullptr"];
static C_BUILTIN: &[&str] = &[
    "printf", "fprintf", "sprintf", "snprintf", "scanf", "fscanf", "sscanf", "puts", "putchar",
    "putc", "gets", "getchar", "getc", "fopen", "fclose", "fread", "fwrite", "fgets", "fputs",
    "feof", "ferror", "perror", "fflush", "rewind", "fseek", "ftell", "malloc", "calloc",
    "realloc", "free", "abort", "exit", "atexit", "qsort", "bsearch", "atoi", "atol", "atof",
    "strtol", "strtod", "strtoul", "strlen", "strcpy", "strncpy", "strcat", "strncat", "strcmp",
    "strncmp", "strchr", "strrchr", "strstr", "strtok", "memset", "memcpy", "memmove", "memcmp",
    "abs", "fabs", "sqrt", "pow", "log", "log2", "log10", "exp", "sin", "cos", "tan", "asin",
    "acos", "atan", "atan2", "ceil", "floor", "round", "fmod", "isalpha", "isdigit", "isalnum",
    "isspace", "isupper", "islower", "toupper", "tolower",
];

// ─── Rust ─────────────────────────────────────────────────────────────────
static RUST_KW: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "final", "fn", "for", "if", "impl", "in", "let", "loop",
    "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "static",
    "struct", "super", "trait", "try", "type", "typeof", "union", "unsafe", "unsized", "use",
    "virtual", "where", "while", "yield",
];
static RUST_FN_DECL: &[&str] = &["fn"];
static RUST_CLASS_DECL: &[&str] = &["struct", "enum", "trait", "type", "union", "impl"];
static RUST_LIT: &[&str] = &["true", "false", "Some", "None", "Ok", "Err"];
static RUST_TY: &[&str] = &[
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    "bool",
    "char",
    "str",
    "String",
    "Vec",
    "Box",
    "Arc",
    "Rc",
    "Mutex",
    "RwLock",
    "Option",
    "Result",
    "HashMap",
    "HashSet",
    "BTreeMap",
    "BTreeSet",
    "VecDeque",
    "BinaryHeap",
    "Cow",
    "Cell",
    "RefCell",
    "Pin",
    "PhantomData",
    "PathBuf",
    "Path",
    "OsStr",
    "OsString",
];
static RUST_BUILTIN: &[&str] = &[
    "Copy",
    "Send",
    "Sized",
    "Sync",
    "Drop",
    "Fn",
    "FnMut",
    "FnOnce",
    "ToOwned",
    "Clone",
    "Debug",
    "PartialEq",
    "PartialOrd",
    "Eq",
    "Ord",
    "AsRef",
    "AsMut",
    "Into",
    "From",
    "Default",
    "Iterator",
    "Extend",
    "IntoIterator",
    "DoubleEndedIterator",
    "ExactSizeIterator",
    "ToString",
    "Display",
    "Write",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "panic",
    "unimplemented",
    "unreachable",
    "todo",
    "print",
    "println",
    "eprint",
    "eprintln",
    "format",
    "write",
    "writeln",
    "vec",
    "concat",
    "env",
    "file",
    "line",
    "module_path",
    "include_bytes",
    "include_str",
    "stringify",
    "cfg",
    "macro_rules",
    "drop",
];

// ─── JavaScript ───────────────────────────────────────────────────────────
static JS_KW: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "static",
    "switch",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "as",
];
static JS_FN_DECL: &[&str] = &["function", "get", "set"];
static JS_CLASS_DECL: &[&str] = &["class", "extends"];
static JS_LIT: &[&str] = &["true", "false", "null", "undefined", "NaN", "Infinity"];
static JS_TY: &[&str] = &[
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "Function",
    "Symbol",
    "BigInt",
    "Promise",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "InternalError",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Date",
    "RegExp",
    "JSON",
    "Math",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "Atomics",
    "Uint8Array",
    "Int8Array",
    "Uint16Array",
    "Int16Array",
    "Uint32Array",
    "Int32Array",
    "Float32Array",
    "Float64Array",
    "Proxy",
    "Reflect",
    "Intl",
    "WebAssembly",
    "Generator",
    "GeneratorFunction",
    "AsyncFunction",
    "globalThis",
];
static JS_VAR_LANG: &[&str] = &["this", "super", "arguments", "self"];
static JS_BUILTIN: &[&str] = &[
    "eval",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "console",
    "window",
    "document",
    "navigator",
    "location",
    "history",
    "module",
    "exports",
    "require",
    "global",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "queueMicrotask",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "fetch",
    "URL",
    "URLSearchParams",
    "FormData",
    "Headers",
    "Request",
    "Response",
    "alert",
    "confirm",
    "prompt",
];

// ─── Python ───────────────────────────────────────────────────────────────
static PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
    "yield",
];
static PY_LIT: &[&str] = &[
    "True",
    "False",
    "None",
    "NotImplemented",
    "Ellipsis",
    "__debug__",
];
static PY_TY: &[&str] = &[
    "Any",
    "Callable",
    "Coroutine",
    "Dict",
    "FrozenSet",
    "List",
    "Literal",
    "Generic",
    "Optional",
    "Sequence",
    "Set",
    "Tuple",
    "Type",
    "Union",
    "Exception",
    "BaseException",
    "ValueError",
    "TypeError",
    "KeyError",
    "IndexError",
    "AttributeError",
    "RuntimeError",
    "StopIteration",
    "OSError",
    "IOError",
    "FileNotFoundError",
    "PermissionError",
    "ImportError",
    "ModuleNotFoundError",
    "NotImplementedError",
    "ArithmeticError",
    "ZeroDivisionError",
    "OverflowError",
    "NameError",
    "UnboundLocalError",
    "RecursionError",
    "AssertionError",
    "SystemExit",
    "KeyboardInterrupt",
    "GeneratorExit",
    "MemoryError",
    "SyntaxError",
    "IndentationError",
    "UnicodeError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "Warning",
    "DeprecationWarning",
    "RuntimeWarning",
    "UserWarning",
];
static PY_BUILTIN: &[&str] = &[
    "__import__",
    "abs",
    "all",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

// ─── PHP ──────────────────────────────────────────────────────────────────
static PHP_KW: &[&str] = &[
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "enum",
    "extends",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "or",
    "print",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "xor",
    "yield",
];
static PHP_FN_DECL: &[&str] = &["function", "fn"];
static PHP_CLASS_DECL: &[&str] = &["class", "interface", "trait", "enum"];
static PHP_LIT: &[&str] = &["null", "true", "false", "NULL", "TRUE", "FALSE"];
static PHP_TY: &[&str] = &[
    "int",
    "float",
    "string",
    "bool",
    "array",
    "object",
    "void",
    "mixed",
    "never",
    "iterable",
    "callable",
    "self",
    "parent",
    "static",
    "Exception",
    "Error",
    "Throwable",
    "Iterator",
    "Countable",
    "Closure",
];

// ─── CSS ──────────────────────────────────────────────────────────────────
static CSS_KW: &[&str] = &[
    "important",
    "inherit",
    "initial",
    "unset",
    "revert",
    "none",
    "auto",
    "normal",
    "bold",
    "italic",
    "underline",
    "line-through",
    "overline",
    "block",
    "inline",
    "inline-block",
    "flex",
    "inline-flex",
    "grid",
    "inline-grid",
    "contents",
    "table",
    "list-item",
    "relative",
    "absolute",
    "fixed",
    "sticky",
    "center",
    "left",
    "right",
    "top",
    "bottom",
    "start",
    "end",
    "solid",
    "dashed",
    "dotted",
    "double",
    "groove",
    "ridge",
    "inset",
    "outset",
    "hidden",
    "visible",
    "scroll",
    "clip",
    "ellipsis",
    "nowrap",
    "wrap",
    "pointer",
    "default",
    "text",
    "crosshair",
    "move",
    "not-allowed",
    "uppercase",
    "lowercase",
    "capitalize",
    "contain",
    "cover",
    "to",
    "from",
    "at",
];
static CSS_TY: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklch",
    "oklab",
    "color",
    "color-mix",
    "var",
    "calc",
    "min",
    "max",
    "clamp",
    "env",
    "url",
    "linear-gradient",
    "radial-gradient",
    "conic-gradient",
    "repeating-linear-gradient",
    "repeating-radial-gradient",
    "translate",
    "translateX",
    "translateY",
    "translateZ",
    "translate3d",
    "rotate",
    "rotateX",
    "rotateY",
    "rotateZ",
    "rotate3d",
    "scale",
    "scaleX",
    "scaleY",
    "scaleZ",
    "scale3d",
    "skew",
    "skewX",
    "skewY",
    "matrix",
    "matrix3d",
    "perspective",
    "blur",
    "brightness",
    "contrast",
    "drop-shadow",
    "grayscale",
    "hue-rotate",
    "invert",
    "opacity",
    "saturate",
    "sepia",
    "cubic-bezier",
    "steps",
    "px",
    "em",
    "rem",
    "vh",
    "vw",
    "vmin",
    "vmax",
    "pt",
    "pc",
    "cm",
    "mm",
    "in",
    "ch",
    "ex",
    "fr",
    "deg",
    "rad",
    "turn",
    "s",
    "ms",
];

// ─── SQL ──────────────────────────────────────────────────────────────────
static SQL_KW: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "TABLE",
    "VIEW",
    "INDEX",
    "DATABASE",
    "SCHEMA",
    "COLUMN",
    "CONSTRAINT",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "ON",
    "AND",
    "OR",
    "NOT",
    "IN",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "IS",
    "NULL",
    "AS",
    "WITH",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ORDER",
    "BY",
    "GROUP",
    "HAVING",
    "DISTINCT",
    "UNION",
    "ALL",
    "EXCEPT",
    "INTERSECT",
    "EXISTS",
    "LIMIT",
    "OFFSET",
    "RETURNING",
    "TRIGGER",
    "FUNCTION",
    "PROCEDURE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "TRANSACTION",
    "SAVEPOINT",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "UNIQUE",
    "CHECK",
    "DEFAULT",
    "AUTO_INCREMENT",
    "SERIAL",
    "IDENTITY",
    "TRUNCATE",
    "REPLACE",
    "EXPLAIN",
    "ANALYZE",
    "CAST",
    "CONVERT",
    "COALESCE",
    "NULLIF",
    "GREATEST",
    "LEAST",
    "IF",
    "WHILE",
    "DECLARE",
    "CURSOR",
    "FETCH",
    "CLOSE",
    "MERGE",
    "OVER",
    "PARTITION",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "LAG",
    "LEAD",
    "FIRST_VALUE",
    "LAST_VALUE",
    "NTILE",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "ROUND",
    "FLOOR",
    "CEIL",
    "NOW",
    "CURRENT_TIMESTAMP",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CONCAT",
    "SUBSTRING",
    "LENGTH",
    "UPPER",
    "LOWER",
    "TRIM",
    "EXTRACT",
    "DATE_TRUNC",
];

// ─── Shell / Bash ─────────────────────────────────────────────────────────
static SH_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until", "case", "esac",
    "in", "function", "return", "exit", "break", "continue", "local", "readonly", "export",
    "unset", "declare", "typeset", "eval", "exec", "source",
];
static SH_BUILTIN: &[&str] = &[
    "echo", "printf", "read", "test", "true", "false", "cd", "pwd", "ls", "mkdir", "rmdir", "rm",
    "cp", "mv", "ln", "cat", "tac", "head", "tail", "wc", "sort", "uniq", "cut", "grep", "awk",
    "sed", "tr", "tee", "xargs", "find", "chmod", "chown", "chgrp", "touch", "stat", "date",
    "time", "sleep", "kill", "jobs", "fg", "bg", "wait", "set", "unset", "shift", "trap", "alias",
    "type", "which", "push", "pop", "enable", "builtin", "command",
];

// ─── Pascal / Delphi / Free Pascal ────────────────────────────────────────
static PAS_KW: &[&str] = &[
    "absolute",
    "abstract",
    "and",
    "array",
    "asm",
    "begin",
    "break",
    "case",
    "class",
    "const",
    "constructor",
    "continue",
    "destructor",
    "div",
    "do",
    "downto",
    "else",
    "end",
    "except",
    "exit",
    "exports",
    "file",
    "finalization",
    "finally",
    "for",
    "forward",
    "function",
    "goto",
    "if",
    "implementation",
    "in",
    "inherited",
    "initialization",
    "inline",
    "interface",
    "label",
    "library",
    "mod",
    "nil",
    "not",
    "object",
    "of",
    "on",
    "operator",
    "or",
    "packed",
    "procedure",
    "program",
    "property",
    "raise",
    "record",
    "repeat",
    "resourcestring",
    "self",
    "set",
    "shl",
    "shr",
    "string",
    "then",
    "threadvar",
    "to",
    "try",
    "type",
    "unit",
    "until",
    "uses",
    "var",
    "virtual",
    "while",
    "with",
    "xor",
];
static PAS_TY: &[&str] = &[
    "integer",
    "cardinal",
    "word",
    "byte",
    "shortint",
    "smallint",
    "longint",
    "int64",
    "qword",
    "dword",
    "real",
    "single",
    "double",
    "extended",
    "currency",
    "boolean",
    "char",
    "widechar",
    "pchar",
    "pwidechar",
    "ansistring",
    "widestring",
    "unicodestring",
    "shortstring",
    "pointer",
    "variant",
    "olevariant",
    "tcomponent",
    "tobject",
];

// ─── x86 / x86-64 Assembler ───────────────────────────────────────────────
static ASM_KW: &[&str] = &[
    "mov", "movs", "movz", "movsx", "movzx", "lea", "xchg", "push", "pop", "pusha", "popa",
    "pushad", "popad", "pushf", "popf", "pushfd", "popfd", "add", "adc", "sub", "sbb", "mul",
    "imul", "div", "idiv", "inc", "dec", "neg", "cmp", "test", "xor", "or", "and", "not", "shl",
    "shr", "sar", "sal", "rol", "ror", "rcl", "rcr", "bt", "bts", "btr", "btc", "bsf", "bsr",
    "call", "ret", "retn", "retf", "leave", "enter", "jmp", "je", "jne", "jz", "jnz", "ja", "jb",
    "jg", "jl", "jae", "jbe", "jge", "jle", "jo", "jno", "js", "jns", "jc", "jnc", "jcxz", "jecxz",
    "jrcxz", "loop", "loope", "loopne", "rep", "repe", "repne", "repnz", "repz", "int", "nop",
    "hlt", "cli", "sti", "clc", "stc", "cld", "std", "cpuid", "rdtsc", "syscall", "sysret",
    "cmpsb", "cmpsw", "cmpsd", "lodsb", "lodsw", "lodsd", "movsb", "movsw", "movsd", "stosb",
    "stosw", "stosd", "scasb", "scasw", "scasd", "db", "dw", "dd", "dq", "dt", "resb", "resw",
    "resd", "resq", "equ", "org", "align", "section", "segment", "proc", "endp", "macro", "endm",
    "local", "global", "extern", "public", "extrn", "assume", "end", "ends", "xlat", "xlatb",
];
static ASM_REG: &[&str] = &[
    "al", "bl", "cl", "dl", "ah", "bh", "ch", "dh", "sil", "dil", "spl", "bpl", "r8b", "r9b",
    "r10b", "r11b", "r12b", "r13b", "r14b", "r15b", "ax", "bx", "cx", "dx", "si", "di", "sp", "bp",
    "r8w", "r9w", "r10w", "r11w", "r12w", "r13w", "r14w", "r15w", "eax", "ebx", "ecx", "edx",
    "esi", "edi", "esp", "ebp", "r8d", "r9d", "r10d", "r11d", "r12d", "r13d", "r14d", "r15d",
    "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rsp", "rbp", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15", "cs", "ds", "es", "fs", "gs", "ss", "cr0", "cr2", "cr3", "cr4", "dr0", "dr1",
    "dr2", "dr3", "dr6", "dr7", "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
    "xmm8", "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15", "ymm0", "ymm1", "ymm2",
    "ymm3", "ymm4", "ymm5", "ymm6", "ymm7", "ymm8", "ymm9", "ymm10", "ymm11", "ymm12", "ymm13",
    "ymm14", "ymm15", "rip", "eip", "ip", "rflags", "eflags", "flags", "mm0", "mm1", "mm2", "mm3",
    "mm4", "mm5", "mm6", "mm7", "st0", "st1", "st2", "st3", "st4", "st5", "st6", "st7",
];

// ─── Ketchup ──────────────────────────────────────────────────────────────
static KETCHUP_KW: &[&str] = &[
    "blackward",
    "ketchup",
    "killers",
    "redbug",
    "access",
    "darkangel",
    "off",
    "topy",
    "kennet",
    "typeone",
    "pulpe",
    "tyby",
    "djamm",
    "vatin",
    "marjorie",
    "katana",
    "ecstasy",
    "cray",
    "magicfred",
    "cobra",
    "z",
];
