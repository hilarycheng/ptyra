use super::{ConfigError, error};
use crate::keymap::Modifiers;
use kbvm::{Keysym, ModifierMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingAction {
    Copy,
    Paste,
    ImeToggle,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
}
const NAMES: [&str; 11] = [
    "copy",
    "paste",
    "ime_toggle",
    "new_tab",
    "close_tab",
    "next_tab",
    "previous_tab",
    "scroll_page_up",
    "scroll_page_down",
    "scroll_top",
    "scroll_bottom",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    modifiers: u32,
    keysym: u32,
}

impl Shortcut {
    fn parse(text: &str) -> Result<Self, ConfigError> {
        let invalid = || error("bindings", "invalid shortcut");
        if text.len() > 128 {
            return Err(invalid());
        }
        let mut parts = text.split('+').map(str::trim).peekable();
        let mut modifiers = 0;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                let name = part.to_ascii_lowercase();
                let symbol = Keysym::from_str_insensitive(&name).ok_or_else(invalid)?;
                return Ok(Self {
                    modifiers,
                    keysym: symbol.to_lowercase().0,
                });
            }
            let mask = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ModifierMask::CONTROL.0,
                "shift" => ModifierMask::SHIFT.0,
                "alt" => ModifierMask::ALT.0,
                "super" => ModifierMask::SUPER.0,
                _ => return Err(invalid()),
            };
            if modifiers & mask != 0 {
                return Err(invalid());
            }
            modifiers |= mask;
        }
        Err(invalid())
    }

    pub fn matches(&self, modifiers: Modifiers, keysym: u32) -> bool {
        let mask = ModifierMask::CONTROL.0
            | ModifierMask::SHIFT.0
            | ModifierMask::ALT.0
            | ModifierMask::SUPER.0;
        let canonical = Keysym(keysym).to_lowercase().0;
        modifiers.effective() & mask == self.modifiers && canonical == self.keysym
    }
}

#[derive(Debug, PartialEq)]
pub struct Bindings([Shortcut; 11]);

impl Default for Bindings {
    fn default() -> Self {
        Self(
            [
                "Ctrl+Shift+c",
                "Ctrl+Shift+v",
                "Ctrl+space",
                "Ctrl+Shift+t",
                "Ctrl+Shift+w",
                "Ctrl+Page_Down",
                "Ctrl+Page_Up",
                "Shift+Page_Up",
                "Shift+Page_Down",
                "Shift+Home",
                "Shift+End",
            ]
            .map(|text| Shortcut::parse(text).expect("built-in shortcut")),
        )
    }
}

impl Bindings {
    pub fn get(&self, action: BindingAction) -> Shortcut {
        self.0[action as usize]
    }

    pub(super) fn from_value(value: &toml::Value) -> Result<Self, ConfigError> {
        let table = value
            .as_table()
            .ok_or_else(|| error("bindings", "expected table"))?;
        let mut bindings = Self::default();
        for (name, value) in table {
            let index = NAMES
                .iter()
                .position(|known| *known == name)
                .ok_or_else(|| error("bindings", "unknown action"))?;
            bindings.0[index] = Shortcut::parse(
                value
                    .as_str()
                    .ok_or_else(|| error("bindings", "expected shortcut string"))?,
            )?;
        }
        bindings.validate()?;
        Ok(bindings)
    }

    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        for (index, shortcut) in self.0.iter().enumerate() {
            if self.0[..index].contains(shortcut) {
                return Err(error(
                    "bindings",
                    "duplicate shortcut including reserved IME toggle",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    #[test]
    fn config_detects_duplicate_shortcuts() {
        assert!(Config::from_toml(Some("[bindings]\nscroll_top=\"Alt+Home\"")).is_ok());
        assert!(Config::from_toml(Some("[bindings]\nscroll_bottom=\"Ctrl+Space\"")).is_err());
        let binding = super::Bindings::default().get(super::BindingAction::Paste);
        let mods = crate::keymap::Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0 | kbvm::ModifierMask::SHIFT.0,
            ..Default::default()
        };
        assert!(binding.matches(mods, b'V' as u32));
        assert!(!binding.matches(Default::default(), b'v' as u32));
        for source in [
            "[bindings]\nnew_tab='Shift+Control+v'",
            "[bindings]\npaste='Ctrl+Space'",
            "[bindings]\ncopy='Alt+x'\nclose_tab='alt+X'",
        ] {
            assert_eq!(
                Config::from_toml(Some(source)).unwrap_err().field,
                "bindings"
            );
        }
        assert!(
            Config::from_toml(Some(
                "[bindings]\npaste='Ctrl+Alt+p'\nime_toggle='Alt+Space'"
            ))
            .is_ok()
        );
        for source in [
            "[bindings]\npaste='Ctrl+Ctrl+v'",
            "[bindings]\npaste='Ctrl+NotAKey'",
            "[bindings]\nunknown='Alt+x'",
            "bindings=42",
        ] {
            assert!(Config::from_toml(Some(source)).is_err());
        }
    }
}
