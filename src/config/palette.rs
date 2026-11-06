//! Basic RGB palette. TOML accepts exact #RRGGBB strings, never implicit coercion.
use super::{ConfigError, error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub foreground: u32,
    pub background: u32,
    pub cursor: u32,
    pub ansi: [u32; 16],
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            foreground: 0xd3d7cf,
            background: 0x181818,
            cursor: 0xeeeeec,
            ansi: [
                0x181818, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf,
                0x555753, 0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
            ],
        }
    }
}
const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];
impl Palette {
    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        if self
            .ansi
            .iter()
            .copied()
            .chain([self.foreground, self.background, self.cursor])
            .any(|color| color > 0xffffff)
        {
            return Err(error("colors", "expected RGB values in 0..=0xffffff"));
        }
        Ok(())
    }
    pub(super) fn from_value(value: &toml::Value) -> Result<Self, ConfigError> {
        let table = value
            .as_table()
            .ok_or_else(|| error("colors", "expected table"))?;
        let mut palette = Self::default();
        for (name, value) in table {
            match name.as_str() {
                "foreground" => palette.foreground = rgb(value, "colors.foreground")?,
                "background" => palette.background = rgb(value, "colors.background")?,
                "cursor" => palette.cursor = rgb(value, "colors.cursor")?,
                "ansi" => {
                    let table = value
                        .as_table()
                        .ok_or_else(|| error("colors.ansi", "expected named color table"))?;
                    for (name, value) in table {
                        let index = ANSI_NAMES
                            .iter()
                            .position(|&known| name == known)
                            .ok_or_else(|| error("colors.ansi", "unknown ANSI color name"))?;
                        palette.ansi[index] = rgb(value, "colors.ansi")?;
                    }
                }
                _ => return Err(error("colors", "unknown color field")),
            }
        }
        palette.validate()?;
        Ok(palette)
    }
}
fn rgb(value: &toml::Value, field: &'static str) -> Result<u32, ConfigError> {
    let invalid = || error(field, "expected '#RRGGBB' RGB color");
    let text = value.as_str().ok_or_else(invalid)?;
    let bytes = text.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid());
    }
    u32::from_str_radix(&text[1..], 16).map_err(|_| invalid())
}
