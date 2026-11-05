use super::{TabPty, TabRegistry};
use crate::{TabId, damage::Rect, input::Point, renderer::CellLayout};

pub struct TabBarItem {
    pub tab: TabId,
    pub rect: Rect,
    pub active: bool,
}

impl<P: TabPty> TabRegistry<P> {
    /// One row at the surface top-left. Rectangles and pointer positions use buffer pixels.
    pub fn bar_items<'a>(
        &'a self,
        layout: &'a CellLayout,
    ) -> impl Iterator<Item = TabBarItem> + 'a {
        self.ids().enumerate().filter_map(|(column, tab)| {
            Some(TabBarItem {
                tab,
                rect: layout.cell_rect(0, column)?,
                active: self.active == Some(tab),
            })
        })
    }

    pub fn tab_at(&self, layout: &CellLayout, buffer_position: Point) -> Option<TabId> {
        let (_, column) = layout.hit_test(buffer_position, 1, self.entries.len())?;
        Some(self.entries.get(column)?.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::tests::FakePty;
    use crate::{geometry::Scale, input::Point, window::WindowSize};

    #[test]
    fn tab_bar_hit_test_selects_correct_tab() {
        let mut tabs = TabRegistry::<FakePty>::default();
        for id in [1, 4, 8] {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        let layout = CellLayout::new(
            WindowSize::new(250, 20).unwrap(),
            Scale::new(2).unwrap(),
            100.0,
            20.0,
        )
        .unwrap();
        let items: Vec<_> = tabs.bar_items(&layout).collect();
        assert_eq!(
            items.iter().map(|item| item.tab).collect::<Vec<_>>(),
            [TabId(1), TabId(4), TabId(8)]
        );
        assert_eq!(
            items[2].rect,
            Rect {
                x: 400,
                y: 0,
                width: 100,
                height: 40
            }
        );
        for (x, expected) in [
            (-1.0, None),
            (0.0, Some(TabId(1))),
            (199.9, Some(TabId(1))),
            (200.0, Some(TabId(4))),
            (499.9, Some(TabId(8))),
            (500.0, None),
        ] {
            assert_eq!(tabs.tab_at(&layout, Point::new(x, 10.0).unwrap()), expected);
        }
        assert!(
            tabs.tab_at(&layout, Point::new(10.0, 40.0).unwrap())
                .is_none()
        );
        let clicked = tabs
            .tab_at(&layout, Point::new(201.0, 10.0).unwrap())
            .unwrap();
        tabs.activate(clicked);
        assert_eq!(
            tabs.bar_items(&layout)
                .filter(|item| item.active)
                .map(|item| item.tab)
                .collect::<Vec<_>>(),
            [TabId(4)]
        );
        assert!(tabs.close(TabId(4)).is_some());
        assert_eq!(
            tabs.tab_at(&layout, Point::new(201.0, 10.0).unwrap()),
            Some(TabId(8))
        );
        assert!(
            TabRegistry::<FakePty>::default()
                .tab_at(&layout, Point::new(1.0, 1.0).unwrap())
                .is_none()
        );
    }
}
