use crate::app::{ComparePanelState, CompareRow, CompareRowKind};
use anyhow::Result;

#[derive(Debug, Clone)]
pub enum CompareBuffer {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone)]
enum CompareOp {
    Equal(usize, usize),
    Removed(usize),
    Added(usize),
}

struct CompareBuildResult {
    summary: String,
    message: Option<String>,
    rows: Vec<CompareRow>,
}

pub fn load_compare_buffer(path: &std::path::Path) -> Result<CompareBuffer> {
    let bytes = std::fs::read(path)?;
    if is_probably_binary(&bytes) {
        Ok(CompareBuffer::Binary(bytes))
    } else {
        Ok(CompareBuffer::Text(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    }
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    let sample_len = bytes.len().min(8192);
    let sample = &bytes[..sample_len];

    // NUL bytes are a strong binary signal.
    if sample.contains(&0) {
        return true;
    }

    // Treat extended bytes (>= 0x80) as textual to support legacy encodings.
    let mut suspicious = 0usize;
    for &byte in sample {
        let is_text_byte = matches!(byte, b'\n' | b'\r' | b'\t' | 0x08 | 0x0C)
            || (0x20..=0x7E).contains(&byte)
            || byte >= 0x80;
        if !is_text_byte {
            suspicious += 1;
        }
    }

    suspicious * 100 > sample_len * 30
}

pub fn build_compare_panel_state(
    left_label: String,
    right_label: String,
    left_buffer: CompareBuffer,
    right_buffer: CompareBuffer,
) -> ComparePanelState {
    let mut state = ComparePanelState {
        left_label,
        right_label,
        left_buffer,
        right_buffer,
        show_only_differences: true,
        ignore_whitespace: false,
        ignore_crlf: false,
        summary: String::new(),
        message: None,
        rows: Vec::new(),
        cursor: 0,
        scroll: 0,
        search_query: String::new(),
        search_cursor: 0,
        search_active: false,
    };
    rebuild_compare_panel_state(&mut state);
    state
}

pub fn rebuild_compare_panel_state(state: &mut ComparePanelState) {
    let result = match (&state.left_buffer, &state.right_buffer) {
        (CompareBuffer::Text(left), CompareBuffer::Text(right)) => rebuild_text_rows(
            left,
            right,
            state.show_only_differences,
            state.ignore_whitespace,
            state.ignore_crlf,
        ),
        (CompareBuffer::Binary(left), CompareBuffer::Binary(right)) => {
            rebuild_binary_rows(left, right, state.show_only_differences)
        }
        (CompareBuffer::Text(_), CompareBuffer::Binary(right)) => rebuild_binary_message(format!(
            "Right file is binary ({} bytes). Compare is only available for text files, or binary files with the same size.",
            right.len()
        )),
        (CompareBuffer::Binary(left), CompareBuffer::Text(_)) => rebuild_binary_message(format!(
            "Left file is binary ({} bytes). Compare is only available for text files, or binary files with the same size.",
            left.len()
        )),
    };

    state.summary = result.summary;
    state.message = result.message;
    state.rows = result.rows;
    clamp_compare_cursor(state);

    if !state.search_query.is_empty() {
        let _ = jump_to_compare_search_match(state, true, false);
    }
}

pub fn jump_to_compare_search_match(
    state: &mut ComparePanelState,
    forward: bool,
    include_current: bool,
) -> bool {
    if state.rows.is_empty() || state.search_query.is_empty() {
        return false;
    }

    let query = state.search_query.to_lowercase();
    let len = state.rows.len();
    let start = if include_current {
        state.cursor.min(len.saturating_sub(1))
    } else if forward {
        (state.cursor + 1).min(len)
    } else if state.cursor == 0 {
        len.saturating_sub(1)
    } else {
        state.cursor - 1
    };

    for step in 0..len {
        let idx = if forward {
            (start + step) % len
        } else {
            (start + len - (step % len)) % len
        };
        let row = &state.rows[idx];
        if row_matches_query(row, &query) {
            state.cursor = idx;
            state.scroll = state.scroll.min(idx);
            return true;
        }
    }

    false
}

fn rebuild_binary_message(message: String) -> CompareBuildResult {
    CompareBuildResult {
        summary: "Binary content".to_string(),
        message: Some(message),
        rows: Vec::new(),
    }
}

fn rebuild_binary_rows(
    left: &[u8],
    right: &[u8],
    show_only_differences: bool,
) -> CompareBuildResult {
    if left.len() != right.len() {
        return rebuild_binary_message(format!(
            "Both files are binary, but their sizes differ ({} vs {} bytes). Compare only shows binary content when sizes are identical.",
            left.len(),
            right.len()
        ));
    }

    let mut rows = Vec::new();
    for offset in (0..left.len()).step_by(16) {
        let end = (offset + 16).min(left.len());
        let left_chunk = &left[offset..end];
        let right_chunk = &right[offset..end];
        let kind = if left_chunk == right_chunk {
            CompareRowKind::Equal
        } else {
            CompareRowKind::Changed
        };
        if kind == CompareRowKind::Equal && show_only_differences {
            continue;
        }
        rows.push(CompareRow {
            kind,
            left_no: Some(offset),
            right_no: Some(offset),
            left_text: format_binary_chunk(offset, left_chunk),
            right_text: format_binary_chunk(offset, right_chunk),
        });
    }

    CompareBuildResult {
        summary: if rows.is_empty() {
            "Binary files are identical".to_string()
        } else {
            format!(
                "{} chunk(s)  binary same-size compare  diff:{}",
                rows.len(),
                if show_only_differences { "on" } else { "off" }
            )
        },
        message: Some(format!(
            "Binary compare shown by 16-byte chunks. Size: {} bytes.",
            left.len()
        )),
        rows,
    }
}

fn rebuild_text_rows(
    left: &str,
    right: &str,
    show_only_differences: bool,
    ignore_whitespace: bool,
    ignore_crlf: bool,
) -> CompareBuildResult {
    let left_lines = compare_lines(left);
    let right_lines = compare_lines(right);
    let left_keys = left_lines
        .iter()
        .map(|line| compare_key(line, ignore_whitespace, ignore_crlf))
        .collect::<Vec<_>>();
    let right_keys = right_lines
        .iter()
        .map(|line| compare_key(line, ignore_whitespace, ignore_crlf))
        .collect::<Vec<_>>();

    let cols = right_keys.len() + 1;
    let mut dp = vec![0usize; (left_keys.len() + 1) * cols];
    for left_idx in (0..left_keys.len()).rev() {
        for right_idx in (0..right_keys.len()).rev() {
            let slot = left_idx * cols + right_idx;
            dp[slot] = if left_keys[left_idx] == right_keys[right_idx] {
                dp[(left_idx + 1) * cols + right_idx + 1] + 1
            } else {
                dp[(left_idx + 1) * cols + right_idx].max(dp[left_idx * cols + right_idx + 1])
            };
        }
    }

    let mut ops = Vec::new();
    let mut left_idx = 0usize;
    let mut right_idx = 0usize;
    while left_idx < left_keys.len() && right_idx < right_keys.len() {
        if left_keys[left_idx] == right_keys[right_idx] {
            ops.push(CompareOp::Equal(left_idx, right_idx));
            left_idx += 1;
            right_idx += 1;
        } else if dp[(left_idx + 1) * cols + right_idx] >= dp[left_idx * cols + right_idx + 1] {
            ops.push(CompareOp::Removed(left_idx));
            left_idx += 1;
        } else {
            ops.push(CompareOp::Added(right_idx));
            right_idx += 1;
        }
    }
    while left_idx < left_keys.len() {
        ops.push(CompareOp::Removed(left_idx));
        left_idx += 1;
    }
    while right_idx < right_keys.len() {
        ops.push(CompareOp::Added(right_idx));
        right_idx += 1;
    }

    let mut rows = Vec::new();
    let mut cursor = 0usize;
    while cursor < ops.len() {
        match ops[cursor].clone() {
            CompareOp::Equal(left_row, right_row) => {
                if !show_only_differences {
                    rows.push(CompareRow {
                        kind: CompareRowKind::Equal,
                        left_no: Some(left_row + 1),
                        right_no: Some(right_row + 1),
                        left_text: compare_display_line(&left_lines[left_row]),
                        right_text: compare_display_line(&right_lines[right_row]),
                    });
                }
                cursor += 1;
            }
            _ => {
                let start = cursor;
                while cursor < ops.len() && !matches!(ops[cursor], CompareOp::Equal(_, _)) {
                    cursor += 1;
                }
                let mut removed = Vec::new();
                let mut added = Vec::new();
                for op in &ops[start..cursor] {
                    match *op {
                        CompareOp::Removed(left_row) => removed.push(left_row),
                        CompareOp::Added(right_row) => added.push(right_row),
                        CompareOp::Equal(_, _) => {}
                    }
                }
                let count = removed.len().max(added.len());
                for idx in 0..count {
                    let left_row = removed.get(idx).copied();
                    let right_row = added.get(idx).copied();
                    let kind = match (left_row.is_some(), right_row.is_some()) {
                        (true, true) => CompareRowKind::Changed,
                        (true, false) => CompareRowKind::Removed,
                        (false, true) => CompareRowKind::Added,
                        (false, false) => continue,
                    };
                    rows.push(CompareRow {
                        kind,
                        left_no: left_row.map(|value| value + 1),
                        right_no: right_row.map(|value| value + 1),
                        left_text: left_row
                            .map(|value| compare_display_line(&left_lines[value]))
                            .unwrap_or_default(),
                        right_text: right_row
                            .map(|value| compare_display_line(&right_lines[value]))
                            .unwrap_or_default(),
                    });
                }
            }
        }
    }

    CompareBuildResult {
        summary: if rows.is_empty() {
            "Files are identical".to_string()
        } else {
            format!(
                "{} row(s)  diff:{} ws:{} crlf:{}",
                rows.len(),
                if show_only_differences { "on" } else { "off" },
                if ignore_whitespace { "on" } else { "off" },
                if ignore_crlf { "on" } else { "off" }
            )
        },
        message: None,
        rows,
    }
}

fn clamp_compare_cursor(state: &mut ComparePanelState) {
    if state.rows.is_empty() {
        state.cursor = 0;
        state.scroll = 0;
    } else {
        state.cursor = state.cursor.min(state.rows.len().saturating_sub(1));
        state.scroll = state.scroll.min(state.cursor);
    }
}

fn format_binary_chunk(offset: usize, bytes: &[u8]) -> String {
    let mut hex = bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    while hex.len() < 47 {
        hex.push(' ');
    }
    let ascii = bytes
        .iter()
        .map(|byte| match byte {
            0x20..=0x7e => *byte as char,
            _ => '.',
        })
        .collect::<String>();
    format!("{offset:08X}: {hex}  |{ascii}|")
}

fn row_matches_query(row: &CompareRow, query: &str) -> bool {
    row.left_text.to_lowercase().contains(query) || row.right_text.to_lowercase().contains(query)
}

fn compare_display_line(line: &str) -> String {
    line.trim_end_matches('\r').to_string()
}

fn compare_key(line: &str, ignore_whitespace: bool, ignore_crlf: bool) -> String {
    let mut value = if ignore_crlf {
        line.trim_end_matches('\r').to_string()
    } else {
        line.to_string()
    };
    if ignore_whitespace {
        value = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    }
    value
}

fn compare_lines(content: &str) -> Vec<String> {
    content
        .split_terminator('\n')
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_same_size_compare_shows_changed_chunks() {
        let state = build_compare_panel_state(
            "left".to_string(),
            "right".to_string(),
            CompareBuffer::Binary(vec![0x41, 0x42, 0x43, 0x44]),
            CompareBuffer::Binary(vec![0x41, 0x42, 0x99, 0x44]),
        );

        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].kind, CompareRowKind::Changed);
        assert!(state.summary.contains("binary same-size compare"));
    }

    #[test]
    fn binary_different_size_compare_shows_message() {
        let state = build_compare_panel_state(
            "left".to_string(),
            "right".to_string(),
            CompareBuffer::Binary(vec![1, 2, 3]),
            CompareBuffer::Binary(vec![1, 2, 3, 4]),
        );

        assert!(state.rows.is_empty());
        assert!(
            state
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("sizes differ")
        );
    }

    #[test]
    fn search_jumps_to_matching_compare_row() {
        let mut state = build_compare_panel_state(
            "left".to_string(),
            "right".to_string(),
            CompareBuffer::Text("alpha\nbeta\ngamma\n".to_string()),
            CompareBuffer::Text("alpha\nbeta changed\ngamma\n".to_string()),
        );

        state.search_query = "changed".to_string();
        assert!(jump_to_compare_search_match(&mut state, true, true));
        assert_eq!(state.cursor, 0);
        assert!(state.rows[state.cursor].right_text.contains("changed"));
    }

    #[test]
    fn binary_detection_accepts_latin1_text() {
        let text_bytes = b"4 to 4 music\n"
            .iter()
            .copied()
            .chain([0xE9])
            .collect::<Vec<_>>();
        assert!(!is_probably_binary(&text_bytes));
    }

    #[test]
    fn binary_detection_rejects_nul_content() {
        let bin = vec![0x41, 0x00, 0x42, 0x43];
        assert!(is_probably_binary(&bin));
    }
}
