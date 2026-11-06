use super::Result;
use crate::{
    config::Config,
    font::cache::{GlyphCache, GlyphKey},
    renderer::palette::color,
    terminal::Terminal,
};
use ab_glyph::{Font, FontVec, ScaleFont, point};
use alacritty_terminal::{
    term::cell::Flags,
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
    ) -> Result<()> {
        let (width, height) = size;
        let (x, y, span) = cell;
        if ch == ' ' {
            return Ok(());
        }
        // This raster owns an immutable font set and its cache. Scale changes clear it;
        // the scalar selects the face, and span affects mark centering/tofu geometry.
        let key = GlyphKey {
            font_instance: u64::from(span),
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
                self.draw_glyph(pixels, size, (x, y, span), ch, fg)?;
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
                color(
                    Color::Named(alacritty_terminal::vte::ansi::NamedColor::Cursor),
                    colors,
                ),
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
        for row in y..y.saturating_add(h).min(size.1) {
            for col in x..x.saturating_add(w).min(size.0) {
                let offset = (row * size.0 + col) as usize * 4;
                let pixel =
                    u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()) ^ 0x00ffffff;
                pixels[offset..offset + 4].copy_from_slice(&pixel.to_ne_bytes());
            }
        }
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
    use crate::renderer::palette::CURSOR;
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
            raster.draw(&terminal, size, &mut pixels, true).unwrap();
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
            raster.draw(&terminal, size, &mut pixels, true).unwrap();
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
        raster.draw(&terminal, size, &mut pixels, true).unwrap();
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
        raster.draw(&terminal, size, &mut pixels, true).unwrap();
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
