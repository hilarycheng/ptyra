//! Shared, bounded tab-strip geometry. Paging always includes the active Tab.
use super::{Result, raster::Raster};
use crate::{
    TabId,
    tabs::{TabPty, TabRegistry},
    terminal::Terminal,
};

pub(super) struct Item {
    pub tab: TabId,
    pub column: usize,
    pub columns: usize,
}
pub(super) struct Layout {
    pub height: u32,
    pub content: (u32, u32),
    pub columns: u16,
    pub items: Vec<Item>,
}
impl Layout {
    pub fn new<P: TabPty>(tabs: &TabRegistry<P>, size: (u32, u32), cell: (u32, u32)) -> Self {
        let height = (cell.1 + 8).min(size.1.saturating_sub(1));
        let columns = (size.0 / cell.0.max(1)).clamp(2, 1024) as u16;
        let ids: Vec<_> = tabs.ids().collect();
        let count = (usize::from(columns) / 12).max(1).min(ids.len());
        let mut items = Vec::new();
        let active = ids
            .iter()
            .position(|id| Some(*id) == tabs.active())
            .unwrap_or(0);
        if let Some(page) = active.checked_div(count) {
            let start = page * count;
            let visible = count.min(ids.len() - start);
            let slot = usize::from(columns) / visible;
            for (index, &tab) in ids[start..start + visible].iter().enumerate() {
                let column = index * slot;
                let width = if index + 1 == visible {
                    usize::from(columns) - column
                } else {
                    slot
                };
                items.push(Item {
                    tab,
                    column,
                    columns: width,
                });
            }
        }
        Self {
            height,
            content: (size.0, size.1 - height),
            columns,
            items,
        }
    }
    pub fn draw<P: TabPty>(
        &self,
        tabs: &TabRegistry<P>,
        raster: &mut Raster,
        size: (u32, u32),
        pixels: &mut [u8],
    ) -> Result<()> {
        let mut strip = Terminal::with_scrollback(TabId(0), 1, self.columns, 0)
            .map_err(|_| "tab strip allocation failed")?;
        let mut bytes = String::from("\x1b[?25l");
        for item in &self.items {
            let tab = tabs.get(item.tab).ok_or("tab strip target unavailable")?;
            let color = if tabs.active() == Some(item.tab) {
                "\x1b[37;44m"
            } else {
                "\x1b[37;40m"
            };
            bytes.push_str(&format!(
                "\x1b[1;{}H{color}{}\x1b[1;{}H",
                item.column + 1,
                " ".repeat(item.columns),
                item.column + 1
            ));
            let label = format!(" {} {}", item.tab.0, tab.title());
            let mut used = 0;
            for ch in label.chars().filter(|ch| !ch.is_control()) {
                let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if used + width > item.columns {
                    break;
                }
                bytes.push(ch);
                used += width;
            }
        }
        for chunk in bytes.as_bytes().chunks(crate::terminal::MAX_FEED_BYTES) {
            strip
                .feed(TabId(0), chunk)
                .map_err(|_| "tab strip text rejected")?;
        }
        raster.draw(&strip, (size.0, self.height), pixels, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(TabId);
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    #[test]
    fn runtime_tab_bar_layout_reserves_content_area() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        for id in 1..=20 {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        for id in 1..=20 {
            tabs.activate(TabId(id));
            let layout = Layout::new(&tabs, (240, 200), (8, 16));
            assert_eq!(layout.height, 24);
            assert_eq!(layout.content, (240, 176));
            assert!(layout.items.len() <= 2);
            assert!(layout.items.iter().any(|item| item.tab == TabId(id)));
            assert_eq!(
                layout.items.iter().map(|item| item.columns).sum::<usize>(),
                30
            );
        }
        let narrow = Layout::new(&tabs, (1, 1), (8, 16));
        assert_eq!(narrow.height, 0);
        assert_eq!(narrow.items.len(), 1);
    }
}
