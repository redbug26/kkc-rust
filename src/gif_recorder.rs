//! Capture the current ratatui buffer as a GIF frame.
//!
//! Each terminal cell is rendered as a CELL_W × CELL_H pixel block.
//! Non-space cells show a fg-coloured rectangle inside a bg-coloured border,
//! giving a "heat-map" representation that clearly shows the TUI layout.
//!
//! If the destination GIF already exists its frames are preserved and the
//! new frame is appended; the file is then rewritten in full.

use anyhow::Result;
use image::{
    AnimationDecoder, Delay, Frame, RgbaImage,
    codecs::gif::{GifDecoder, GifEncoder, Repeat},
};
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Returns the path where the GIF recording is saved:
/// `<data_dir>/screen.gif` from `ProjectDirs`, falling back to `./data/screen.gif`.
pub fn gif_path() -> PathBuf {
    crate::config::project_dirs()
        .map(|d| d.data_dir().join("screen.gif"))
        .unwrap_or_else(|_| PathBuf::from("data/screen.gif"))
}

/// Width in pixels of a single terminal cell (matches font8x8 glyph width).
const CELL_W: u32 = 8;
/// Height in pixels of a single terminal cell (glyph 8px scaled ×2 vertically).
const CELL_H: u32 = 16;
/// Delay per frame in milliseconds.
const FRAME_DELAY_MS: u64 = 500;

/// Render `buffer` as one GIF frame and append it to `path`.
/// Creates `path` (and its parent directories) if they do not exist yet.
pub fn capture_frame(buffer: &Buffer, path: &Path) -> Result<()> {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let img = render_buffer(buffer);
    let delay = Delay::from_saturating_duration(Duration::from_millis(FRAME_DELAY_MS));
    let new_frame = Frame::from_parts(img, 0, 0, delay);

    // Collect existing frames so we can append.
    let mut frames: Vec<Frame> = Vec::new();
    if path.exists() {
        let file = File::open(path)?;
        let decoder = GifDecoder::new(BufReader::new(file))?;
        frames = decoder
            .into_frames()
            .collect::<Result<Vec<_>, _>>()?;
    }
    frames.push(new_frame);

    // Write all frames back to disk.
    let out = File::create(path)?;
    let mut encoder = GifEncoder::new_with_speed(out, 30);
    encoder.set_repeat(Repeat::Infinite)?;
    for frame in frames {
        encoder.encode_frame(frame)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Render the full terminal buffer into an RGBA image using bitmap glyphs.
fn render_buffer(buffer: &Buffer) -> RgbaImage {
    let area = buffer.area;
    let cols = area.width as u32;
    let rows = area.height as u32;
    let mut img = RgbaImage::new(cols * CELL_W, rows * CELL_H);

    for row in 0..rows {
        for col in 0..cols {
            let x = area.x + col as u16;
            let y = area.y + row as u16;

            let (bg, fg, sym) = if let Some(cell) = buffer.cell((x, y)) {
                (
                    color_to_rgba(cell.bg, false),
                    color_to_rgba(cell.fg, true),
                    cell.symbol().to_owned(),
                )
            } else {
                (
                    image::Rgba([20u8, 20, 20, 255]),
                    image::Rgba([212u8, 212, 212, 255]),
                    String::new(),
                )
            };

            // Fill entire cell with background colour.
            for py in 0..CELL_H {
                for px in 0..CELL_W {
                    img.put_pixel(col * CELL_W + px, row * CELL_H + py, bg);
                }
            }

            // Render the glyph bitmap if the cell is not blank.
            let ch = sym.chars().next().unwrap_or(' ');
            if ch != ' ' {
                if let Some(glyph) = get_glyph(ch) {
                    render_glyph(&mut img, col, row, &glyph, fg, bg);
                }
            }
        }
    }
    img
}

/// Obtain an 8×8 bitmap for `ch` from the embedded font8x8 tables.
/// Returns `None` for unrecognised glyphs (caller falls back to blank).
fn get_glyph(ch: char) -> Option<[u8; 8]> {
    use font8x8::UnicodeFonts;
    let cp = ch as u32;
    if cp < 0x80 {
        font8x8::BASIC_FONTS.get(ch)
    } else if (0x2500..=0x257F).contains(&cp) {
        font8x8::BOX_FONTS.get(ch)
    } else if (0x2580..=0x259F).contains(&cp) {
        font8x8::BLOCK_FONTS.get(ch)
    } else {
        None
    }
}

/// Blit an 8×8 font8x8 glyph into the image at cell (col, row).
/// The glyph is scaled 1×2 vertically to fill a CELL_W×CELL_H (8×16) cell.
/// Each byte in `glyph` is one pixel row; bit 0 = leftmost column.
fn render_glyph(
    img: &mut RgbaImage,
    col: u32,
    row: u32,
    glyph: &[u8; 8],
    fg: image::Rgba<u8>,
    bg: image::Rgba<u8>,
) {
    for (gy, &row_bits) in glyph.iter().enumerate() {
        for gx in 0..8u32 {
            let pixel = if (row_bits & (1 << gx)) != 0 { fg } else { bg };
            let px = col * CELL_W + gx;
            // Scale vertically ×2 so the 8-row glyph fills the 16-row cell.
            let py_base = row * CELL_H + gy as u32 * 2;
            img.put_pixel(px, py_base, pixel);
            img.put_pixel(px, py_base + 1, pixel);
        }
    }
}

// ---------------------------------------------------------------------------
// Colour conversion
// ---------------------------------------------------------------------------

fn color_to_rgba(color: Color, is_fg: bool) -> image::Rgba<u8> {
    let (r, g, b) = match color {
        Color::Reset => {
            if is_fg { (212, 212, 212) } else { (20, 20, 20) }
        }
        Color::Black => (0, 0, 0),
        Color::Red => (128, 0, 0),
        Color::Green => (0, 128, 0),
        Color::Yellow => (128, 128, 0),
        Color::Blue => (0, 0, 168),
        Color::Magenta => (128, 0, 128),
        Color::Cyan => (0, 128, 128),
        Color::Gray => (170, 170, 170),
        Color::DarkGray => (85, 85, 85),
        Color::LightRed => (255, 85, 85),
        Color::LightGreen => (85, 255, 85),
        Color::LightYellow => (255, 255, 85),
        Color::LightBlue => (85, 85, 255),
        Color::LightMagenta => (255, 85, 255),
        Color::LightCyan => (85, 255, 255),
        Color::White => (255, 255, 255),
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed_to_rgb(i),
    };
    image::Rgba([r, g, b, 255])
}

/// Map an ANSI 256-colour index to (r, g, b).
fn indexed_to_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0  => (0, 0, 0),
        1  => (128, 0, 0),
        2  => (0, 128, 0),
        3  => (128, 128, 0),
        4  => (0, 0, 128),
        5  => (128, 0, 128),
        6  => (0, 128, 128),
        7  => (192, 192, 192),
        8  => (128, 128, 128),
        9  => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let v = i - 16;
            let b = v % 6;
            let g = (v / 6) % 6;
            let r = v / 36;
            let scale = |c: u8| if c == 0 { 0 } else { 55 + c * 40 };
            (scale(r), scale(g), scale(b))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}
