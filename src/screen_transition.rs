use ratatui::{
    Frame,
    buffer::{Buffer, Cell},
    layout::{Position, Rect},
    style::{Color, Modifier},
};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScreenTransitionEffect {
    Dither,
    WipeDown,
    WipeUp,
    WipeRight,
    WipeLeft,
    DiagonalBands,
    Checkerboard,
    VerticalBlinds,
    HorizontalBlinds,
    Radial,
    Diamond,
    Spiral,
    Plasma,
    Tunnel,
    Melt,
}

impl ScreenTransitionEffect {
    pub const ALL: [Self; 15] = [
        Self::Dither,
        Self::WipeDown,
        Self::WipeUp,
        Self::WipeRight,
        Self::WipeLeft,
        Self::DiagonalBands,
        Self::Checkerboard,
        Self::VerticalBlinds,
        Self::HorizontalBlinds,
        Self::Radial,
        Self::Diamond,
        Self::Spiral,
        Self::Plasma,
        Self::Tunnel,
        Self::Melt,
    ];

    pub const fn as_config_name(self) -> &'static str {
        match self {
            Self::Dither => "dither",
            Self::WipeDown => "wipe_down",
            Self::WipeUp => "wipe_up",
            Self::WipeRight => "wipe_right",
            Self::WipeLeft => "wipe_left",
            Self::DiagonalBands => "diagonal_bands",
            Self::Checkerboard => "checkerboard",
            Self::VerticalBlinds => "vertical_blinds",
            Self::HorizontalBlinds => "horizontal_blinds",
            Self::Radial => "radial",
            Self::Diamond => "diamond",
            Self::Spiral => "spiral",
            Self::Plasma => "plasma",
            Self::Tunnel => "tunnel",
            Self::Melt => "melt",
        }
    }
}

impl FromStr for ScreenTransitionEffect {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = normalize_effect_name(value);

        for effect in Self::ALL {
            if normalized == normalize_effect_name(effect.as_config_name()) {
                return Ok(effect);
            }
        }

        Err(())
    }
}

fn normalize_effect_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScreenTransitionDirection {
    ToBlack,
    FromBlack,
}

#[derive(Debug, Clone)]
pub struct ScreenTransition {
    effect: ScreenTransitionEffect,
    direction: ScreenTransitionDirection,
    frame: u16,
    frames: u16,
    base: Option<Buffer>,
}

impl ScreenTransition {
    pub fn new(
        effect: ScreenTransitionEffect,
        direction: ScreenTransitionDirection,
        frames: u16,
    ) -> Self {
        Self {
            effect,
            direction,
            frame: 0,
            frames: frames.max(1),
            base: None,
        }
    }

    pub fn to_black(effect: ScreenTransitionEffect, frames: u16, base: Option<Buffer>) -> Self {
        Self {
            base,
            ..Self::new(effect, ScreenTransitionDirection::ToBlack, frames)
        }
    }

    pub fn from_black(effect: ScreenTransitionEffect, frames: u16) -> Self {
        Self::new(effect, ScreenTransitionDirection::FromBlack, frames)
    }

    pub const fn direction(&self) -> ScreenTransitionDirection {
        self.direction
    }

    pub fn advance(&mut self) -> bool {
        self.frame = self.frame.saturating_add(1);
        self.frame >= self.frames
    }

    pub fn render(&self, f: &mut Frame) {
        render_screen_transition(f, self);
    }

    fn progress(&self) -> f32 {
        ((self.frame.saturating_add(1)) as f32 / self.frames as f32).clamp(0.0, 1.0)
    }
}

pub fn render_screen_transition(f: &mut Frame, transition: &ScreenTransition) {
    let area = f.area();
    let buffer = f.buffer_mut();

    if let Some(base) = &transition.base {
        copy_buffer(base, buffer, area);
    }

    apply_black_mask(
        buffer,
        area,
        transition.effect,
        transition.direction,
        transition.progress(),
    );
}

fn copy_buffer(source: &Buffer, target: &mut Buffer, area: Rect) {
    let width = area.width.min(source.area.width);
    let height = area.height.min(source.area.height);

    for y in 0..height {
        for x in 0..width {
            let Some(source_cell) =
                source.cell(Position::new(source.area.x + x, source.area.y + y))
            else {
                continue;
            };
            let Some(target_cell) = target.cell_mut(Position::new(area.x + x, area.y + y)) else {
                continue;
            };
            *target_cell = source_cell.clone();
        }
    }
}

fn apply_black_mask(
    buffer: &mut Buffer,
    area: Rect,
    effect: ScreenTransitionEffect,
    direction: ScreenTransitionDirection,
    progress: f32,
) {
    for y in 0..area.height {
        for x in 0..area.width {
            if should_blacken(effect, direction, progress, x, y, area) {
                blacken(buffer, area.x + x, area.y + y);
            }
        }
    }
}

fn should_blacken(
    effect: ScreenTransitionEffect,
    direction: ScreenTransitionDirection,
    progress: f32,
    x: u16,
    y: u16,
    area: Rect,
) -> bool {
    let threshold = match effect {
        ScreenTransitionEffect::Dither => hash_threshold(x, y),
        ScreenTransitionEffect::WipeDown => axis_threshold(y, area.height),
        ScreenTransitionEffect::WipeUp => {
            axis_threshold(area.height.saturating_sub(1) - y, area.height)
        }
        ScreenTransitionEffect::WipeRight => axis_threshold(x, area.width),
        ScreenTransitionEffect::WipeLeft => {
            axis_threshold(area.width.saturating_sub(1) - x, area.width)
        }
        ScreenTransitionEffect::DiagonalBands => diagonal_bands_threshold(x, y, area),
        ScreenTransitionEffect::Checkerboard => checkerboard_threshold(x, y),
        ScreenTransitionEffect::VerticalBlinds => blinds_threshold(x),
        ScreenTransitionEffect::HorizontalBlinds => blinds_threshold(y),
        ScreenTransitionEffect::Radial => radial_threshold(x, y, area),
        ScreenTransitionEffect::Diamond => diamond_threshold(x, y, area),
        ScreenTransitionEffect::Spiral => spiral_threshold(x, y, area),
        ScreenTransitionEffect::Plasma => plasma_threshold(x, y, area),
        ScreenTransitionEffect::Tunnel => tunnel_threshold(x, y, area),
        ScreenTransitionEffect::Melt => melt_threshold(x, y, area),
    };

    match direction {
        ScreenTransitionDirection::ToBlack => threshold <= progress,
        ScreenTransitionDirection::FromBlack => threshold > progress,
    }
}

fn normalized_cell(x: u16, y: u16, area: Rect) -> (f32, f32) {
    let nx = if area.width <= 1 {
        0.0
    } else {
        x as f32 / (area.width - 1) as f32
    };
    let ny = if area.height <= 1 {
        0.0
    } else {
        y as f32 / (area.height - 1) as f32
    };
    (nx, ny)
}

fn center_delta(x: u16, y: u16, area: Rect) -> (f32, f32) {
    let (nx, ny) = normalized_cell(x, y, area);
    let aspect = if area.height == 0 {
        1.0
    } else {
        area.width.max(1) as f32 / area.height.max(1) as f32
    };
    ((nx - 0.5) * aspect, ny - 0.5)
}

fn axis_threshold(pos: u16, len: u16) -> f32 {
    if len <= 1 {
        0.0
    } else {
        pos as f32 / (len - 1) as f32
    }
}

fn diagonal_bands_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let denom = area.width.saturating_add(area.height).saturating_sub(2);
    if denom == 0 {
        return 0.0;
    }

    let wave = ((x as u32 * 3 + y as u32 * 5) % 11) as f32 / 11.0;
    ((x as f32 + y as f32) / denom as f32 * 0.78 + wave * 0.22).clamp(0.0, 1.0)
}

fn checkerboard_threshold(x: u16, y: u16) -> f32 {
    let block = 2;
    let bx = x / block;
    let by = y / block;
    let parity = ((bx + by) & 1) as f32;
    let jitter = hash_threshold(bx, by) * 0.32;
    (parity * 0.5 + jitter).clamp(0.0, 1.0)
}

fn blinds_threshold(pos: u16) -> f32 {
    let stripe_width = 6;
    let stripe_pos = pos % stripe_width;
    let stripe = pos / stripe_width;
    let local = stripe_pos as f32 / (stripe_width - 1) as f32;
    let stagger = hash_threshold(stripe, stripe) * 0.22;
    (local * 0.78 + stagger).clamp(0.0, 1.0)
}

fn radial_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (dx, dy) = center_delta(x, y, area);
    let aspect = if area.height == 0 {
        1.0
    } else {
        area.width.max(1) as f32 / area.height.max(1) as f32
    };
    let max_dist = ((0.5 * aspect).powi(2) + 0.5_f32.powi(2)).sqrt();
    let dist = (dx * dx + dy * dy).sqrt();
    (dist / max_dist.max(0.001)).clamp(0.0, 1.0)
}

fn diamond_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (nx, ny) = normalized_cell(x, y, area);
    ((nx - 0.5).abs() + (ny - 0.5).abs()).clamp(0.0, 1.0)
}

fn spiral_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (dx, dy) = center_delta(x, y, area);
    let radius = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx) / std::f32::consts::TAU + 0.5;
    (radius * 1.45 + angle * 0.35).fract()
}

fn plasma_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (nx, ny) = normalized_cell(x, y, area);
    let (dx, dy) = center_delta(x, y, area);
    let v = (nx * 17.0).sin()
        + (ny * 13.0).sin()
        + ((dx * dx + dy * dy).sqrt() * 28.0).sin()
        + ((nx + ny) * 11.0).sin();
    ((v + 4.0) / 8.0).clamp(0.0, 1.0)
}

fn tunnel_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (dx, dy) = center_delta(x, y, area);
    let radius = (dx * dx + dy * dy).sqrt().max(0.001);
    let angle = dy.atan2(dx) / std::f32::consts::TAU + 0.5;
    ((1.0 / radius) * 0.12 + angle * 0.35).fract()
}

fn melt_threshold(x: u16, y: u16, area: Rect) -> f32 {
    let (_, ny) = normalized_cell(x, y, area);
    let drift = hash_threshold(x, 0) * 0.42;
    (ny * 0.78 + drift).clamp(0.0, 1.0)
}

fn hash_threshold(x: u16, y: u16) -> f32 {
    let mut value = ((x as u32) << 16) ^ y as u32 ^ 0x9E37_79B9;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    (value & 0xFFFF) as f32 / 0xFFFF as f32
}

fn blacken(buffer: &mut Buffer, x: u16, y: u16) {
    if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
        *cell = Cell::EMPTY.clone();
        cell.set_symbol(" ");
        cell.set_fg(Color::Black);
        cell.set_bg(Color::Black);
        cell.modifier = Modifier::empty();
        cell.skip = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn wipe_down_blackens_top_before_bottom() {
        let area = Rect::new(0, 0, 4, 4);

        assert!(should_blacken(
            ScreenTransitionEffect::WipeDown,
            ScreenTransitionDirection::ToBlack,
            0.4,
            0,
            0,
            area
        ));
        assert!(!should_blacken(
            ScreenTransitionEffect::WipeDown,
            ScreenTransitionDirection::ToBlack,
            0.4,
            0,
            3,
            area
        ));
    }

    #[test]
    fn from_black_inverts_mask() {
        let area = Rect::new(0, 0, 4, 4);

        assert!(!should_blacken(
            ScreenTransitionEffect::WipeRight,
            ScreenTransitionDirection::FromBlack,
            0.4,
            0,
            0,
            area
        ));
        assert!(should_blacken(
            ScreenTransitionEffect::WipeRight,
            ScreenTransitionDirection::FromBlack,
            0.4,
            3,
            0,
            area
        ));
    }

    #[test]
    fn copy_buffer_preserves_cells() {
        let area = Rect::new(0, 0, 2, 1);
        let mut source = Buffer::empty(area);
        source
            .cell_mut(Position::new(1, 0))
            .expect("source cell")
            .set_symbol("X")
            .set_fg(Color::Green);

        let mut target = Buffer::empty(area);
        copy_buffer(&source, &mut target, area);

        let copied = target.cell(Position::new(1, 0)).expect("target cell");
        assert_eq!(copied.symbol(), "X");
        assert_eq!(copied.fg, Color::Green);
    }

    #[test]
    fn effect_names_parse_flexibly() {
        assert_eq!(
            "plasma".parse::<ScreenTransitionEffect>(),
            Ok(ScreenTransitionEffect::Plasma)
        );
        assert_eq!(
            "Diagonal Bands".parse::<ScreenTransitionEffect>(),
            Ok(ScreenTransitionEffect::DiagonalBands)
        );
        assert_eq!(
            "wipe-left".parse::<ScreenTransitionEffect>(),
            Ok(ScreenTransitionEffect::WipeLeft)
        );
        assert!("not_real".parse::<ScreenTransitionEffect>().is_err());
    }

    #[test]
    fn all_effect_thresholds_stay_normalized() {
        let area = Rect::new(0, 0, 80, 24);

        for effect in ScreenTransitionEffect::ALL {
            for &(x, y) in &[(0, 0), (40, 12), (79, 23)] {
                let should_blacken_at_start =
                    should_blacken(effect, ScreenTransitionDirection::ToBlack, 0.0, x, y, area);
                let should_blacken_at_end =
                    should_blacken(effect, ScreenTransitionDirection::ToBlack, 1.0, x, y, area);

                assert!(
                    !should_blacken_at_start || should_blacken_at_end,
                    "{effect:?} should not move from black to visible during ToBlack"
                );
            }
        }
    }
}
