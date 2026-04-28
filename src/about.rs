// ---------------------------------------------------------------------------
// About dialog — demomaker / demoscene style
// ---------------------------------------------------------------------------
// Animated:
//   • KKC logo Y-axis 3D rotation (yellow-gold/cyan flip)
//   • Rainbow scrolling title "Ketchup Killers Commander"
//   • Starfield (3-layer parallax) in the zone above the CODING section
//   • Worm snake bouncing below the CODING section
// Layout: 62×28 floating panel, black bg, cyan double border.
// ---------------------------------------------------------------------------

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

// Dialog dimensions (including border).
const W: u16 = 62;
const H: u16 = 28;

// Inner width = W - 2 borders
const INNER_W: usize = (W - 2) as usize;

// KKC logo: 5×17 binary pixel grid.
//   K(5) gap(1) K(5) gap(1) C(5) = 17 pixel columns
//   Each pixel → 2 chars wide → 34 rendered chars total
const LOGO_PIXELS: [[u8; 17]; 5] = [
    [1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0],
    [1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0],
    [1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0],
    [1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0],
];

// Worm length and characters
const WORM_LEN: usize = 14;

// ---------------------------------------------------------------------------
// Public state (tick incremented by main loop at ~60 fps)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AboutState {
    pub tick: u64,
    // Worm: head position x (0..INNER_W), direction (+1/-1), trail of x positions
    worm_x: i32,
    worm_dir: i32,
    worm_trail: Vec<i32>,
}

impl AboutState {
    pub fn new() -> Self {
        Self {
            tick: 0,
            worm_x: (INNER_W / 2) as i32,
            worm_dir: 1,
            worm_trail: vec![(INNER_W / 2) as i32; WORM_LEN],
        }
    }

    /// Advance worm position (call once per tick from the main loop).
    pub fn step_worm(&mut self) {
        // Move head every 2 ticks for a visible speed
        if self.tick % 2 == 0 {
            let new_x = self.worm_x + self.worm_dir;
            let max = INNER_W as i32 - 1;
            if new_x < 0 || new_x > max {
                self.worm_dir = -self.worm_dir;
            } else {
                self.worm_x = new_x;
            }
            // Push head to trail, drop oldest
            self.worm_trail.insert(0, self.worm_x);
            self.worm_trail.truncate(WORM_LEN);
        }
    }
}

// ---------------------------------------------------------------------------
// Public render entry-point
// ---------------------------------------------------------------------------

pub fn render_about(f: &mut Frame, state: &AboutState, area: Rect) {
    let w = W.min(area.width);
    let h = H.min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let popup = Rect { x, y, width: w, height: h };

    // Drop-shadow (2 right, 1 down)
    let sh = Rect {
        x: popup.x.saturating_add(2),
        y: popup.y.saturating_add(1),
        width: w.min(area.right().saturating_sub(popup.x.saturating_add(2))),
        height: h.min(area.bottom().saturating_sub(popup.y.saturating_add(1))),
    };
    if sh.width > 0 && sh.height > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Rgb(4, 4, 14))),
            sh,
        );
    }

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // ── Animation values ──────────────────────────────────────────────────────
    let angle_rad = (state.tick as f64) * 2.0_f64.to_radians();
    let cos_val   = angle_rad.cos();
    let abs_cos   = cos_val.abs();

    // ── Section separator fills ───────────────────────────────────────────────
    let full_sep   = INNER_W.saturating_sub(4);
    let coding_sep = "─".repeat(full_sep.saturating_sub("CODING".len() + 3));
    let demo_sep   = "─".repeat(full_sep.saturating_sub("DEMOPARTIES".len() + 3));
    let tech_sep   = "─".repeat(full_sep.saturating_sub("TECH".len() + 3));
    let bottom_sep = "─".repeat(INNER_W);

    let version = env!("CARGO_PKG_VERSION");

    let bg = Style::default().fg(Color::Black).bg(Color::Black);

    // ── Build lines ───────────────────────────────────────────────────────────
    // Total visible inner rows = H - 2 = 26
    // Layout:
    //   row  0   : blank (black)
    //   rows 1-5 : logo (3D rotation)
    //   row  6   : blank (black)
    //   row  7   : rainbow title
    //   row  8   : version
    //   row  9   : tagline
    //   rows 10-12: starfield (3 rows × INNER_W cols)
    //   rows 13-14: CODING header + author
    //   row  15  : worm
    //   rows 16-17: DEMOPARTIES header + parties
    //   row  18  : blank
    //   rows 19-21: TECH header + tech + born
    //   row  22  : blank
    //   row  23  : copyright
    //   row  24  : bottom separator
    //   row  25  : close hint

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(26);

    // row 0 — blank
    lines.push(blank_line());

    // rows 1-5 — logo
    for row_idx in 0..5 {
        lines.push(render_logo_row(row_idx, cos_val, abs_cos));
    }

    // row 6 — blank
    lines.push(blank_line());

    // row 7 — rainbow title
    lines.push(rainbow_line("Ketchup Killers Commander", state.tick));

    // row 8 — version
    lines.push(
        Line::from(Span::styled(
            format!("v {version}"),
            Style::default().fg(Color::Rgb(210, 80, 210)),
        ))
        .alignment(Alignment::Center),
    );

    // row 9 — tagline
    lines.push(
        Line::from(Span::styled(
            "Written in Rust  \u{00b7}  runs in any terminal",
            Style::default().fg(Color::Rgb(100, 100, 100)),
        ))
        .alignment(Alignment::Center),
    );

    // rows 10-12 — starfield (3 planes: slow/mid/fast)
    for plane in 0..3u64 {
        lines.push(render_starfield_row(plane, state.tick, INNER_W));
    }

    // rows 13-14 — CODING
    lines.push(Line::from(vec![
        Span::styled("  \u{2500} ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "CODING",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {coding_sep}"),
            Style::default().fg(Color::Rgb(20, 60, 60)),
        ),
    ]));
    lines.push(
        Line::from(Span::styled(
            "RedBug  /  Ketchup Killers",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
    );

    // row 15 — worm
    lines.push(render_worm_row(&state.worm_trail, INNER_W));

    // rows 16-17 — DEMOPARTIES
    lines.push(Line::from(vec![
        Span::styled("  \u{2500} ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "DEMOPARTIES",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {demo_sep}"),
            Style::default().fg(Color::Rgb(20, 60, 60)),
        ),
    ]));
    lines.push(
        Line::from(Span::styled(
            "Saturne\u{2019}97  \u{00b7}  Wired\u{2019}97  \u{00b7}  Mekka/Symposium\u{2019}98",
            Style::default().fg(Color::Rgb(255, 220, 80)),
        ))
        .alignment(Alignment::Center),
    );

    // row 18 — blank
    lines.push(blank_line());

    // rows 19-21 — TECH
    lines.push(Line::from(vec![
        Span::styled("  \u{2500} ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "TECH",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {tech_sep}"),
            Style::default().fg(Color::Rgb(20, 60, 60)),
        ),
    ]));
    lines.push(
        Line::from(Span::styled(
            "Rust  \u{00b7}  ratatui  \u{00b7}  crossterm  \u{00b7}  mlua  \u{00b7}  Lua 5.4",
            Style::default().fg(Color::Rgb(160, 200, 160)),
        ))
        .alignment(Alignment::Center),
    );
    lines.push(
        Line::from(Span::styled(
            "Born 1997 in Watcom C++  \u{00b7}  Reborn 2026 in Rust",
            Style::default().fg(Color::Rgb(100, 100, 100)),
        ))
        .alignment(Alignment::Center),
    );

    // row 22 — blank
    lines.push(blank_line());

    // row 23 — copyright
    lines.push(
        Line::from(Span::styled(
            "\u{00a9} 1997\u{2013}2026  RedBug  /  Ketchup Killers",
            Style::default().fg(Color::Rgb(160, 140, 90)),
        ))
        .alignment(Alignment::Center),
    );

    // row 24 — bottom separator
    lines.push(Line::from(Span::styled(
        bottom_sep,
        Style::default().fg(Color::Rgb(30, 60, 60)),
    )));

    // row 25 — close hint
    lines.push(
        Line::from(Span::styled(
            "any key to close",
            Style::default().fg(Color::Rgb(80, 180, 180)),
        ))
        .alignment(Alignment::Center),
    );

    f.render_widget(
        Paragraph::new(lines).style(bg),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Blank line with explicit black fg+bg to prevent terminal colour bleed
// ---------------------------------------------------------------------------
#[inline]
fn blank_line() -> Line<'static> {
    Line::from(Span::styled(
        "  ",
        Style::default().fg(Color::Black).bg(Color::Black),
    ))
}

// ---------------------------------------------------------------------------
// 3-D Y-axis rotation of one logo row
// ---------------------------------------------------------------------------
// Each pixel p (0..17) is centred at output column:
//   out_centre(p) = 17 + (2·p – 16) · cos_val
// Inverting for output char index c (midpoint c + 0.5):
//   p = ((c + 0.5 – 17) / cos_val + 16) / 2
// Block character intensity depends on abs_cos (face-on = dense, edge = sparse).
// Colour: front face = yellow-gold, back face = cyan (visible when cos<0).
// ---------------------------------------------------------------------------
fn render_logo_row(row_idx: usize, cos_val: f64, abs_cos: f64) -> Line<'static> {
    let px_char = if abs_cos > 0.90 {
        '\u{2588}' // █
    } else if abs_cos > 0.65 {
        '\u{2593}' // ▓
    } else if abs_cos > 0.40 {
        '\u{2592}' // ▒
    } else if abs_cos > 0.15 {
        '\u{2591}' // ░
    } else {
        ' '
    };

    let mut chars = vec![' '; 34];

    if abs_cos > 0.02 {
        let pixels = &LOGO_PIXELS[row_idx];
        for c in 0..34usize {
            let p_float = ((c as f64 + 0.5 - 17.0) / cos_val + 16.0) / 2.0;
            let p_int = p_float.round() as i32;
            if p_int >= 0 && p_int < 17 && pixels[p_int as usize] == 1 {
                chars[c] = px_char;
            }
        }
    }

    let text: String = chars.into_iter().collect();
    let fg = if cos_val >= 0.0 {
        Color::Rgb(255, 220, 0)  // front: yellow-gold
    } else {
        Color::Rgb(0, 215, 255)  // back:  cyan
    };

    Line::from(Span::styled(
        text,
        Style::default().fg(fg).bg(Color::Black).add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Center)
}

// ---------------------------------------------------------------------------
// Rainbow scrolling title
// ---------------------------------------------------------------------------
fn rainbow_line(text: &'static str, tick: u64) -> Line<'static> {
    let len = text.chars().count().max(1);
    let spans: Vec<Span<'static>> = text
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let hue = ((i as f64 / len as f64) + tick as f64 * 0.018) % 1.0;
            let (r, g, b) = hue_to_rgb(hue);
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(Color::Rgb(r, g, b))
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    Line::from(spans).alignment(Alignment::Center)
}

// ---------------------------------------------------------------------------
// Starfield — 3 parallax planes
// ---------------------------------------------------------------------------
// Each "star" is a pseudo-random position derived from (plane, slot, inner_w).
// Stars scroll left at different speeds: plane 0 = slow, 1 = mid, 2 = fast.
// Brightness is fixed per star; twinkle is done by phase-modulating the char.
// ---------------------------------------------------------------------------
fn render_starfield_row(plane: u64, tick: u64, width: usize) -> Line<'static> {
    // Number of stars per row per plane
    let star_count: u64 = match plane {
        0 => 4,  // slowest / farthest: fewest, dimmest
        1 => 6,  // medium
        _ => 8,  // fastest / nearest: most, brightest
    };
    // Scroll speed (chars per N ticks)
    let scroll_period: u64 = match plane {
        0 => 12,
        1 => 6,
        _ => 3,
    };
    let scroll_offset = tick / scroll_period;

    let w = width as u64;
    // Build a char buffer for this row
    let mut row: Vec<(char, Color)> = vec![(' ', Color::Black); width];

    for slot in 0..star_count {
        // Deterministic column for this (plane, slot) pair, then apply scroll
        let base_x = lcg_hash(plane * 1000 + slot * 37) % w;
        let x = ((base_x + scroll_offset) % w) as usize;

        // Twinkle: phase cycles at a per-star rate
        let twinkle_period = 8 + (lcg_hash(plane * 999 + slot) % 12);
        let phase = (tick / twinkle_period) % 4;

        let (ch, brightness) = match (plane, phase) {
            (0, _)    => ('.', 80u8),
            (1, 0)    => ('+', 140),
            (1, _)    => ('.', 90),
            (_, 0)    => ('*', 255),
            (_, 1..=2) => ('+', 200),
            _          => ('.', 120),
        };
        let color = Color::Rgb(brightness, brightness, brightness);
        row[x] = (ch, color);
    }

    let spans: Vec<Span<'static>> = row
        .into_iter()
        .map(|(ch, color)| {
            Span::styled(
                ch.to_string(),
                Style::default().fg(color).bg(Color::Black),
            )
        })
        .collect();

    Line::from(spans)
}

/// Cheap integer hash (linear congruential step) for deterministic star positions.
#[inline]
fn lcg_hash(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 33
}

// ---------------------------------------------------------------------------
// Worm — a snake-like trail of coloured block chars
// ---------------------------------------------------------------------------
fn render_worm_row(trail: &[i32], width: usize) -> Line<'static> {
    let mut row: Vec<(char, Color)> = vec![(' ', Color::Black); width];

    let worm_chars = ['◉', '●', '◎', '○', '◌', '·', '·', '·', '·', ' ', ' ', ' ', ' ', ' '];
    let worm_colors = [
        Color::Rgb(0, 255, 180),   // head: bright green-cyan
        Color::Rgb(0, 220, 150),
        Color::Rgb(0, 180, 120),
        Color::Rgb(0, 140, 90),
        Color::Rgb(0, 100, 70),
        Color::Rgb(0,  70, 50),
        Color::Rgb(0,  50, 40),
        Color::Rgb(0,  40, 30),
    ];

    for (i, &x) in trail.iter().enumerate() {
        let xi = x as usize;
        if xi < width {
            let ch  = worm_chars[i.min(worm_chars.len() - 1)];
            let col = worm_colors[i.min(worm_colors.len() - 1)];
            row[xi] = (ch, col);
        }
    }

    let spans: Vec<Span<'static>> = row
        .into_iter()
        .map(|(ch, color)| {
            Span::styled(
                ch.to_string(),
                Style::default().fg(color).bg(Color::Black),
            )
        })
        .collect();

    Line::from(spans)
}

// ---------------------------------------------------------------------------
// Hue → RGB  (full saturation, full value)
// ---------------------------------------------------------------------------
fn hue_to_rgb(h: f64) -> (u8, u8, u8) {
    let h6 = h * 6.0;
    let i = h6.floor() as u32;
    let f = h6 - h6.floor();
    let (r, g, b) = match i % 6 {
        0 => (1.0_f64, f,       0.0_f64),
        1 => (1.0 - f, 1.0,     0.0),
        2 => (0.0,     1.0,     f),
        3 => (0.0,     1.0 - f, 1.0),
        4 => (f,       0.0,     1.0),
        _ => (1.0,     0.0,     1.0 - f),
    };
    (
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

