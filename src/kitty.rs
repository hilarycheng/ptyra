//! Kitty keyboard protocol primitives; capabilities are enabled only by a complete input pipeline.
use crate::error::{AppError, Component};
pub(crate) mod protocol;
pub use alacritty_terminal::vte::ansi::KeyboardModes as KeyboardFlags;

/// Parse a decimal flag value, rejecting unknown bits and non-decimal input.
pub fn parse_flags(value: &str) -> Result<KeyboardFlags, AppError> {
    let invalid = || AppError::new(Component::Terminal, std::io::ErrorKind::InvalidData);
    if value.is_empty() || value.len() > 3 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    KeyboardFlags::from_bits(value.parse().map_err(|_| invalid())?).ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kitty_flags_roundtrip() {
        for bits in 0..=31 {
            let flags = KeyboardFlags::from_bits(bits).unwrap();
            assert_eq!(parse_flags(&flags.bits().to_string()).unwrap(), flags);
        }
        for invalid in ["", "-1", "+1", " 1", "32", "256", "999999", "一"] {
            assert!(parse_flags(invalid).is_err());
        }
        assert_eq!(KeyboardFlags::DISAMBIGUATE_ESC_CODES.bits(), 1);
        assert_eq!(KeyboardFlags::REPORT_EVENT_TYPES.bits(), 2);
        assert_eq!(KeyboardFlags::REPORT_ALTERNATE_KEYS.bits(), 4);
        assert_eq!(KeyboardFlags::REPORT_ALL_KEYS_AS_ESC.bits(), 8);
        assert_eq!(KeyboardFlags::REPORT_ASSOCIATED_TEXT.bits(), 16);
    }
}
