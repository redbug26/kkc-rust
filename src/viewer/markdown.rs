use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

const TABLE_LINK_OPEN: &str = "[[[KKC_LINK]]]";
const TABLE_LINK_CLOSE: &str = "[[[/KKC_LINK]]]";
const LINK_SUFFIX: &str = " 🔗";

#[derive(Debug, Clone)]
pub(crate) struct MarkdownRenderedLine {
    pub(crate) plain: String,
    pub(crate) styled: Line<'static>,
}

pub(crate) fn gemtext_to_markdown(source: &str) -> String {
    let mut out = String::new();
    let mut in_pre = false;

    for line in source.lines() {
        if line.starts_with("```") {
            in_pre = !in_pre;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_pre {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if let Some(rest) = line.strip_prefix("=>") {
            let mut parts = rest.trim_start().splitn(2, char::is_whitespace);
            let target = parts.next().unwrap_or("").trim();
            let label = parts.next().unwrap_or("").trim();
            if !target.is_empty() {
                let text = if label.is_empty() { target } else { label };
                out.push_str(&format!(
                    "[{}]({})\n",
                    escape_markdown_link_text(text),
                    escape_markdown_link_target(target)
                ));
                continue;
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

fn escape_markdown_link_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_markdown_link_target(value: &str) -> String {
    value.replace(')', "%29").replace(' ', "%20")
}

#[derive(Debug, Clone)]
struct ListFrame {
    ordered: bool,
    next_index: u64,
}

#[derive(Debug, Clone)]
struct TableState {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<String>>,
    header_rows: usize,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
    in_cell: bool,
    row_open: bool,
    current_row_is_header: bool,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            rows: Vec::new(),
            header_rows: 0,
            current_row: Vec::new(),
            current_cell: String::new(),
            in_head: false,
            in_cell: false,
            row_open: false,
            current_row_is_header: false,
        }
    }

    fn begin_row(&mut self, header: bool) {
        self.current_row.clear();
        self.row_open = true;
        self.current_row_is_header = header;
    }

    fn begin_cell(&mut self) {
        if !self.row_open {
            self.begin_row(self.in_head);
        }
        self.current_cell.clear();
        self.in_cell = true;
    }

    fn push_cell_text(&mut self, text: &str) {
        if self.in_cell {
            self.current_cell.push_str(text);
        }
    }

    fn end_cell(&mut self) {
        if !self.in_cell {
            return;
        }
        self.in_cell = false;
        let normalized = self
            .current_cell
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        self.current_row.push(normalized);
        self.current_cell.clear();
    }

    fn end_row(&mut self) {
        if !self.row_open {
            return;
        }
        if self.in_cell {
            self.end_cell();
        }
        if self.current_row.is_empty() {
            self.row_open = false;
            return;
        }
        if self.current_row_is_header {
            self.header_rows = self.header_rows.saturating_add(1);
        }
        self.rows.push(std::mem::take(&mut self.current_row));
        self.row_open = false;
        self.current_row_is_header = false;
    }
}

#[derive(Debug, Clone)]
struct RenderState {
    lines: Vec<MarkdownRenderedLine>,
    spans: Vec<Span<'static>>,
    plain: String,
    heading_level: Option<HeadingLevel>,
    blockquote_depth: usize,
    list_stack: Vec<ListFrame>,
    in_item: bool,
    pending_item_prefix: Option<String>,
    continuation_prefix: Option<String>,
    in_code_block: bool,
    code_block_lang: Option<String>,
    strong_depth: usize,
    emphasis_depth: usize,
    strikethrough_depth: usize,
    link_depth: usize,
    current_link_dest: Option<String>,
    table: Option<TableState>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            spans: Vec::new(),
            plain: String::new(),
            heading_level: None,
            blockquote_depth: 0,
            list_stack: Vec::new(),
            in_item: false,
            pending_item_prefix: None,
            continuation_prefix: None,
            in_code_block: false,
            code_block_lang: None,
            strong_depth: 0,
            emphasis_depth: 0,
            strikethrough_depth: 0,
            link_depth: 0,
            current_link_dest: None,
            table: None,
        }
    }

    fn ensure_line_prefix(&mut self) {
        if !self.plain.is_empty() {
            return;
        }
        if self.blockquote_depth > 0 {
            let quote = "│ ".repeat(self.blockquote_depth);
            self.push_with_style(
                &quote,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if let Some(prefix) = self.pending_item_prefix.take() {
            self.continuation_prefix = Some(" ".repeat(prefix.chars().count()));
            self.push_with_style(
                &prefix,
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            );
        } else if self.in_item
            && let Some(prefix) = self.continuation_prefix.clone()
        {
            self.push_with_style(&prefix, Style::default().fg(Color::DarkGray));
        }
    }

    fn current_text_style(&self) -> Style {
        let mut style = Style::default().fg(Color::White);
        if self.in_code_block {
            style = style.fg(Color::LightGreen);
        }
        if let Some(level) = self.heading_level {
            style = match level {
                HeadingLevel::H1 => Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                HeadingLevel::H2 => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                HeadingLevel::H3 => Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            };
        }
        if self.emphasis_depth > 0 {
            style = style.fg(Color::LightMagenta);
        }
        if self.strong_depth > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.strikethrough_depth > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link_depth > 0 {
            style = style
                .fg(Color::LightCyan)
                .add_modifier(Modifier::UNDERLINED);
        }
        if self.blockquote_depth > 0 {
            style = style.fg(Color::Gray);
        }
        style
    }

    fn push_with_style(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.plain.push_str(text);
        self.spans.push(Span::styled(text.to_string(), style));
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.ensure_line_prefix();
        let style = self.current_text_style();
        self.push_with_style(text, style);
    }

    fn flush_line(&mut self, allow_empty: bool) {
        if self.plain.is_empty() && !allow_empty {
            return;
        }
        let plain = std::mem::take(&mut self.plain);
        let spans = std::mem::take(&mut self.spans);
        self.lines.push(MarkdownRenderedLine {
            plain,
            styled: Line::from(spans),
        });
    }

    fn blank_line(&mut self) {
        let needs_blank = self
            .lines
            .last()
            .map(|line| !line.plain.is_empty())
            .unwrap_or(true);
        if needs_blank {
            self.lines.push(MarkdownRenderedLine {
                plain: String::new(),
                styled: Line::from(Span::raw(String::new())),
            });
        }
    }
}

fn push_table_header_separator(out: &mut Vec<MarkdownRenderedLine>, widths: &[usize]) {
    let mut plain = String::new();
    for (idx, width) in widths.iter().enumerate() {
        if idx > 0 {
            plain.push_str("─┼─");
        }
        plain.push_str(&"─".repeat(*width));
    }
    out.push(MarkdownRenderedLine {
        plain: plain.clone(),
        styled: Line::from(Span::styled(plain, Style::default().fg(Color::DarkGray))),
    });
}

fn heading_prefix(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "# ",
        HeadingLevel::H2 => "## ",
        HeadingLevel::H3 => "### ",
        HeadingLevel::H4 => "#### ",
        HeadingLevel::H5 => "##### ",
        HeadingLevel::H6 => "###### ",
    }
}

fn pad_cell_text(text: &str, width: usize, align: Alignment) -> String {
    let text_width = UnicodeWidthStr::width(text);
    if text_width >= width {
        return text.to_string();
    }
    let gap = width - text_width;
    match align {
        Alignment::Right => format!("{}{}", " ".repeat(gap), text),
        Alignment::Center => {
            let left = gap / 2;
            let right = gap - left;
            format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
        }
        _ => format!("{}{}", text, " ".repeat(gap)),
    }
}

fn take_prefix_by_width(text: &str, max_width: usize) -> (&str, &str) {
    if text.is_empty() || max_width == 0 {
        return ("", text);
    }
    let mut width = 0usize;
    let mut end = 0usize;
    for (idx, ch) in text.char_indices() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        // Fallback: at least consume one scalar when width accounting returns 0.
        let mut chars = text.chars();
        let first = chars.next().unwrap_or_default();
        let split = first.len_utf8();
        return (&text[..split], &text[split..]);
    }
    (&text[..end], &text[end..])
}

fn wrap_cell_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut out = Vec::new();
    let mut current = String::new();

    for word in text.split(' ') {
        if word.is_empty() {
            continue;
        }

        let word_width = UnicodeWidthStr::width(word);
        if current.is_empty() {
            if word_width <= width {
                current.push_str(word);
            } else {
                let mut rest = word;
                while !rest.is_empty() {
                    let (part, tail) = take_prefix_by_width(rest, width);
                    out.push(part.to_string());
                    rest = tail;
                }
            }
            continue;
        }

        let current_width = UnicodeWidthStr::width(current.as_str());
        if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            continue;
        }

        out.push(std::mem::take(&mut current));
        if word_width <= width {
            current.push_str(word);
        } else {
            let mut rest = word;
            while !rest.is_empty() {
                let (part, tail) = take_prefix_by_width(rest, width);
                out.push(part.to_string());
                rest = tail;
            }
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn table_strip_link_markers(cell: &str) -> String {
    cell.replace(TABLE_LINK_OPEN, "")
        .replace(TABLE_LINK_CLOSE, "")
}

fn table_cell_contains_link(cell: &str) -> bool {
    cell.contains(TABLE_LINK_OPEN) && cell.contains(TABLE_LINK_CLOSE)
}

fn render_table_lines(table: TableState, max_total_width: usize) -> Vec<MarkdownRenderedLine> {
    if table.rows.is_empty() {
        return Vec::new();
    }

    let col_count = table
        .rows
        .iter()
        .map(|row| row.len())
        .max()
        .unwrap_or(0)
        .max(table.alignments.len());
    if col_count == 0 {
        return Vec::new();
    }

    let mut widths = vec![3usize; col_count];
    for row in &table.rows {
        for (idx, cell) in row.iter().enumerate() {
            let visible = table_strip_link_markers(cell);
            widths[idx] = widths[idx].max(UnicodeWidthStr::width(visible.as_str()));
        }
    }

    // Keep very large tables readable in narrow viewer panes by constraining
    // total width and allowing cells to wrap on multiple visual lines.
    let table_max_total_width = max_total_width.max(24);
    let min_widths = vec![6usize; col_count];
    let separators_width = (col_count.saturating_sub(1)) * 3;
    let mut total_width = widths.iter().sum::<usize>() + separators_width;
    while total_width > table_max_total_width {
        let mut widest_idx = None;
        let mut widest = 0usize;
        for (idx, w) in widths.iter().copied().enumerate() {
            if w > min_widths[idx] && w > widest {
                widest = w;
                widest_idx = Some(idx);
            }
        }
        let Some(idx) = widest_idx else {
            break;
        };
        widths[idx] = widths[idx].saturating_sub(1);
        total_width = widths.iter().sum::<usize>() + separators_width;
    }

    let mut out = Vec::new();
    let header_rows = if table.header_rows > 0 {
        table.header_rows.min(table.rows.len())
    } else {
        1.min(table.rows.len())
    };

    for (row_idx, row) in table.rows.iter().enumerate() {
        let wrapped_cells = (0..col_count)
            .map(|col| {
                let text = row.get(col).map(String::as_str).unwrap_or("");
                wrap_cell_text(&table_strip_link_markers(text), widths[col])
            })
            .collect::<Vec<_>>();
        let visual_rows = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1).max(1);

        for visual_idx in 0..visual_rows {
            let mut spans = Vec::new();
            let mut plain = String::new();
            for col in 0..col_count {
                if col > 0 {
                    spans.push(Span::styled(
                        " │ ".to_string(),
                        Style::default().fg(Color::DarkGray),
                    ));
                    plain.push_str(" │ ");
                }
                let align = table
                    .alignments
                    .get(col)
                    .copied()
                    .unwrap_or(Alignment::Left);
                let text = wrapped_cells[col]
                    .get(visual_idx)
                    .map(String::as_str)
                    .unwrap_or("");
                let padded = pad_cell_text(text, widths[col], align);
                let mut cell_style = if row_idx < header_rows {
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let raw_cell = row.get(col).map(String::as_str).unwrap_or("");
                if table_cell_contains_link(raw_cell) {
                    cell_style = cell_style
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::UNDERLINED);
                }
                spans.push(Span::styled(padded.clone(), cell_style));
                plain.push_str(&padded);
            }
            out.push(MarkdownRenderedLine {
                plain,
                styled: Line::from(spans),
            });
        }

        if row_idx + 1 == header_rows {
            push_table_header_separator(&mut out, &widths);
        }
    }
    out
}

/// Wrap lines that exceed max_width, breaking at word boundaries.
fn wrap_markdown_lines(
    lines: Vec<MarkdownRenderedLine>,
    max_width: usize,
) -> Vec<MarkdownRenderedLine> {
    if max_width == 0 {
        return lines;
    }

    let mut wrapped = Vec::new();

    for line in lines {
        let line_width = UnicodeWidthStr::width(line.plain.as_str());

        // If line fits within max_width, keep it as-is
        if line_width <= max_width {
            wrapped.push(line);
            continue;
        }

        let continuation_prefix = markdown_wrap_continuation_prefix(&line.plain);
        let continuation_prefix_width = UnicodeWidthStr::width(continuation_prefix.as_str());
        let continuation_prefix_style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);

        // Line is too long; we need to wrap it by breaking at word boundaries
        // while preserving the styling from the original spans.
        // Strategy: collect all styled words, then redistribute them across lines.

        let mut word_spans: Vec<(String, Style)> = Vec::new();
        let mut current_word = String::new();
        let mut current_style = Style::default();

        // Extract words with their styles from the original spans
        for span in &line.styled.spans {
            let text = span.content.as_ref();
            let style = span.style;

            for ch in text.chars() {
                if ch == ' ' || ch == '\t' || ch == '\n' {
                    if !current_word.is_empty() {
                        word_spans.push((current_word.clone(), current_style));
                        current_word.clear();
                    }
                    word_spans.push((ch.to_string(), style)); // Space as separate "word"
                } else {
                    current_word.push(ch);
                    current_style = style;
                }
            }
        }

        if !current_word.is_empty() {
            word_spans.push((current_word, current_style));
        }

        // Now redistribute words across lines, respecting max_width
        let mut current_line_plain = String::new();
        let mut current_line_spans: Vec<Span<'static>> = Vec::new();
        let mut current_line_width = 0usize;

        for (word, style) in word_spans {
            let word_width = UnicodeWidthStr::width(word.as_str());
            if current_line_width == continuation_prefix_width
                && word.chars().all(char::is_whitespace)
            {
                continue;
            }
            if word_width > max_width {
                if current_line_width > 0 {
                    wrapped.push(MarkdownRenderedLine {
                        plain: current_line_plain.clone(),
                        styled: Line::from(current_line_spans),
                    });
                    current_line_plain = continuation_prefix.clone();
                    current_line_spans = if continuation_prefix.is_empty() {
                        Vec::new()
                    } else {
                        vec![Span::styled(
                            continuation_prefix.clone(),
                            continuation_prefix_style,
                        )]
                    };
                    current_line_width = continuation_prefix_width;
                }

                let mut rest = word.as_str();
                while !rest.is_empty() {
                    let available = max_width.saturating_sub(current_line_width).max(1);
                    let (part, tail) = take_prefix_by_width(rest, available);
                    current_line_plain.push_str(part);
                    current_line_spans.push(Span::styled(part.to_string(), style));
                    current_line_width += UnicodeWidthStr::width(part);
                    rest = tail;
                    if !rest.is_empty() {
                        wrapped.push(MarkdownRenderedLine {
                            plain: current_line_plain.clone(),
                            styled: Line::from(current_line_spans),
                        });
                        current_line_plain = continuation_prefix.clone();
                        current_line_spans = if continuation_prefix.is_empty() {
                            Vec::new()
                        } else {
                            vec![Span::styled(
                                continuation_prefix.clone(),
                                continuation_prefix_style,
                            )]
                        };
                        current_line_width = continuation_prefix_width;
                    }
                }
                continue;
            }

            // If adding this word to current line would exceed max_width, flush current line
            if current_line_width > 0 && current_line_width + word_width > max_width {
                wrapped.push(MarkdownRenderedLine {
                    plain: current_line_plain.clone(),
                    styled: Line::from(current_line_spans),
                });
                current_line_plain = continuation_prefix.clone();
                current_line_spans = if continuation_prefix.is_empty() {
                    Vec::new()
                } else {
                    vec![Span::styled(
                        continuation_prefix.clone(),
                        continuation_prefix_style,
                    )]
                };
                current_line_width = continuation_prefix_width;
                if word.chars().all(char::is_whitespace) {
                    continue;
                }
            }

            // Add word to current line
            current_line_plain.push_str(&word);
            current_line_spans.push(Span::styled(word, style));
            current_line_width += word_width;
        }

        // Add remaining content
        if !current_line_plain.is_empty() {
            wrapped.push(MarkdownRenderedLine {
                plain: current_line_plain,
                styled: Line::from(current_line_spans),
            });
        }
    }

    wrapped
}

fn markdown_wrap_continuation_prefix(plain: &str) -> String {
    let mut prefix = String::new();
    let mut rest = plain;
    while let Some(after_quote) = rest.strip_prefix("│ ") {
        prefix.push_str("│ ");
        rest = after_quote;
    }
    prefix
}

pub(crate) fn render_commonmark(
    source: &str,
    max_table_width: usize,
    wrap_text: bool,
) -> Vec<MarkdownRenderedLine> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(source, options);
    let mut state = RenderState::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Table(alignments) => {
                    state.flush_line(false);
                    state.table = Some(TableState::new(alignments));
                }
                Tag::TableHead => {
                    if let Some(table) = state.table.as_mut() {
                        table.in_head = true;
                    }
                }
                Tag::TableRow => {
                    if let Some(table) = state.table.as_mut() {
                        table.begin_row(table.in_head);
                    }
                }
                Tag::TableCell => {
                    if let Some(table) = state.table.as_mut() {
                        table.begin_cell();
                    }
                }
                Tag::Heading { level, .. } => {
                    state.flush_line(false);
                    state.heading_level = Some(level);
                    state.push_with_style(
                        heading_prefix(level),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    );
                }
                Tag::BlockQuote(_) => {
                    state.flush_line(false);
                    state.blockquote_depth = state.blockquote_depth.saturating_add(1);
                }
                Tag::CodeBlock(kind) => {
                    state.flush_line(false);
                    state.in_code_block = true;
                    state.code_block_lang = match kind {
                        CodeBlockKind::Indented => None,
                        CodeBlockKind::Fenced(lang) if lang.is_empty() => None,
                        CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    };
                    let header = match state.code_block_lang.as_deref() {
                        Some(lang) => format!("┌─ code [{lang}]"),
                        None => "┌─ code".to_string(),
                    };
                    state.push_with_style(
                        &header,
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    );
                    state.flush_line(true);
                }
                Tag::List(start) => {
                    state.flush_line(false);
                    state.list_stack.push(ListFrame {
                        ordered: start.is_some(),
                        next_index: start.unwrap_or(1),
                    });
                }
                Tag::Item => {
                    state.flush_line(false);
                    state.in_item = true;
                    let prefix = if let Some(frame) = state.list_stack.last_mut() {
                        if frame.ordered {
                            let value = frame.next_index;
                            frame.next_index = frame.next_index.saturating_add(1);
                            format!("{value}. ")
                        } else {
                            "• ".to_string()
                        }
                    } else {
                        "• ".to_string()
                    };
                    state.pending_item_prefix = Some(prefix);
                }
                Tag::Emphasis => state.emphasis_depth = state.emphasis_depth.saturating_add(1),
                Tag::Strong => state.strong_depth = state.strong_depth.saturating_add(1),
                Tag::Strikethrough => {
                    state.strikethrough_depth = state.strikethrough_depth.saturating_add(1)
                }
                Tag::Link { dest_url, .. } => {
                    state.link_depth = state.link_depth.saturating_add(1);
                    state.current_link_dest = Some(dest_url.to_string());
                    if let Some(table) = state.table.as_mut() {
                        table.push_cell_text(TABLE_LINK_OPEN);
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Table => {
                    if let Some(table) = state.table.take() {
                        let lines = render_table_lines(table, max_table_width);
                        state.lines.extend(lines);
                        state.blank_line();
                    }
                }
                TagEnd::TableHead => {
                    if let Some(table) = state.table.as_mut() {
                        table.end_row();
                        table.in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(table) = state.table.as_mut() {
                        table.end_row();
                    }
                }
                TagEnd::TableCell => {
                    if let Some(table) = state.table.as_mut() {
                        table.end_cell();
                    }
                }
                TagEnd::Paragraph => {
                    state.flush_line(true);
                    state.blank_line();
                }
                TagEnd::Heading(_) => {
                    state.heading_level = None;
                    state.flush_line(true);
                    state.blank_line();
                }
                TagEnd::BlockQuote(_) => {
                    state.flush_line(false);
                    state.blockquote_depth = state.blockquote_depth.saturating_sub(1);
                    state.blank_line();
                }
                TagEnd::CodeBlock => {
                    if !state.plain.is_empty() {
                        state.flush_line(true);
                    }
                    state.in_code_block = false;
                    state.code_block_lang = None;
                    state.push_with_style(
                        "└─",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    );
                    state.flush_line(true);
                    state.blank_line();
                }
                TagEnd::List(_) => {
                    state.flush_line(false);
                    state.list_stack.pop();
                    state.blank_line();
                }
                TagEnd::Item => {
                    state.flush_line(false);
                    state.in_item = false;
                    state.continuation_prefix = None;
                }
                TagEnd::Emphasis => {
                    state.emphasis_depth = state.emphasis_depth.saturating_sub(1);
                }
                TagEnd::Strong => {
                    state.strong_depth = state.strong_depth.saturating_sub(1);
                }
                TagEnd::Strikethrough => {
                    state.strikethrough_depth = state.strikethrough_depth.saturating_sub(1);
                }
                TagEnd::Link => {
                    state.link_depth = state.link_depth.saturating_sub(1);
                    if state.link_depth == 0 {
                        if let Some(table) = state.table.as_mut() {
                            table.push_cell_text(LINK_SUFFIX);
                            table.push_cell_text(TABLE_LINK_CLOSE);
                        } else {
                            state.push_with_style(
                                LINK_SUFFIX,
                                Style::default()
                                    .fg(Color::LightCyan)
                                    .add_modifier(Modifier::BOLD),
                            );
                        }
                        state.current_link_dest = None;
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(&text);
                } else if state.in_code_block {
                    let mut iter = text.split('\n').peekable();
                    let mut idx = 0usize;
                    while let Some(line) = iter.next() {
                        // Ignore trailing empty line from fenced blocks ending with '\n'
                        // so we don't render an extra blank row before "└─".
                        if line.is_empty() && iter.peek().is_none() {
                            break;
                        }
                        if idx > 0 {
                            state.flush_line(true);
                        }
                        state.push_with_style("  ", Style::default().fg(Color::DarkGray));
                        state.push_with_style(line, Style::default().fg(Color::LightGreen));
                        idx += 1;
                    }
                } else {
                    state.push_text(&text);
                }
            }
            Event::Code(code) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(&code);
                } else {
                    state.push_with_style(
                        &code,
                        Style::default()
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            Event::SoftBreak => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(" ");
                } else if state.in_code_block {
                    state.flush_line(true);
                } else {
                    state.flush_line(true);
                }
            }
            Event::HardBreak => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(" ");
                } else if state.in_code_block {
                    state.flush_line(true);
                } else {
                    state.flush_line(true);
                }
            }
            Event::Rule => {
                state.flush_line(false);
                let rule_width = max_table_width.max(1);
                let rule = "─".repeat(rule_width - 2);
                state.push_with_style(&rule, Style::default().fg(Color::DarkGray));
                state.flush_line(true);
                state.blank_line();
            }
            Event::Html(text) | Event::InlineHtml(text) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(&text);
                } else {
                    state.push_with_style(
                        &text,
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::ITALIC),
                    );
                }
            }
            Event::FootnoteReference(label) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(&format!("[^{}]", label));
                } else {
                    state.push_with_style(
                        &format!("[^{}]", label),
                        Style::default()
                            .fg(Color::LightBlue)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            Event::TaskListMarker(done) => {
                let marker = if done { "[x] " } else { "[ ] " };
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(marker);
                } else {
                    state.push_with_style(
                        marker,
                        Style::default()
                            .fg(Color::LightBlue)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell_text(&text);
                } else {
                    state.push_with_style(
                        &text,
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::ITALIC),
                    );
                }
            }
        }
    }

    if let Some(table) = state.table.as_mut() {
        table.end_row();
    }
    state.flush_line(false);
    while state
        .lines
        .last()
        .map(|line| line.plain.is_empty())
        .unwrap_or(false)
    {
        state.lines.pop();
    }
    if state.lines.is_empty() {
        state.lines.push(MarkdownRenderedLine {
            plain: String::new(),
            styled: Line::from(Span::raw(String::new())),
        });
    }

    // Wrap lines to max_table_width only when wrapping is enabled.
    if wrap_text {
        wrap_markdown_lines(state.lines, max_table_width)
    } else {
        state.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_blockquotes_keep_quote_prefix() {
        let lines = render_commonmark(
            "> Apres le premier morceau vient une continuation lisible",
            24,
            true,
        );
        let quote_lines = lines
            .iter()
            .filter(|line| line.plain.starts_with("│ "))
            .collect::<Vec<_>>();

        assert!(quote_lines.len() >= 2);
        assert!(quote_lines.iter().all(|line| line.plain.starts_with("│ ")));
    }

    #[test]
    fn wrapped_links_break_long_urls() {
        let lines = render_commonmark(
            "[Picture](https://commons.wikimedia.org/wiki/File:The_Brain_Machine_(4906298386%29.jpg)",
            40,
            true,
        );

        assert!(lines.len() >= 2);
        assert!(
            lines
                .iter()
                .all(|line| UnicodeWidthStr::width(line.plain.as_str()) <= 40)
        );
    }

    #[test]
    fn wrapped_lines_drop_leading_break_space() {
        let lines = render_commonmark(
            "La pub nous prend pour des cretins. Et experimentalement parlant elle a raison.",
            38,
            true,
        );

        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|line| !line.plain.starts_with(' ')));
    }

    #[test]
    fn rendered_links_show_link_suffix() {
        let lines = render_commonmark("[Ploum](gemini://ploum.net)", 80, true);

        assert!(lines.iter().any(|line| line.plain.contains("Ploum 🔗")));
    }
}
