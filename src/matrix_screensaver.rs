use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::time::{SystemTime, UNIX_EPOCH};

const MATRIX_ASCII_GLYPHS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*()[]{}<>+-=:/?";

#[derive(Debug, Clone)]
pub struct MatrixDropState {
    pub head: i32,
    pub len: i32,
    pub speed: u8,
}

#[derive(Debug, Clone)]
pub struct MatrixScreensaverState {
    pub frame: u64,
    pub seed: u64,
    pub drops: Vec<MatrixDropState>,
}

impl MatrixScreensaverState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED_1234_9ABC_DEF0);
        Self {
            frame: 0,
            seed: now ^ 0xA5A5_5A5A_1337_C0DE,
            drops: Vec::new(),
        }
    }

    fn rand_u32(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        (x & 0xFFFF_FFFF) as u32
    }

    fn rand_i32_between(&mut self, min_inclusive: i32, max_inclusive: i32) -> i32 {
        if max_inclusive <= min_inclusive {
            return min_inclusive;
        }
        let span = (max_inclusive - min_inclusive + 1) as u32;
        min_inclusive + (self.rand_u32() % span) as i32
    }

    fn spawn_drop(&mut self, rows: i32) -> MatrixDropState {
        let len_min = (rows / 8).max(4);
        let len_max = (rows / 3).max(len_min + 1);
        MatrixDropState {
            head: self.rand_i32_between(-rows, 0),
            len: self.rand_i32_between(len_min, len_max),
            speed: self.rand_i32_between(1, 4) as u8,
        }
    }

    pub fn step(&mut self, cols: usize, rows: usize) {
        self.frame = self.frame.wrapping_add(1);
        if cols == 0 || rows == 0 {
            self.drops.clear();
            return;
        }

        let rows_i32 = rows as i32;

        if self.drops.len() < cols {
            let missing = cols - self.drops.len();
            for _ in 0..missing {
                let drop = self.spawn_drop(rows_i32);
                self.drops.push(drop);
            }
        } else if self.drops.len() > cols {
            self.drops.truncate(cols);
        }

        for idx in 0..self.drops.len() {
            let speed = self.drops[idx].speed.max(1) as u64;
            if !self.frame.is_multiple_of(speed) {
                continue;
            }

            self.drops[idx].head += 1;
            if self.drops[idx].head - self.drops[idx].len > rows_i32 {
                self.drops[idx] = self.spawn_drop(rows_i32);
            }
        }
    }
}

fn matrix_char(seed: u64, col: usize, row: usize) -> char {
    let mut x = seed ^ ((col as u64) << 32) ^ ((row as u64) << 16);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;

    if (x & 0x7) != 0 {
        let katakana_start = 0xFF66u32;
        let katakana_end = 0xFF9Du32;
        let span = katakana_end - katakana_start + 1;
        let cp = katakana_start + ((x as u32) % span);
        char::from_u32(cp).unwrap_or('ﾏ')
    } else {
        let idx = (x as usize) % MATRIX_ASCII_GLYPHS.len();
        MATRIX_ASCII_GLYPHS[idx] as char
    }
}

pub fn render(f: &mut Frame, state: &MatrixScreensaverState, area: Rect) {
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let cols = area.width as usize;
    let rows = area.height as usize;
    if cols == 0 || rows == 0 {
        return;
    }

    let mut chars = vec![vec![' '; cols]; rows];
    let mut styles = vec![vec![Style::default().fg(Color::Black).bg(Color::Black); cols]; rows];

    for col in 0..cols.min(state.drops.len()) {
        let drop = &state.drops[col];
        let head = drop.head;
        let tail_start = head - drop.len;

        for y in tail_start..=head {
            if y < 0 || y >= rows as i32 {
                continue;
            }
            let dist = (head - y) as usize;
            let row = y as usize;
            let ch = matrix_char(state.seed, col, row);

            let style = if dist == 0 {
                Style::default()
                    .fg(Color::Rgb(220, 255, 220))
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if dist < 3 {
                Style::default()
                    .fg(Color::Rgb(150, 255, 150))
                    .bg(Color::Black)
            } else if dist < 7 {
                Style::default()
                    .fg(Color::Rgb(60, 210, 60))
                    .bg(Color::Black)
            } else {
                Style::default()
                    .fg(Color::Rgb(20, 120, 20))
                    .bg(Color::Black)
            };

            chars[row][col] = ch;
            styles[row][col] = style;
        }
    }

    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut spans = Vec::new();
        let mut run_style = styles[row][0];
        let mut run = String::new();

        for col in 0..cols {
            let st = styles[row][col];
            if st != run_style {
                if !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = st;
            }
            run.push(chars[row][col]);
        }

        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }

        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), area);

    // Intentionally empty hint line to keep the bottom row subtle.
    let hint = "";
    if area.height > 1 {
        let hint_area = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default()
                    .fg(Color::Rgb(30, 180, 30))
                    .bg(Color::Black),
            )))
            .alignment(Alignment::Center),
            hint_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_keeps_one_drop_per_column() {
        let mut state = MatrixScreensaverState::new();

        state.step(80, 24);
        assert_eq!(state.drops.len(), 80);

        state.step(20, 24);
        assert_eq!(state.drops.len(), 20);

        state.step(0, 24);
        assert!(state.drops.is_empty());
    }

    #[test]
    fn matrix_chars_are_stable_for_a_cell() {
        let seed = 0xA5A5_5A5A_1337_C0DE;

        assert_eq!(matrix_char(seed, 12, 8), matrix_char(seed, 12, 8));
        assert_ne!(matrix_char(seed, 12, 8), matrix_char(seed, 13, 8));
    }
}
