//! Bounded TOML configuration; diagnostics contain metadata, never source text or values.
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub struct Config {
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
            font_family: "monospace".into(),
            font_size: 14.0,
            scrollback_lines: crate::terminal::DEFAULT_SCROLLBACK_LINES,
            clipboard_bytes: crate::clipboard::read::MAX_PASTE_BYTES,
            glyph_cache_bytes: crate::font::cache::MAX_CACHE_BYTES,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
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
