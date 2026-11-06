use super::Result;
use crate::{
    config::Config,
    font::cache::{GlyphCache, GlyphKey},
    terminal::Terminal,
};
use ab_glyph::{Font, FontVec, ScaleFont, point};
use alacritty_terminal::{
    term::{cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape},
};
use std::{fs::File, io::Read, process::Command};

const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;
const ANSI: [u32; 16] = [
    0x181818, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
    0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
];

pub(super) struct Raster {
    fonts: Vec<FontVec>,
    pub cell_width: u32,
    pub cell_height: u32,
    baseline: f32,
    size: f32,
    cache: GlyphCache,
}
impl Raster {
    pub fn load(config: &Config) -> Result<Self> {
        let primary = load_font(&config.font_family)?;
        let mut fonts = vec![primary];
        if let Ok(fallback) = load_font(":charset=7e41")
            && fallback.as_slice() != fonts[0].as_slice()
        {
            fonts.push(fallback);
        }
        Self::new(fonts, config)
    }
    fn new(fonts: Vec<FontVec>, config: &Config) -> Result<Self> {
        config
            .validate()
            .map_err(|_| "invalid raster configuration")?;
        let font = fonts.first().ok_or("no usable font")?;
        let scaled = font.as_scaled(config.font_size as f32);
        let cell_width = scaled.h_advance(scaled.glyph_id('M')).ceil().max(1.0) as u32;
        let cell_height = (scaled.height() + scaled.line_gap()).ceil().max(1.0) as u32;
        let baseline = scaled.ascent();
        if cell_width > 256 || cell_height > 512 {
            return Err("font metrics exceed limit");
        }
        Ok(Self {
            fonts,
            cell_width,
            cell_height,
            baseline,
            size: config.font_size as f32,
            cache: config
                .glyph_cache()
                .map_err(|_| "glyph cache allocation failed")?,
        })
    }
    pub fn draw(
        &mut self,
        terminal: &Terminal,
        size: (u32, u32),
        pixels: &mut [u8],
        focused: bool,
    ) -> Result<()> {
        let (width, height) = size;
        if pixels.len() != width as usize * height as usize * 4 {
            return Err("invalid raster buffer");
        }
        let snapshot = terminal.snapshot().ok_or("terminal unavailable")?;
        let colors = snapshot.content.colors;
        let display_offset = snapshot.content.display_offset;
        let rows = snapshot.rows;
        let cell_height = self.cell_height;
        let viewport_y = |line: i32| {
            let row = display_offset.checked_add_signed(line as isize)?;
            (row < rows).then_some(row as u32 * cell_height)
        };
        let default_bg = color(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
            colors,
        );
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&(0xff000000 | default_bg).to_ne_bytes());
        }
        // Paint backgrounds first so wide cells never erase the preceding glyph.
        for cell in snapshot.content.display_iter {
            let x = cell.point.column.0 as u32 * self.cell_width;
            let Some(y) = viewport_y(cell.point.line.0) else {
                continue;
            };
            let bg = if cell.cell.flags.contains(Flags::INVERSE) {
                cell.cell.fg
            } else {
                cell.cell.bg
            };
            fill(
                pixels,
                size,
                (x, y, self.cell_width, self.cell_height),
                color(bg, colors),
            );
        }
        for cell in terminal
            .snapshot()
            .ok_or("terminal unavailable")?
            .content
            .display_iter
        {
            let flags = cell.cell.flags;
            if flags.intersects(
                Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN,
            ) {
                continue;
            }
            let x = cell.point.column.0 as u32 * self.cell_width;
            let Some(y) = viewport_y(cell.point.line.0) else {
                continue;
            };
            let fg = if flags.contains(Flags::INVERSE) {
                cell.cell.bg
            } else {
                cell.cell.fg
            };
            let fg = color(fg, colors);
            let span = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            for ch in std::iter::once(cell.cell.c)
                .chain(cell.cell.zerowidth().unwrap_or_default().iter().copied())
            {
                if ch == ' ' {
                    continue;
                }
                let font_index =
                    crate::font::select_font(ch, &self.fonts, |font, ch| font.glyph_id(ch).0 != 0)
                        .unwrap_or(0);
                let key = GlyphKey {
                    font_instance: font_index as u64,
                    glyph: ch as u32,
                };
                let mut uncached = None;
                if self.cache.get(key).is_none() {
                    let font = &self.fonts[font_index];
                    let glyph = font
                        .glyph_id(ch)
                        .with_scale_and_position(self.size, point(0.0, self.baseline));
                    let mut mask = vec![0u8; (2 * self.cell_width * self.cell_height) as usize];
                    if let Some(outline) = font.outline_glyph(glyph) {
                        let bounds = outline.px_bounds();
                        if bounds.width() > 1024.0 || bounds.height() > 1024.0 {
                            return Err("glyph bounds exceed limit");
                        }
                        outline.draw(|gx, gy, coverage| {
                            let px = gx as i32 + bounds.min.x as i32;
                            let py = gy as i32 + bounds.min.y as i32;
                            if px >= 0
                                && py >= 0
                                && px < (2 * self.cell_width) as i32
                                && py < self.cell_height as i32
                            {
                                mask[py as usize * (2 * self.cell_width) as usize + px as usize] =
                                    (coverage.clamp(0.0, 1.0) * 255.0) as u8;
                            }
                        });
                    }
                    uncached = self.cache.insert(key, mask.into_boxed_slice());
                }
                let mask = uncached
                    .as_deref()
                    .or_else(|| self.cache.get(key))
                    .ok_or("glyph unavailable")?;
                for gy in 0..self.cell_height.min(height.saturating_sub(y)) {
                    for gx in 0..(span * self.cell_width).min(width.saturating_sub(x)) {
                        let alpha = u32::from(mask[(gy * 2 * self.cell_width + gx) as usize]);
                        if alpha == 0 {
                            continue;
                        }
                        let offset = ((y + gy) * width + x + gx) as usize * 4;
                        let bg = u32::from_ne_bytes(
                            pixels[offset..offset + 4]
                                .try_into()
                                .map_err(|_| "pixel unavailable")?,
                        );
                        let mut result = 0xff000000;
                        for shift in [0, 8, 16] {
                            result |= ((((fg >> shift) & 255) * alpha
                                + ((bg >> shift) & 255) * (255 - alpha))
                                / 255)
                                << shift;
                        }
                        pixels[offset..offset + 4].copy_from_slice(&result.to_ne_bytes());
                    }
                }
            }
            if flags.intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE) {
                fill(
                    pixels,
                    size,
                    (x, y + self.cell_height - 1, span * self.cell_width, 1),
                    fg,
                );
            }
        }
        let cursor = terminal
            .snapshot()
            .ok_or("terminal unavailable")?
            .content
            .cursor;
        if cursor.shape != CursorShape::Hidden
            && let Some(y) = viewport_y(cursor.point.line.0)
        {
            let x = cursor.point.column.0 as u32 * self.cell_width;
            paint_cursor(
                pixels,
                size,
                (x, y, self.cell_width, self.cell_height),
                cursor.shape,
                focused,
            );
        }
        Ok(())
    }
}
fn paint_cursor(
    pixels: &mut [u8],
    size: (u32, u32),
    rect: (u32, u32, u32, u32),
    shape: CursorShape,
    focused: bool,
) {
    let (x, y, w, h) = rect;
    if shape == CursorShape::Hidden {
        return;
    }
    if !focused || shape == CursorShape::HollowBlock {
        for rect in [
            (x, y, w, 1),
            (x, y + h - 1, w, 1),
            (x, y, 1, h),
            (x + w - 1, y, 1, h),
        ] {
            fill(pixels, size, rect, 0xeeeeec);
        }
    } else if shape == CursorShape::Block {
        for row in y..y.saturating_add(h).min(size.1) {
            for col in x..x.saturating_add(w).min(size.0) {
                let offset = (row * size.0 + col) as usize * 4;
                let pixel =
                    u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()) ^ 0x00ffffff;
                pixels[offset..offset + 4].copy_from_slice(&pixel.to_ne_bytes());
            }
        }
    } else if shape == CursorShape::Beam {
        fill(pixels, size, (x, y, 1, h), 0xeeeeec);
    } else if shape == CursorShape::Underline {
        fill(pixels, size, (x, y + h - 1, w, 1), 0xeeeeec);
    }
}

fn load_font(pattern: &str) -> Result<FontVec> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}\n", "--", pattern])
        .output()
        .map_err(|_| "Fontconfig fc-match unavailable")?;
    if !output.status.success() || output.stdout.len() > 8192 {
        return Err("Fontconfig lookup failed");
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|_| "invalid Fontconfig response")?;
    let mut lines = text.lines();
    let path = lines.next().ok_or("font path missing")?;
    let index = lines
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or("font index missing")?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "font file unavailable")?
        .take(MAX_FONT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "font read failed")?;
    if bytes.len() as u64 > MAX_FONT_BYTES {
        return Err("font file exceeds limit");
    }
    FontVec::try_from_vec_and_index(bytes, index).map_err(|_| "unsupported font file")
}
fn color(value: Color, colors: &Colors) -> u32 {
    let index = match value {
        Color::Spec(rgb) => {
            return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
        }
        Color::Named(n) => n as usize,
        Color::Indexed(n) => n as usize,
    };
    if let Some(rgb) = colors[index] {
        return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
    }
    match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = index - 16;
            let level = |v| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
        }
        232..=255 => {
            let level = 8 + 10 * (index as u32 - 232);
            level * 0x010101
        }
        257 => 0x181818,
        259..=266 => ANSI[index - 259],
        _ => 0xd3d7cf,
    }
}
fn fill(pixels: &mut [u8], size: (u32, u32), rect: (u32, u32, u32, u32), color: u32) {
    let (x, y, w, h) = rect;
    for row in y..y.saturating_add(h).min(size.1) {
        for col in x..x.saturating_add(w).min(size.0) {
            let offset = (row * size.0 + col) as usize * 4;
            pixels[offset..offset + 4].copy_from_slice(&(0xff000000 | color).to_ne_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_history_rows_use_display_offset() {
        use alacritty_terminal::grid::Scroll;
        let font = FontVec::try_from_vec(include_bytes!("../../tests/fixtures/demo.ttf").to_vec())
            .unwrap();
        let mut raster = Raster::new(vec![font], &Config::default()).unwrap();
        let tab = crate::TabId(1);
        let mut history = Terminal::new(tab, 3, 8).unwrap();
        let first = "\x1b[?25l\x1b[41mA中e\u{301} BCD\x1b[42mEF中G HI\x1b[44mJKLMN OP";
        history.feed(tab, first.as_bytes()).unwrap();
        let render = |raster: &mut Raster, terminal: &Terminal, rows, columns| {
            let size = (columns * raster.cell_width, rows * raster.cell_height);
            let mut pixels = vec![0x55; (size.0 * size.1 * 4) as usize];
            raster.draw(terminal, size, &mut pixels, true).unwrap();
            pixels
        };
        let expected = render(&mut raster, &history, 3, 8);
        history
            .feed(tab, b"\r\n\x1b[0mnew\r\nmore\r\nlast")
            .unwrap();
        history.scroll_viewport(Scroll::Top).unwrap();
        assert_eq!(history.snapshot().unwrap().content.display_offset, 3);
        assert_eq!(render(&mut raster, &history, 3, 8), expected);

        // Reflow the same original soft-wrapped CJK/combining text into six columns.
        let mut reference = Terminal::new(tab, 4, 6).unwrap();
        reference.feed(tab, first.as_bytes()).unwrap();
        history.resize(4, 6).unwrap();
        history.scroll_viewport(Scroll::Top).unwrap();
        assert_eq!(
            render(&mut raster, &history, 4, 6),
            render(&mut raster, &reference, 4, 6)
        );
        history.scroll_viewport(Scroll::Bottom).unwrap();
        let bottom = render(&mut raster, &history, 4, 6);
        history.scroll_viewport(Scroll::Top).unwrap();
        history.scroll_viewport(Scroll::Bottom).unwrap();
        assert_eq!(render(&mut raster, &history, 4, 6), bottom);

        // The live cursor is a grid point: it must move down by display_offset,
        // or disappear entirely when it is below the visible grid, even with padding.
        let mut cursor = Terminal::new(tab, 3, 8).unwrap();
        cursor.feed(tab, b"\r\n\r\n\r\n\r\n").unwrap();
        cursor.scroll_viewport(Scroll::Top).unwrap();
        let visible = render(&mut raster, &cursor, 5, 8);
        cursor.feed(tab, b"\x1b[?25l").unwrap();
        assert_eq!(visible, render(&mut raster, &cursor, 5, 8));
        cursor.feed(tab, b"\x1b[?25h\x1b[H").unwrap();
        cursor.scroll_viewport(Scroll::Bottom).unwrap();
        cursor.scroll_viewport(Scroll::Delta(1)).unwrap();
        let shifted = render(&mut raster, &cursor, 3, 8);
        cursor.scroll_viewport(Scroll::Bottom).unwrap();
        cursor.feed(tab, b"\x1b[2;1H").unwrap();
        assert_eq!(shifted, render(&mut raster, &cursor, 3, 8));
    }
    #[test]
    fn runtime_cursor_matches_focus_and_shape() {
        let mut pixels = vec![0; 8 * 8 * 4];
        paint_cursor(&mut pixels, (8, 8), (1, 1, 6, 6), CursorShape::Block, true);
        assert_ne!(&pixels[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[0; 4]);
        pixels.fill(0);
        paint_cursor(&mut pixels, (8, 8), (1, 1, 6, 6), CursorShape::Block, false);
        assert_eq!(&pixels[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[0; 4]);
        assert_ne!(&pixels[(8 + 1) * 4..(8 + 1) * 4 + 4], &[0; 4]);
        pixels.fill(0);
        paint_cursor(&mut pixels, (8, 8), (1, 1, 6, 6), CursorShape::Hidden, true);
        assert!(pixels.iter().all(|b| *b == 0));
    }
    #[test]
    fn runtime_raster_draws_terminal_cells() {
        let config = crate::config::Config::default();
        let font = FontVec::try_from_vec(include_bytes!("../../tests/fixtures/demo.ttf").to_vec())
            .unwrap();
        let mut raster = Raster::new(vec![font], &config).unwrap();
        let mut terminal = crate::terminal::Terminal::new(crate::TabId(1), 2, 8).unwrap();
        terminal
            .feed(crate::TabId(1), b"\x1b[31;44mA\x1b[0m\x1b[?25l")
            .unwrap();
        let mut pixels = vec![0; 128 * 64 * 4];
        raster
            .draw(&terminal, (128, 64), &mut pixels, true)
            .unwrap();
        let colors: std::collections::BTreeSet<_> = pixels
            .chunks_exact(4)
            .map(|p| u32::from_ne_bytes(p.try_into().unwrap()))
            .collect();
        assert!(colors.contains(&0xff3465a4));
        assert!(
            colors.len() > 3,
            "glyph antialiasing must produce real pixels"
        );
        assert!(!raster.cache.is_empty());
        assert!(
            raster
                .draw(&terminal, (128, 64), &mut pixels[..16], true)
                .is_err()
        );
    }
}
