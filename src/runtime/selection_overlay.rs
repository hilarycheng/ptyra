//! Selection presentation consumes the owned Tab range without mutating terminal cells.
use super::Result;
use crate::{
    geometry::Scale,
    renderer::{
        CellLayout,
        overlay::{OverlayCommand, overlay_commands},
    },
    tabs::Tab,
    window::WindowSize,
};

#[cfg(test)]
pub(super) fn draw<P>(
    tab: &Tab<P>,
    size: (u32, u32),
    cell: (u32, u32),
    pixels: &mut [u8],
) -> Result<()> {
    draw_rows(tab, size, cell, pixels, 0..size.1)
}

pub(super) fn bounds<P>(tab: &Tab<P>, cell_height: u32) -> Option<super::damage::Band> {
    let selection = tab.selection_range()?;
    let snapshot = tab.terminal().snapshot()?;
    let offset = snapshot.content.display_offset as i64;
    let start = (offset + i64::from(selection.start.line.0)).clamp(0, snapshot.rows as i64);
    let end = (offset + i64::from(selection.end.line.0) + 1).clamp(0, snapshot.rows as i64);
    super::damage::Band::new(start as u32 * cell_height, end as u32 * cell_height)
}

pub(super) fn draw_rows<P>(
    tab: &Tab<P>,
    size: (u32, u32),
    cell: (u32, u32),
    pixels: &mut [u8],
    rows: std::ops::Range<u32>,
) -> Result<()> {
    if u64::from(size.0) * u64::from(size.1) > super::MAX_PIXELS as u64
        || pixels.len() != size.0 as usize * size.1 as usize * 4
    {
        return Err("invalid selection buffer");
    }
    let layout = CellLayout::new(
        WindowSize::new(size.0, size.1).map_err(|_| "invalid selection size")?,
        Scale::new(1).unwrap(),
        f64::from(cell.0),
        f64::from(cell.1),
    )
    .map_err(|_| "invalid selection layout")?;
    let mut snapshot = tab.terminal().snapshot().ok_or("terminal unavailable")?;
    snapshot.content.selection = tab.selection_range();
    for command in
        overlay_commands(snapshot, &layout, None).map_err(|_| "invalid selection overlay")?
    {
        let OverlayCommand::Selection { rect } = command else {
            continue;
        };
        // Invert RGB only so glyph contrast and alpha are preserved in this independent layer.
        for y in
            (rect.y as u32).max(rows.start)..(rect.y as u32 + rect.height).min(size.1).min(rows.end)
        {
            for x in rect.x as u32..(rect.x as u32 + rect.width).min(size.0) {
                let offset = (y * size.0 + x) as usize * 4;
                let pixel =
                    u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()) ^ 0x00ffffff;
                pixels[offset..offset + 4].copy_from_slice(&pixel.to_ne_bytes());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn runtime_selection_overlay_matches_range() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 2, 8).unwrap();
        let tab = tabs.get_mut(TabId(1)).unwrap();
        tab.terminal_mut()
            .feed(TabId(1), "a中e\u{301}".as_bytes())
            .unwrap();
        let cursor = tab.terminal().grid().unwrap().cursor.point;
        let base = vec![0x55; 80 * 20 * 4];
        let mut pixels = base.clone();
        draw(tab, (80, 20), (10, 10), &mut pixels).unwrap();
        assert_eq!(pixels, base);
        let point = Point::new(Line(0), Column(2));
        tab.select(Screen::Main, SelectionAction::Press, point);
        tab.select(Screen::Main, SelectionAction::Release, point);
        draw(tab, (80, 20), (10, 10), &mut pixels).unwrap();
        for row in 0..20 {
            for col in 0..80 {
                let offset = (row * 80 + col) * 4;
                let changed = row < 10 && (10..30).contains(&col);
                assert_eq!(pixels[offset] != base[offset], changed);
                assert_eq!(pixels[offset + 3], base[offset + 3]);
            }
        }
        assert_eq!(tab.terminal().grid().unwrap().cursor.point, cursor);
        assert_eq!(
            crate::selection::copy_text(
                tab.terminal().grid().unwrap(),
                tab.selection_range().unwrap()
            )
            .unwrap(),
            "中"
        );
        tab.select(Screen::Main, SelectionAction::Press, point);
        tab.cancel_selection_drag(Screen::Main);
        pixels.copy_from_slice(&base);
        draw(tab, (80, 20), (10, 10), &mut pixels).unwrap();
        assert_eq!(pixels, base);
        assert!(draw(tab, (80, 20), (10, 10), &mut pixels[..4]).is_err());
    }
}
