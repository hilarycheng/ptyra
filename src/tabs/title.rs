use super::Tab;
use crate::error::{AppError, Component};
use std::io::ErrorKind;

impl<P> Tab<P> {
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Plain display text only; bound both input scanning and stored UTF-8 bytes.
    pub fn set_title(&mut self, raw: &str) -> Result<(), AppError> {
        let mut title = String::new();
        title
            .try_reserve_exact(256)
            .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
        for (_, ch) in raw.char_indices().take_while(|(offset, _)| *offset < 4096) {
            if ch.is_control()
                || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                continue;
            }
            if title.len() + ch.len_utf8() > 256 {
                break;
            }
            title.push(ch);
        }
        self.title = title;
        Ok(())
    }

    pub fn refresh_title(&mut self) -> Result<bool, AppError> {
        let Some(raw) = self.terminal.take_title() else {
            return Ok(false);
        };
        self.set_title(&raw)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        TabId,
        geometry::Scale,
        renderer::CellLayout,
        tabs::{TabRegistry, tests::FakePty},
        window::WindowSize,
    };

    #[test]
    fn tab_title_is_sanitized() {
        let mut tabs = TabRegistry::<FakePty>::default();
        tabs.insert(TabId(1), 2, 8).unwrap();
        tabs.insert(TabId(2), 2, 8).unwrap();
        let tab = tabs.get_mut(TabId(1)).unwrap();
        tab.set_title("中文\x1b[31m\n\t\u{85}\u{202e}safe\u{2066}\u{2028}")
            .unwrap();
        assert_eq!(tab.title(), "中文[31msafe");
        tab.set_title(&"中".repeat(1000)).unwrap();
        assert_eq!(tab.title().len(), 255);
        assert!(tab.title().chars().all(|c| c == '中'));
        tab.terminal_mut()
            .feed(TabId(1), "\x1b]2;標題\u{202e}\x07".as_bytes())
            .unwrap();
        assert!(tab.refresh_title().unwrap());
        assert_eq!(tab.title(), "標題");
        assert!(!tab.refresh_title().unwrap());
        let layout = CellLayout::new(
            WindowSize::new(160, 20).unwrap(),
            Scale::new(1).unwrap(),
            80.0,
            20.0,
        )
        .unwrap();
        assert_eq!(tabs.bar_items(&layout).next().unwrap().title, "標題");
        assert_eq!(tabs.get(TabId(2)).unwrap().title(), "Terminal");
    }
}
