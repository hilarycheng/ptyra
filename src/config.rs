//! Bounded TOML configuration; diagnostics contain metadata, never source text or values.
pub mod bindings;
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub struct Config {
    pub bindings: bindings::Bindings,
    pub font_family: String,
    pub font_size: f64,
    pub scrollback_lines: usize,
    pub clipboard_bytes: usize,
    pub glyph_cache_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub field: &'static str,
    pub reason: &'static str,
    pub offset: Option<usize>,
}

fn error(field: &'static str, reason: &'static str) -> ConfigError {
    ConfigError {
        field,
        reason,
        offset: None,
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bindings: bindings::Bindings::default(),
            font_family: "monospace".into(),
            font_size: 14.0,
            scrollback_lines: crate::terminal::DEFAULT_SCROLLBACK_LINES,
            clipboard_bytes: crate::clipboard::read::MAX_PASTE_BYTES,
            glyph_cache_bytes: crate::font::cache::MAX_CACHE_BYTES,
        }
    }
}

impl Config {
    pub fn glyph_cache(&self) -> Result<crate::font::cache::GlyphCache, crate::error::AppError> {
        crate::font::cache::GlyphCache::new(
            self.glyph_cache_bytes,
            crate::font::cache::MAX_CACHE_ENTRIES,
        )
    }

    pub fn clipboard_receiver(
        &self,
    ) -> Result<crate::clipboard::read::ClipboardReceive, crate::error::AppError> {
        crate::clipboard::read::ClipboardReceive::with_limit(self.clipboard_bytes)
    }

    pub fn tab_registry<P: crate::tabs::TabPty>(
        &self,
    ) -> Result<crate::tabs::TabRegistry<P>, crate::error::AppError> {
        crate::tabs::TabRegistry::with_scrollback(self.scrollback_lines)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.bindings.validate()?;
        if self.scrollback_lines > crate::terminal::MAX_SCROLLBACK_LINES {
            return Err(error("scrollback_lines", "expected 0..=10000 lines"));
        }
        if self.clipboard_bytes == 0
            || self.clipboard_bytes > crate::clipboard::read::MAX_PASTE_BYTES
        {
            return Err(error("clipboard_bytes", "expected 1..=16777216 bytes"));
        }
        if self.glyph_cache_bytes > crate::font::cache::MAX_CACHE_BYTES {
            return Err(error("glyph_cache_bytes", "expected 0..=16777216 bytes"));
        }
        if !self.font_size.is_finite() || !(1.0..=256.0).contains(&self.font_size) {
            return Err(error(
                "font_size",
                "expected finite logical pixel size in 1..=256",
            ));
        }
        if self.font_family.trim().is_empty()
            || self.font_family.len() > 256
            || self.font_family.chars().any(char::is_control)
        {
            return Err(error(
                "font_family",
                "expected nonempty family, at most 256 bytes, without controls",
            ));
        }
        Ok(())
    }

    pub fn from_toml(source: Option<&str>) -> Result<Self, ConfigError> {
        let source = source.unwrap_or_default();
        if source.len() > MAX_CONFIG_BYTES {
            return Err(error("config", "file exceeds 64 KiB"));
        }
        let table: toml::Table = source.parse().map_err(|e: toml::de::Error| ConfigError {
            field: "config",
            reason: "invalid TOML",
            offset: e.span().map(|span| span.start),
        })?;
        let mut config = Self::default();
        for (key, value) in table {
            match key.as_str() {
                "bindings" => config.bindings = bindings::Bindings::from_value(&value)?,
                "font_family" => {
                    config.font_family = value
                        .as_str()
                        .ok_or_else(|| error("font_family", "expected string"))?
                        .to_owned()
                }
                "font_size" => {
                    config.font_size = value
                        .as_float()
                        .or_else(|| value.as_integer().map(|v| v as f64))
                        .ok_or_else(|| error("font_size", "expected number"))?
                }
                "scrollback_lines" => {
                    config.scrollback_lines = integer(&value, "scrollback_lines")?
                }
                "clipboard_bytes" => config.clipboard_bytes = integer(&value, "clipboard_bytes")?,
                "glyph_cache_bytes" => {
                    config.glyph_cache_bytes = integer(&value, "glyph_cache_bytes")?
                }
                _ => return Err(error("config", "unknown field")),
            }
        }
        config.validate()?;
        Ok(config)
    }
}

fn integer(value: &toml::Value, field: &'static str) -> Result<usize, ConfigError> {
    value
        .as_integer()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| error(field, "expected nonnegative integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_limits_are_bounded() {
        for source in [
            "scrollback_lines=10001",
            "clipboard_bytes=0",
            "clipboard_bytes=16777217",
            "glyph_cache_bytes=16777217",
            "scrollback_lines=-1",
            "clipboard_bytes=9223372036854775807",
        ] {
            assert!(Config::from_toml(Some(source)).is_err(), "{source}");
        }
        let config = Config::from_toml(Some(
            "scrollback_lines=0\nclipboard_bytes=4\nglyph_cache_bytes=0",
        ))
        .unwrap();
        let cache = config.glyph_cache().unwrap();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes_used(), 0);
        let mut receiver = config.clipboard_receiver().unwrap();
        let mut input = std::io::Cursor::new(b"12345");
        assert!(receiver.read_ready(&mut input).is_err());
        struct FakePty;
        impl crate::tabs::TabPty for FakePty {
            fn tab(&self) -> crate::TabId {
                crate::TabId(1)
            }
        }
        let mut tabs = config.tab_registry::<FakePty>().unwrap();
        tabs.insert(crate::TabId(1), 2, 8).unwrap();
        tabs.get_mut(crate::TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(crate::TabId(1), b"a\r\nb\r\nc\r\nd")
            .unwrap();
        use alacritty_terminal::grid::Dimensions;
        assert_eq!(
            tabs.get(crate::TabId(1))
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .history_size(),
            0
        );
        let maximum = Config::from_toml(Some(
            "scrollback_lines=10000\nclipboard_bytes=16777216\nglyph_cache_bytes=16777216",
        ))
        .unwrap();
        assert!(maximum.validate().is_ok());
    }

    #[test]
    fn config_rejects_invalid_font_size() {
        for size in ["0", "-1", "nan", "inf", "-inf", "1e300", "257", "'large'"] {
            let err = Config::from_toml(Some(&format!("font_size = {size}"))).unwrap_err();
            assert_eq!(err.field, "font_size");
        }
        for size in ["1", "14.5", "256"] {
            assert!(Config::from_toml(Some(&format!("font_size = {size}"))).is_ok());
        }
        for family in ["''", "'   '", "\"bad\\nname\"", "123"] {
            assert_eq!(
                Config::from_toml(Some(&format!("font_family = {family}")))
                    .unwrap_err()
                    .field,
                "font_family"
            );
        }
        let config = Config {
            font_family: "字".repeat(100),
            ..Config::default()
        };
        assert!(config.validate().is_err());
        assert_eq!(
            Config::from_toml(Some("font_family = '思源等寬'\nfont_size=18.5"))
                .unwrap()
                .font_family,
            "思源等寬"
        );
    }

    #[test]
    fn config_defaults_are_valid() {
        let defaults = Config::default();
        assert_eq!(Config::from_toml(None).unwrap(), defaults);
        assert_eq!(
            Config::from_toml(Some("# no overrides\n")).unwrap(),
            defaults
        );
        assert_eq!(defaults.font_family, "monospace");
        assert_eq!(defaults.font_size, 14.0);
        assert_eq!(
            defaults.scrollback_lines,
            crate::terminal::DEFAULT_SCROLLBACK_LINES
        );
        assert!(crate::font::cache::GlyphCache::new(defaults.glyph_cache_bytes, 4096).is_ok());
        assert!(
            crate::clipboard::read::ClipboardReceive::with_limit(defaults.clipboard_bytes).is_ok()
        );
        let custom = Config::from_toml(Some(
            "font_family = 'Example Mono'\nfont_size = 16\nscrollback_lines = 100",
        ))
        .unwrap();
        assert_eq!(custom.font_family, "Example Mono");
        assert_eq!(custom.font_size, 16.0);
        assert_eq!(custom.scrollback_lines, 100);
        assert_eq!(custom.clipboard_bytes, defaults.clipboard_bytes);
    }
}
