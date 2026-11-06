use super::Result;
use crate::{
    config::{Config, palette::Palette},
    font::cache::{GlyphCache, GlyphKey},
    renderer::palette::color,
    terminal::Terminal,
};
use ab_glyph::{Font, FontVec, ScaleFont, point};
use alacritty_terminal::{
    term::{cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape},
};
use unicode_width::UnicodeWidthChar;

pub(super) struct Raster {
    fonts: Vec<FontVec>,
    pub cell_width: u32,
    pub cell_height: u32,
    logical_cell: (u32, u32),
    logical_baseline: f32,
    logical_size: f32,
    scale: u32,
    baseline: f32,
    size: f32,
    cache: GlyphCache,
    pub(super) palette: Palette,
}
impl Raster {
    pub fn load(config: &Config) -> Result<Self> {
        Self::new(super::fonts::load(&config.font_family)?, config)
    }
    fn new(fonts: Vec<FontVec>, config: &Config) -> Result<Self> {
        config
            .validate()
            .map_err(|_| "invalid raster configuration")?;
        if fonts.len() > super::fonts::MAX_FONTS {
            return Err("too many fonts");
        }
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
            palette: config.colors,
            cell_width,
            cell_height,
            logical_cell: (cell_width, cell_height),
            logical_baseline: baseline,
            logical_size: config.font_size as f32,
            scale: 1,
            baseline,
            size: config.font_size as f32,
            cache: config
                .glyph_cache()
                .map_err(|_| "glyph cache allocation failed")?,
        })
    }
    pub(super) fn logical_cell(&self) -> (u32, u32) {
        self.logical_cell
    }
    pub(super) fn set_scale(&mut self, scale: u32) {
        if self.scale == scale {
            return;
        }
        self.cell_width = self.logical_cell.0 * scale;
        self.cell_height = self.logical_cell.1 * scale;
        self.baseline = self.logical_baseline * scale as f32;
        self.size = self.logical_size * scale as f32;
        self.scale = scale;
        self.cache.clear();
    }
    fn draw_glyph(
        &mut self,
        pixels: &mut [u8],
        size: (u32, u32),
        cell: (u32, u32, u32),
        ch: char,
        fg: u32,
        flags: Flags,
    ) -> Result<()> {
        let (width, height) = size;
        let (x, y, span) = cell;
        if ch == ' ' {
            return Ok(());
        }
        // This raster owns an immutable font set and its cache. Scale changes clear it;
        // the scalar selects the face; span and synthetic style affect the mask.
        let key = GlyphKey {
            font_instance: u64::from(span) | (u64::from((flags & Flags::BOLD_ITALIC).bits()) << 32),
            glyph: ch as u32,
        };
        let mut uncached = None;
        if self.cache.get(key).is_none() {
            let mut mask = vec![0u8; (2 * self.cell_width * self.cell_height) as usize];
            let mut mapped = false;
            let outline = self.fonts.iter().find_map(|font| {
                let id = font.glyph_id(ch);
                if id.0 == 0 {
                    return None;
                }
                mapped = true;
                font.outline_glyph(id.with_scale_and_position(self.size, point(0.0, self.baseline)))
            });
            let missing =
                outline.is_none() && !ch.is_whitespace() && !(mapped && ch.width() == Some(0));
            if let Some(outline) = outline {
                let bounds = outline.px_bounds();
                if ![bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                    .into_iter()
                    .all(f32::is_finite)
                    || bounds.width() > 1024.0
                    || bounds.height() > 1024.0
                {
                    return Err("glyph bounds exceed limit");
                }
                // shortcut: center scalar marks over the owning cell; no shaping/anchor tables.
                let left = if ch.width() == Some(0) {
                    ((span * self.cell_width) as f32 - bounds.width()) / 2.0
                } else {
                    bounds.min.x
                };
                outline.draw(|gx, gy, coverage| {
                    let px = (gx as i64).saturating_add(left.floor() as i64);
                    let py = (gy as i64).saturating_add(bounds.min.y as i64);
                    if px >= 0
                        && py >= 0
                        && px < i64::from(span * self.cell_width)
                        && py < i64::from(self.cell_height)
                    {
                        mask[py as usize * (2 * self.cell_width) as usize + px as usize] =
                            (coverage.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                });
            }
            if missing {
                // Do not silently render glyph zero: even fonts with an empty .notdef get a box.
                let width = span * self.cell_width;
                let inset = self
                    .scale
                    .min((width.saturating_sub(1)) / 2)
                    .min((self.cell_height.saturating_sub(1)) / 2);
                for py in inset..self.cell_height - inset {
                    for px in inset..width - inset {
                        if px < inset + self.scale
                            || px >= width.saturating_sub(inset + self.scale)
                            || py < inset + self.scale
                            || py >= self.cell_height.saturating_sub(inset + self.scale)
                        {
                            mask[(py * 2 * self.cell_width + px) as usize] = 255;
                        }
                    }
                }
            }
            if flags.intersects(Flags::BOLD_ITALIC) {
                // shortcut: bounded synthetic styles; use designed faces only with measured need.
                let width = (span * self.cell_width) as usize;
                let mut row = vec![0; width];
                for gy in 0..self.cell_height {
                    let offset = (gy * 2 * self.cell_width) as usize;
                    row.copy_from_slice(&mask[offset..offset + width]);
                    mask[offset..offset + width].fill(0);
                    let slant = if flags.contains(Flags::ITALIC) {
                        (self.baseline as i64 - i64::from(gy)) / 5
                    } else {
                        0
                    };
                    let weight = if flags.contains(Flags::BOLD) {
                        self.scale
                    } else {
                        0
                    };
                    for (gx, alpha) in row.iter().copied().enumerate().filter(|(_, a)| *a != 0) {
                        for extra in 0..=weight {
                            let px = gx as i64 + slant + i64::from(extra);
                            if (0..width as i64).contains(&px) {
                                let target = &mut mask[offset + px as usize];
                                *target = (*target).max(alpha);
                            }
                        }
                    }
                }
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
        Ok(())
    }

    pub fn draw(
        &mut self,
        terminal: &Terminal,
        size: (u32, u32),
        pixels: &mut [u8],
        focused: bool,
        cursor_visible: bool,
    ) -> Result<()> {
        self.draw_rows(terminal, size, pixels, focused, cursor_visible, 0..size.1)
    }

    // Whole cell rows preserve wide/combining glyph ownership without a per-pixel scissor.
    pub(super) fn draw_rows(
        &mut self,
        terminal: &Terminal,
        size: (u32, u32),
        pixels: &mut [u8],
        focused: bool,
        cursor_visible: bool,
        rows: std::ops::Range<u32>,
    ) -> Result<()> {
        let (width, height) = size;
        if pixels.len() != width as usize * height as usize * 4 {
            return Err("invalid raster buffer");
        }
        if rows.start > rows.end
            || rows.end > height
            || !rows.start.is_multiple_of(self.cell_height)
            || (rows.end != height && !rows.end.is_multiple_of(self.cell_height))
        {
            return Err("invalid raster damage rows");
        }
        let damage_rows = rows;
        let snapshot = terminal.snapshot().ok_or("terminal unavailable")?;
        let colors = snapshot.content.colors;
        let cursor = snapshot.content.cursor;
        let block_cursor = cursor_visible && focused && cursor.shape == CursorShape::Block;
        let display_offset = snapshot.content.display_offset;
        let rows = snapshot.rows;
        let cell_height = self.cell_height;
        let viewport_y = |line: i32| {
            let row = display_offset.checked_add_signed(line as isize)?;
            let y = row as u32 * cell_height;
            (row < rows && damage_rows.contains(&y)).then_some(y)
        };
        let default_bg = color(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
            colors,
            &self.palette,
        );
        let stride = width as usize * 4;
        for pixel in pixels[damage_rows.start as usize * stride..damage_rows.end as usize * stride]
            .chunks_exact_mut(4)
        {
            pixel.copy_from_slice(&(0xff000000 | default_bg).to_ne_bytes());
        }
        let mut block_rect = None;
        // Paint backgrounds first so wide cells never erase the preceding glyph.
        for cell in snapshot.content.display_iter {
            let x = cell.point.column.0 as u32 * self.cell_width;
            let Some(y) = viewport_y(cell.point.line.0) else {
                continue;
            };
            let bg = if cell.cell.flags.contains(Flags::INVERSE) {
                foreground(cell.cell.fg, cell.cell.flags, colors, &self.palette)
            } else {
                color(cell.cell.bg, colors, &self.palette)
            };
            fill(pixels, size, (x, y, self.cell_width, self.cell_height), bg);
            if block_cursor && cell.point == cursor.point {
                let span = if cell.cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                block_rect = Some((x, y, span * self.cell_width, self.cell_height));
            }
        }
        if let Some(rect) = block_rect {
            fill(
                pixels,
                size,
                rect,
                color(
                    Color::Named(alacritty_terminal::vte::ansi::NamedColor::Cursor),
                    colors,
                    &self.palette,
                ),
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
            let cursor_cell = block_cursor && cell.point == cursor.point;
            let fg = if cursor_cell {
                default_bg
            } else if flags.contains(Flags::INVERSE) {
                color(cell.cell.bg, colors, &self.palette)
            } else {
                foreground(cell.cell.fg, flags, colors, &self.palette)
            };
            let span = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            for ch in std::iter::once(cell.cell.c)
                .chain(cell.cell.zerowidth().unwrap_or_default().iter().copied())
            {
                self.draw_glyph(pixels, size, (x, y, span), ch, fg, flags)?;
            }
            paint_underline(
                pixels,
                size,
                (x, y, span * self.cell_width, self.cell_height),
                flags,
                self.scale,
                if cursor_cell {
                    fg
                } else {
                    cell.cell
                        .underline_color()
                        .map(|value| color(value, colors, &self.palette))
                        .unwrap_or(fg)
                },
            );
            if flags.contains(Flags::STRIKEOUT) {
                fill(
                    pixels,
                    size,
                    (
                        x,
                        y + self.cell_height / 2,
                        span * self.cell_width,
                        self.scale.min(self.cell_height - self.cell_height / 2),
                    ),
                    fg,
                );
            }
        }
        let cursor = terminal
            .snapshot()
            .ok_or("terminal unavailable")?
            .content
            .cursor;
        if cursor_visible
            && !block_cursor
            && cursor.shape != CursorShape::Hidden
            && let Some(y) = viewport_y(cursor.point.line.0)
        {
            let x = cursor.point.column.0 as u32 * self.cell_width;
            paint_cursor(
                pixels,
                size,
                (x, y, self.cell_width, self.cell_height),
                cursor.shape,
                focused,
                color(
                    Color::Named(alacritty_terminal::vte::ansi::NamedColor::Cursor),
                    colors,
                    &self.palette,
                ),
            );
        }
        Ok(())
    }
}
// DIM affects the source foreground before inverse swaps foreground/background.
fn foreground(value: Color, flags: Flags, colors: &Colors, palette: &Palette) -> u32 {
    let value = color(value, colors, palette);
    if flags.contains(Flags::DIM) {
        (value & 0xfefefe) >> 1
    } else {
        value
    }
}

fn paint_underline(
    pixels: &mut [u8],
    size: (u32, u32),
    rect: (u32, u32, u32, u32),
    flags: Flags,
    scale: u32,
    color: u32,
) {
    if !flags.intersects(Flags::ALL_UNDERLINES) {
        return;
    }
    let (x, y, width, height) = rect;
    let thickness = scale.min(height);
    let bottom = height - thickness;
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        for offset in [bottom, height.saturating_sub(3 * thickness)] {
            fill(pixels, size, (x, y + offset, width, thickness), color);
        }
    } else {
        for px in x..x.saturating_add(width).min(size.0) {
            let phase = px / scale;
            if (flags.contains(Flags::DOTTED_UNDERLINE) && !phase.is_multiple_of(2))
                || (flags.contains(Flags::DASHED_UNDERLINE) && phase % 6 >= 4)
            {
                continue;
            }
            let wave = if flags.contains(Flags::UNDERCURL) {
                [0, 1, 2, 1][phase as usize % 4] * thickness
            } else {
                0
            };
            fill(
                pixels,
                size,
                (px, y + bottom.saturating_sub(wave), 1, thickness),
                color,
            );
        }
    }
}

fn paint_cursor(
    pixels: &mut [u8],
    size: (u32, u32),
    rect: (u32, u32, u32, u32),
    shape: CursorShape,
    focused: bool,
    cursor_color: u32,
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
            fill(pixels, size, rect, cursor_color);
        }
    } else if shape == CursorShape::Block {
        fill(pixels, size, rect, cursor_color);
    } else if shape == CursorShape::Beam {
        fill(pixels, size, (x, y, 1, h), cursor_color);
    } else if shape == CursorShape::Underline {
        fill(pixels, size, (x, y + h - 1, w, 1), cursor_color);
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
    const CURSOR: u32 = 0xeeeeec;
    #[test]
    fn runtime_partial_damage_preserves_other_pixels() {
        let font = FontVec::try_from_vec(
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
        )
        .unwrap();
        let mut raster = Raster::new(vec![font], &Config::default()).unwrap();
        let size = (raster.cell_width * 8, raster.cell_height * 4 + 3);
        let mut terminal = Terminal::with_scrollback(crate::TabId(1), 4, 8, 0).unwrap();
        terminal
            .feed(crate::TabId(1), b"\x1b[?25lA\x1b[2;1HB\x1b[3;1HC")
            .unwrap();
        let mut pixels = vec![0x55; (size.0 * size.1 * 4) as usize];
        raster
            .draw_rows(
                &terminal,
                size,
                &mut pixels,
                true,
                true,
                raster.cell_height..2 * raster.cell_height,
            )
            .unwrap();
        let stride = size.0 as usize * 4;
        assert!(
            pixels[..raster.cell_height as usize * stride]
                .iter()
                .all(|&p| p == 0x55)
        );
        assert!(
            pixels[2 * raster.cell_height as usize * stride..]
                .iter()
                .all(|&p| p == 0x55)
        );
        let preserved = pixels.clone();
        for rows in [
            1..raster.cell_height,
            0..size.1 + 1,
            std::ops::Range {
                start: raster.cell_height,
                end: 0,
            },
        ] {
            assert!(
                raster
                    .draw_rows(&terminal, size, &mut pixels, true, true, rows)
                    .is_err()
            );
            assert_eq!(pixels, preserved, "invalid damage cannot modify pixels");
        }
        raster
            .draw_rows(&terminal, size, &mut pixels, true, true, 0..0)
            .unwrap();
        assert_eq!(pixels, preserved);

        // A retained four-row grid can be clipped to one row plus three pixels.
        let clipped = (size.0, raster.cell_height + 3);
        let mut pixels = vec![0; (clipped.0 * clipped.1 * 4) as usize];
        raster
            .draw(&terminal, clipped, &mut pixels, true, true)
            .unwrap();
        terminal.damage_rows();
        terminal.damage_presented();
        for text in [b"\x1b[2;1HQ\x1b[?25h".as_slice(), b"\x1b[4;1H", b"Z"] {
            terminal.feed(crate::TabId(1), text).unwrap();
            let rows = terminal.damage_rows().unwrap();
            if let Some(rows) =
                super::super::damage::Band::terminal(rows, 4, clipped.1, raster.cell_height)
            {
                raster
                    .draw_rows(&terminal, clipped, &mut pixels, true, true, rows.range())
                    .unwrap();
            }
            let mut full = vec![0; pixels.len()];
            raster
                .draw(&terminal, clipped, &mut full, true, true)
                .unwrap();
            assert_eq!(
                pixels, full,
                "retained-grid damage is clipped to visible pixels"
            );
            terminal.damage_presented();
        }
    }

    #[test]
    fn runtime_partial_frames_match_full_raster_and_reused_buffers() {
        use super::super::{bar, damage, notice, selection_overlay};
        use crate::{
            TabId,
            selection::Screen,
            tabs::{SelectionAction, TabPty, TabRegistry},
        };
        use alacritty_terminal::index::{Column, Line, Point};
        struct Fake;
        impl TabPty for Fake {
            fn tab(&self) -> TabId {
                TabId(1)
            }
        }
        let fonts = [
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").as_slice(),
            include_bytes!("../../tests/fixtures/fallback-cjk.ttf").as_slice(),
            include_bytes!("../../tests/fixtures/fallback-mark.ttf").as_slice(),
        ]
        .into_iter()
        .map(|f| FontVec::try_from_vec(f.to_vec()).unwrap())
        .collect();
        let mut raster = Raster::new(fonts, &Config::default()).unwrap();
        let mut tabs = TabRegistry::<Fake>::with_scrollback(20).unwrap();
        tabs.insert(TabId(1), 6, 12).unwrap();
        tabs.insert(TabId(2), 6, 12).unwrap();
        let cell_height = raster.cell_height;
        let mut damage = damage::State::default();
        let size = (
            12 * raster.cell_width,
            6 * raster.cell_height + raster.cell_height + 8 + 3,
        );
        let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
        let mut slots = [pixels.clone(), pixels.clone()];
        let mut index = 0;
        let mut frame = |tabs: &mut TabRegistry<Fake>,
                         visible,
                         focused,
                         message: Option<notice::Notice>| {
            let id = tabs.active().unwrap();
            let layout = bar::Layout::new(tabs, size, (raster.cell_width, raster.cell_height), 1);
            let terminal = tabs.get_mut(id).unwrap().terminal_mut();
            let total_rows = terminal.snapshot().unwrap().rows;
            let changed = terminal.damage_rows().and_then(|rows| {
                damage::Band::terminal(rows, total_rows, layout.content.1, raster.cell_height)
            });
            let tab = tabs.get(id).unwrap();
            let mut overlays = selection_overlay::bounds(tab, raster.cell_height)
                .map(|b| b.shifted(layout.height));
            if let Some(message) = message {
                let height = notice::height(size, (raster.cell_width, raster.cell_height), message);
                damage::Band::add(&mut overlays, damage::Band::new(size.1 - height, size.1));
            }
            let plan = damage.prepare(
                damage::View {
                    tab: id,
                    size,
                    cell: (raster.cell_width, raster.cell_height),
                    scale: 1,
                    origin: layout.height,
                    titles: tabs
                        .ids()
                        .map(|id| (id, tabs.get(id).unwrap().title().to_owned()))
                        .collect(),
                },
                changed,
                overlays,
            );
            let offset = (size.0 * layout.height * 4) as usize;
            let before = pixels.clone();
            if let Some(rows) = plan.content {
                raster
                    .draw_rows(
                        tab.terminal(),
                        layout.content,
                        &mut pixels[offset..],
                        focused,
                        visible,
                        rows.range(),
                    )
                    .unwrap();
                selection_overlay::draw_rows(
                    tab,
                    layout.content,
                    (raster.cell_width, raster.cell_height),
                    &mut pixels[offset..],
                    rows.range(),
                )
                .unwrap();
            }
            if plan.bar {
                layout
                    .draw(tabs, &mut raster, size, &mut pixels[..offset])
                    .unwrap();
            }
            if let Some(message) = message {
                notice::draw(&mut raster, size, &mut pixels, message).unwrap();
            }
            for y in 0..size.1 {
                if !plan
                    .surface
                    .iter()
                    .flatten()
                    .any(|b| b.range().contains(&y))
                {
                    let start = (y * size.0 * 4) as usize;
                    assert_eq!(
                        &pixels[start..start + (size.0 * 4) as usize],
                        &before[start..start + (size.0 * 4) as usize]
                    );
                }
            }
            let mut full = vec![0; pixels.len()];
            raster
                .draw(
                    tab.terminal(),
                    layout.content,
                    &mut full[offset..],
                    focused,
                    visible,
                )
                .unwrap();
            selection_overlay::draw(
                tab,
                layout.content,
                (raster.cell_width, raster.cell_height),
                &mut full[offset..],
            )
            .unwrap();
            layout
                .draw(tabs, &mut raster, size, &mut full[..offset])
                .unwrap();
            if let Some(message) = message {
                notice::draw(&mut raster, size, &mut full, message).unwrap();
            }
            assert_eq!(pixels, full, "partial frame differs from full composition");
            if let Some(stale) = damage.stale(index) {
                let start = (stale.start * size.0 * 4) as usize;
                let end = (stale.end * size.0 * 4) as usize;
                slots[index][start..end].copy_from_slice(&pixels[start..end]);
            }
            assert_eq!(slots[index], full, "alternating slot retains stale pixels");
            damage.submitted(index);
            tabs.get_mut(id).unwrap().terminal_mut().damage_presented();
            index = 1 - index;
            plan
        };
        let feed = |tabs: &mut TabRegistry<Fake>, bytes: &[u8]| {
            let id = tabs.active().unwrap();
            tabs.get_mut(id)
                .unwrap()
                .terminal_mut()
                .feed(id, bytes)
                .unwrap();
        };
        feed(
            &mut tabs,
            "\x1b[?25lA\x1b[2;1H中e\u{301}\x1b[3;1H\x1b[1;3;4:3;9mAA\x1b[0m\x1b[4;1HZZ".as_bytes(),
        );
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b[4;1HQ");
        let one = frame(&mut tabs, true, true, None);
        assert!(!one.bar);
        assert_eq!(
            one.content.unwrap().end - one.content.unwrap().start,
            cell_height
        );
        for bytes in [
            "\x1b[2;2H ",
            "\x1b[2;1H中e\u{301}",
            "\x1b[2;4H\u{301}",
            "\x1b[3;1H\x1b[K",
            "\x1b[?25h\x1b[2;1H",
            "\x1b[3 q",
            "\x1b[5 q",
        ] {
            feed(&mut tabs, bytes.as_bytes());
            frame(&mut tabs, true, true, None);
        }
        frame(&mut tabs, false, true, None);
        frame(&mut tabs, true, false, None);
        frame(&mut tabs, true, true, None);
        let point = Point::new(Line(1), Column(1));
        let tab = tabs.get_mut(TabId(1)).unwrap();
        tab.select(Screen::Main, SelectionAction::Press, point);
        tab.select(Screen::Main, SelectionAction::Release, point);
        frame(&mut tabs, true, true, None);
        frame(&mut tabs, false, true, None); // Repainting selection must not XOR it twice.
        let point = Point::new(Line(3), Column(0));
        let tab = tabs.get_mut(TabId(1)).unwrap();
        tab.select(Screen::Main, SelectionAction::Press, point);
        tab.select(Screen::Main, SelectionAction::Release, point);
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b[4;2HB"); // Content epoch clears the prior selection.
        frame(&mut tabs, true, true, None);
        frame(&mut tabs, true, true, Some(notice::Notice::TransferFailed));
        frame(&mut tabs, true, true, Some(notice::Notice::Empty));
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b]10;#123456\x07");
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b]110\x07");
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b[?1049hOther screen");
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\x1b[?1049l");
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7");
        frame(&mut tabs, true, true, None);
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .scroll_viewport(alacritty_terminal::grid::Scroll::Top)
            .unwrap();
        frame(&mut tabs, true, true, None);
        feed(&mut tabs, b"\r\nmore");
        frame(&mut tabs, true, true, None);
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .scroll_viewport(alacritty_terminal::grid::Scroll::Bottom)
            .unwrap();
        frame(&mut tabs, true, true, None);
        assert!(tabs.activate(TabId(2)));
        feed(&mut tabs, b"Other tab");
        frame(&mut tabs, true, true, None);
        assert!(tabs.activate(TabId(1)));
        frame(&mut tabs, true, true, None);
    }

    #[test]
    fn runtime_cursor_blink_preserves_pixels_and_grid() {
        let font = FontVec::try_from_vec(
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
        )
        .unwrap();
        let mut raster = Raster::new(vec![font], &Config::default()).unwrap();
        let tab = crate::TabId(1);
        let mut terminal = Terminal::new(tab, 2, 8).unwrap();
        terminal.feed(tab, b"AB\x1b[H").unwrap();
        for scale in [1, 2, 1] {
            raster.set_scale(scale);
            let size = (8 * raster.cell_width, 2 * raster.cell_height);
            let render = |raster: &mut Raster, terminal: &Terminal, visible, focused| {
                let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
                raster
                    .draw(terminal, size, &mut pixels, focused, visible)
                    .unwrap();
                pixels
            };
            terminal.feed(tab, b"\x1b[?25l").unwrap();
            let no_cursor = render(&mut raster, &terminal, true, true);
            terminal.feed(tab, b"\x1b[?25h").unwrap();
            for shape in [1, 3, 5] {
                terminal
                    .feed(tab, format!("\x1b[{shape} q").as_bytes())
                    .unwrap();
                let grid = terminal.grid().unwrap().clone();
                let visible = render(&mut raster, &terminal, true, true);
                assert_ne!(visible, no_cursor);
                assert_eq!(render(&mut raster, &terminal, false, true), no_cursor);
                assert_eq!(render(&mut raster, &terminal, true, true), visible);
                assert_eq!(
                    terminal.grid().unwrap(),
                    &grid,
                    "blink rendering cannot mutate terminal cells"
                );
                let hollow = render(&mut raster, &terminal, true, false);
                assert_ne!(hollow, no_cursor);
                assert_eq!(render(&mut raster, &terminal, true, false), hollow);
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        if x >= raster.cell_width || y >= raster.cell_height {
                            let offset = ((y * size.0 + x) * 4) as usize;
                            assert_eq!(
                                &visible[offset..offset + 4],
                                &no_cursor[offset..offset + 4]
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn runtime_sgr_styles_match_terminal_cells() {
        let fonts = [
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").as_slice(),
            include_bytes!("../../tests/fixtures/fallback-cjk.ttf").as_slice(),
            include_bytes!("../../tests/fixtures/fallback-mark.ttf").as_slice(),
        ]
        .map(|bytes| FontVec::try_from_vec(bytes.to_vec()).unwrap());
        let mut raster = Raster::new(
            fonts.into(),
            &Config {
                font_size: 20.,
                ..Config::default()
            },
        )
        .unwrap();
        let render = |raster: &mut Raster, sgr: &str, text: &str| {
            let tab = crate::TabId(1);
            let mut terminal = Terminal::new(tab, 1, 8).unwrap();
            terminal
                .feed(
                    tab,
                    format!("\x1b[?25l\x1b[38;2;200;160;120m\x1b[48;2;16;32;48m\x1b[{sgr}m{text}")
                        .as_bytes(),
                )
                .unwrap();
            let size = (8 * raster.cell_width, raster.cell_height);
            let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
            raster
                .draw(&terminal, size, &mut pixels, true, true)
                .unwrap();
            pixels
        };
        let pixel = |pixels: &[u8], scale: u32, x: u32, y: u32| {
            let offset = ((y * scale * 96 * scale + x * scale) * 4) as usize;
            u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap())
        };
        let fg = 0xffc8a078;
        let bg = 0xff102030;
        let dim = 0xff64503c;
        let mut original = None;
        for scale in [1, 2, 1] {
            raster.set_scale(scale);
            let regular = render(&mut raster, "22;23", "A");
            assert_eq!(pixel(&regular, scale, 2, 4), fg);
            assert_eq!(pixel(&regular, scale, 10, 8), bg);
            let bold = render(&mut raster, "1", "A");
            assert_eq!(
                pixel(&bold, scale, 10, 8),
                fg,
                "synthetic weight expands ink"
            );
            let italic = render(&mut raster, "3", "A");
            assert_eq!(
                pixel(&italic, scale, 2, 4),
                bg,
                "italic shifts the top right"
            );
            assert_eq!(pixel(&italic, scale, 11, 4), fg);
            let both = render(&mut raster, "1;3", "A");
            assert_ne!(both, bold);
            assert_ne!(both, italic);
            for (sgr, expected) in [
                ("22;23", &regular),
                ("1", &bold),
                ("3", &italic),
                ("1;3", &both),
            ] {
                assert_eq!(
                    &render(&mut raster, sgr, "A"),
                    expected,
                    "cache retains independent style masks"
                );
            }
            let faint = render(&mut raster, "2", "A");
            assert_eq!(pixel(&faint, scale, 3, 8), dim);
            assert_eq!(pixel(&faint, scale, 0, 0), bg);
            for sgr in ["31;2", "38;5;1;2"] {
                assert_eq!(
                    pixel(&render(&mut raster, sgr, "A"), scale, 3, 8),
                    0xff660000
                );
                assert_eq!(
                    pixel(
                        &render(&mut raster, sgr, "\x1b]4;1;#204060\x07A"),
                        scale,
                        3,
                        8
                    ),
                    0xff102030
                );
            }
            let indexed_underline = render(&mut raster, "4;58;5;1", "\x1b]4;1;#204060\x07 ");
            assert_eq!(pixel(&indexed_underline, scale, 0, 19), 0xff204060);
            let bold_dim = render(&mut raster, "1;2", "A");
            assert_eq!(pixel(&bold_dim, scale, 10, 8), dim);
            let inverse = render(&mut raster, "7", "A");
            assert_eq!(pixel(&inverse, scale, 0, 0), fg);
            assert_eq!(pixel(&inverse, scale, 3, 8), bg);
            let inverse_dim = render(&mut raster, "2;7", "A");
            assert_eq!(pixel(&inverse_dim, scale, 0, 0), dim);
            assert_eq!(pixel(&inverse_dim, scale, 3, 8), bg);
            for sgr in ["8", "1;2;3;4:3;9;8"] {
                let hidden = render(&mut raster, sgr, "A");
                assert!(
                    (0..20).all(|y| (0..12).all(|x| pixel(&hidden, scale, x, y) == bg)),
                    "hidden suppresses all ink"
                );
            }
            let strike = render(&mut raster, "9", " ");
            assert_eq!(pixel(&strike, scale, 0, 10), fg);
            assert_eq!(pixel(&strike, scale, 0, 9), bg);
            let mut underlines = Vec::new();
            for sgr in ["4", "4:2", "4:3", "4:4", "4:5"] {
                let underline = render(&mut raster, sgr, " ");
                assert!(underline != render(&mut raster, "24", " "));
                assert!(
                    !underlines.contains(&underline),
                    "each variant has distinct pixels"
                );
                underlines.push(underline);
            }
            assert_eq!(pixel(&underlines[0], scale, 0, 19), fg);
            assert_eq!(pixel(&underlines[0], scale, 0, 17), bg);
            assert_eq!(pixel(&underlines[1], scale, 0, 17), fg);
            assert_eq!(pixel(&underlines[1], scale, 0, 18), bg);
            assert_eq!(pixel(&underlines[2], scale, 2, 17), fg);
            assert_eq!(pixel(&underlines[2], scale, 2, 19), bg);
            assert_eq!(pixel(&underlines[3], scale, 0, 19), fg);
            assert_eq!(pixel(&underlines[3], scale, 1, 19), bg);
            assert_eq!(pixel(&underlines[4], scale, 3, 19), fg);
            assert_eq!(pixel(&underlines[4], scale, 4, 19), bg);
            let colored = render(&mut raster, "4;58;2;18;52;86", " ");
            assert_eq!(pixel(&colored, scale, 0, 19), 0xff123456);
            let reset_color = render(&mut raster, "4;58;2;18;52;86;59", " ");
            assert_eq!(reset_color, underlines[0]);
            assert_eq!(
                render(&mut raster, "1;21", "A"),
                regular,
                "pinned core uses SGR 21 to cancel bold"
            );
            assert_eq!(render(&mut raster, "4:1", " "), underlines[0]);
            assert_eq!(
                render(&mut raster, "4:3;4:0", " "),
                render(&mut raster, "24", " ")
            );
            let inverse_underline = render(&mut raster, "2;7;4;58;2;18;52;86", " ");
            assert_eq!(pixel(&inverse_underline, scale, 0, 19), 0xff123456);
            assert_eq!(pixel(&inverse_underline, scale, 0, 18), dim);
            let hidden_underline = render(&mut raster, "8;4;58;2;18;52;86", " ");
            assert_eq!(pixel(&hidden_underline, scale, 0, 19), bg);
            assert_eq!(
                render(&mut raster, "4:2;24;9;29;1;2;22;3;23;7;27;8;28", "A"),
                regular
            );
            // Fallback faces, combining marks, wide-cell clipping and resets use the same styles.
            let wide = render(&mut raster, "1;3;4;9", "中\u{301}\x1b[0mA");
            assert_eq!(pixel(&wide, scale, 23, 19), fg);
            assert_eq!(pixel(&wide, scale, 24, 19), 0xff181818);
            assert_eq!(pixel(&wide, scale, 26, 4), 0xffd3d7cf);
            assert_ne!(wide, render(&mut raster, "4;9", "中\u{301}\x1b[0mA"));
            assert_eq!(render(&mut raster, "22;23", "A"), regular);
            if scale == 1 {
                if let Some(first) = &original {
                    assert_eq!(&both, first);
                }
                original = Some(both);
            }
        }
        // Disabled cache renders the same styles, without retaining masks.
        let reference = render(&mut raster, "1;2;3;4:3;9", "e\u{301}中🦀");
        raster.cache = GlyphCache::new(0, 0).unwrap();
        assert_eq!(
            render(&mut raster, "1;2;3;4:3;9", "e\u{301}中🦀"),
            reference
        );
        assert!(raster.cache.is_empty());
        let font = FontVec::try_from_vec(
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
        )
        .unwrap();
        let mut tiny = Raster::new(
            vec![font],
            &Config {
                font_size: 1.,
                ..Config::default()
            },
        )
        .unwrap();
        tiny.set_scale(2);
        let tab = crate::TabId(1);
        let mut terminal = Terminal::new(tab, 2, 8).unwrap();
        terminal.feed(tab, b"\x1b[?25l\x1b[1;3;4:3;9mA").unwrap();
        let size = (8 * tiny.cell_width, 2 * tiny.cell_height);
        let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
        tiny.draw(&terminal, size, &mut pixels, true, true).unwrap();
        assert!(
            pixels[(tiny.cell_height * size.0 * 4) as usize..]
                .chunks_exact(4)
                .all(|p| u32::from_ne_bytes(p.try_into().unwrap()) == 0xff181818),
            "tiny decorations never spill into the next row"
        );
    }
    #[test]
    fn runtime_font_fallback_covers_cjk_and_combining() {
        let fixture = |bytes: &[u8]| FontVec::try_from_vec(bytes.to_vec()).unwrap();
        let fonts = vec![
            fixture(include_bytes!("../../tests/fixtures/fallback-primary.ttf")),
            fixture(include_bytes!("../../tests/fixtures/fallback-cjk.ttf")),
            fixture(include_bytes!("../../tests/fixtures/fallback-mark.ttf")),
        ];
        for (ch, expected) in [
            ('A', Some(0)),
            ('繁', Some(1)),
            ('體', Some(1)),
            ('中', Some(1)),
            ('𠮷', Some(1)),
            ('\u{301}', Some(2)),
            ('🦀', None),
        ] {
            assert_eq!(
                crate::font::select_font(ch, &fonts, |font, ch| font.glyph_id(ch).0 != 0),
                expected
            );
        }
        let config = Config {
            font_size: 20.,
            ..Config::default()
        };
        let mut raster = Raster::new(fonts, &config).unwrap();
        assert_eq!(raster.logical_cell(), (12, 20));
        assert_eq!(raster.baseline, 16.);
        let tab = crate::TabId(1);
        let draw = |raster: &mut Raster, text: &str| {
            let mut terminal = Terminal::new(tab, 2, 8).unwrap();
            terminal.feed(tab, b"\x1b[?25l").unwrap();
            terminal.feed(tab, text.as_bytes()).unwrap();
            let size = (8 * raster.cell_width, 2 * raster.cell_height);
            let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
            raster
                .draw(&terminal, size, &mut pixels, true, true)
                .unwrap();
            pixels
        };
        let mut at_scale_one = None;
        for scale in [1, 2, 1] {
            raster.set_scale(scale);
            let plain = draw(&mut raster, "A中e");
            let marked = draw(&mut raster, "A中e\u{301}");
            assert!(plain != marked, "negative-bearing mark must not disappear");
            let w = 8 * raster.cell_width;
            let ink = |pixels: &[u8], x: u32, y: u32| {
                let offset = ((y * w + x) * 4) as usize;
                u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()) != 0xff181818
            };
            assert!(ink(&plain, 3 * scale, 10 * scale));
            assert!(
                ink(&plain, 32 * scale, 10 * scale),
                "CJK reaches second cell"
            );
            assert!(
                !ink(&plain, 36 * scale, 10 * scale),
                "wide glyph does not spill"
            );
            for y in 16 * scale..20 * scale {
                assert!((0..w).all(|x| !ink(&plain, x, y)), "shared baseline");
            }
            for y in 0..2 * raster.cell_height {
                for x in 0..w {
                    if !(36 * scale..48 * scale).contains(&x) || y >= 4 * scale {
                        let o = ((y * w + x) * 4) as usize;
                        assert_eq!(&plain[o..o + 4], &marked[o..o + 4]);
                    }
                }
            }
            let both = draw(&mut raster, "e\u{301}中\u{301}");
            assert!(ink(&both, 4 * scale, scale));
            assert!(
                ink(&both, 23 * scale, scale),
                "same mark centers over a wide base"
            );
            assert!(
                !ink(&both, 15 * scale, scale),
                "narrow mask is not reused for wide base"
            );
            let later = draw(&mut raster, "E");
            assert!(
                ink(&later, 4 * scale, 8 * scale),
                "mapped blank face must not hide a later outline"
            );
            let unsupported = draw(&mut raster, "B");
            assert!(
                ink(&unsupported, scale, 8 * scale),
                "mapped but unsupported visible glyph gets tofu"
            );
            assert!(!ink(&unsupported, 4 * scale, 8 * scale));
            let missing = draw(&mut raster, "🦀A");
            assert!(
                (0..raster.cell_height)
                    .any(|y| (0..2 * raster.cell_width).any(|x| ink(&missing, x, y))),
                "missing glyph must have an explicit visible fallback"
            );
            assert_eq!(draw(&mut raster, "🦀A"), missing, "cached mask is stable");
            if scale == 1 {
                if let Some(first) = &at_scale_one {
                    assert_eq!(&marked, first);
                }
                at_scale_one = Some(marked);
            }
        }
    }
    #[test]
    fn runtime_scaled_glyphs_rebuild_cache_and_preserve_grid_geometry() {
        let font = FontVec::try_from_vec(include_bytes!("../../tests/fixtures/demo.ttf").to_vec())
            .unwrap();
        let mut raster = Raster::new(vec![font], &Config::default()).unwrap();
        let mut terminal = Terminal::new(crate::TabId(1), 2, 8).unwrap();
        terminal
            .feed(crate::TabId(1), "\x1b[?25l\x1b[41mA中e\u{301}".as_bytes())
            .unwrap();
        let cell = raster.logical_cell();
        let draw = |raster: &mut Raster| {
            let size = (8 * raster.cell_width, 2 * raster.cell_height);
            let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
            raster
                .draw(&terminal, size, &mut pixels, true, true)
                .unwrap();
            pixels
        };
        let original = draw(&mut raster);
        let cache_bytes = raster.cache.bytes_used();
        assert!(cache_bytes > 0);
        raster.set_scale(2);
        assert!(raster.cache.is_empty());
        assert_eq!(
            (raster.cell_width, raster.cell_height),
            (cell.0 * 2, cell.1 * 2)
        );
        assert_eq!(raster.logical_cell(), cell);
        assert_eq!(raster.baseline, raster.logical_baseline * 2.);
        let scaled = draw(&mut raster);
        assert_eq!(scaled.len(), original.len() * 4);
        assert_eq!(raster.cache.bytes_used(), cache_bytes * 4);
        raster.set_scale(2);
        assert_eq!(
            raster.cache.bytes_used(),
            cache_bytes * 4,
            "unchanged scale retains cache"
        );
        raster.set_scale(1);
        assert_eq!(draw(&mut raster), original);
    }
    #[test]
    fn runtime_query_palette_matches_cell_and_cursor_pixels() {
        let font = FontVec::try_from_vec(include_bytes!("../../tests/fixtures/demo.ttf").to_vec())
            .unwrap();
        let mut raster = Raster::new(vec![font], &Config::default()).unwrap();
        let mut terminal = Terminal::with_scrollback(crate::TabId(1), 2, 8, 0).unwrap();
        terminal.set_cell_pixels((raster.cell_width, raster.cell_height));
        terminal.feed(crate::TabId(1), b"\x1b]4;1;#123456\x07\x1b]12;#abcdef\x07\x1b[41m \x1b[0m\x1b[6 q\x1b]4;1;?\x07\x1b]12;?\x07").unwrap();
        let size = (8 * raster.cell_width, 2 * raster.cell_height);
        let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
        raster
            .draw(&terminal, size, &mut pixels, true, true)
            .unwrap();
        let pixel = |x: u32, y: u32| {
            let offset = ((y * size.0 + x) * 4) as usize;
            u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap())
        };
        assert_eq!(pixel(0, 0), 0xff123456);
        assert_eq!(pixel(raster.cell_width, 0), 0xffabcdef);
        assert_eq!(pixel(raster.cell_width * 2, 0), 0xff181818);
        assert_eq!(
            terminal.take_core_replies().0,
            b"\x1b]4;1;rgb:1212/3434/5656\x07\x1b]12;rgb:abab/cdcd/efef\x07"
        );
        terminal
            .feed(crate::TabId(1), b"\x1b]112\x07\x1b]12;?\x07")
            .unwrap();
        raster
            .draw(&terminal, size, &mut pixels, true, true)
            .unwrap();
        let offset = (raster.cell_width * 4) as usize;
        assert_eq!(
            u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()),
            0xffeeeeec
        );
        assert_eq!(
            terminal.take_core_replies().0,
            b"\x1b]12;rgb:eeee/eeee/ecec\x07"
        );
    }
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
            raster
                .draw(terminal, size, &mut pixels, true, true)
                .unwrap();
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
        paint_cursor(
            &mut pixels,
            (8, 8),
            (1, 1, 6, 6),
            CursorShape::Block,
            true,
            CURSOR,
        );
        assert_ne!(&pixels[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[0; 4]);
        pixels.fill(0);
        paint_cursor(
            &mut pixels,
            (8, 8),
            (1, 1, 6, 6),
            CursorShape::Block,
            false,
            CURSOR,
        );
        assert_eq!(&pixels[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[0; 4]);
        assert_ne!(&pixels[(8 + 1) * 4..(8 + 1) * 4 + 4], &[0; 4]);
        pixels.fill(0);
        paint_cursor(
            &mut pixels,
            (8, 8),
            (1, 1, 6, 6),
            CursorShape::Hidden,
            true,
            CURSOR,
        );
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
            .draw(&terminal, (128, 64), &mut pixels, true, true)
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
                .draw(&terminal, (128, 64), &mut pixels[..16], true, true)
                .is_err()
        );
    }
}

#[cfg(test)]
mod palette_tests;
