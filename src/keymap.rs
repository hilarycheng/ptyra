use crate::{
    error::{AppError, Component},
    input::{FocusTarget, KeyRoute},
};
use kbvm::{
    GroupIndex, Keycode, ModifierMask,
    lookup::LookupTable,
    xkb::{
        Context,
        diagnostic::{Diagnostic, DiagnosticHandler, DiagnosticKind, Severity},
    },
};
use std::{cell::Cell, io, sync::Arc};

pub const MAX_KEYMAP_BYTES: usize = 256 * 1024;
const MAX_KEYS: usize = 4096;
const MAX_LEVELS: usize = 256;
const MAX_SYMBOLS: usize = 16;
const MAX_EXPRESSION_OPERATORS: usize = 128;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modifiers {
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    pub group: u32,
}

impl Modifiers {
    pub fn effective(&self) -> u32 {
        self.depressed | self.latched | self.locked
    }
    pub fn shift(&self) -> bool {
        self.effective() & ModifierMask::SHIFT.0 != 0
    }
    pub fn ctrl(&self) -> bool {
        self.effective() & ModifierMask::CONTROL.0 != 0
    }
    /// XKB's conventional Alt (Mod1); retain the full masks for other bindings.
    pub fn alt(&self) -> bool {
        self.effective() & ModifierMask::ALT.0 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
    Repeated,
}

// Input payloads deliberately have no Debug implementation.
pub struct TranslatedKey {
    pub target: FocusTarget,
    pub physical_code: u32,
    pub keysyms: Vec<u32>,
    pub unshifted_keysyms: Vec<u32>,
    pub modifiers: Modifiers,
    pub num_lock: bool,
    pub state: KeyState,
    pub repeatable: bool,
    pub(crate) focus_epoch: Arc<()>,
    pub(crate) keymap_epoch: Arc<()>,
}

pub struct Keymap {
    epoch: Arc<()>,
    lookup: LookupTable,
    keys: Vec<u32>,
    modifiers: Modifiers,
    num_lock_mask: u32,
}

fn invalid() -> AppError {
    AppError::new(Component::Wayland, io::ErrorKind::InvalidData)
}

#[derive(Default)]
struct Diagnostics(Cell<bool>);
impl DiagnosticHandler for Diagnostics {
    fn filter(&self, kind: DiagnosticKind, fatal: bool) -> bool {
        if fatal || kind.severity() >= Severity::Warning {
            self.0.set(true);
        }
        false // Do not retain or print keymap text in diagnostics.
    }
    fn handle(&mut self, _: Diagnostic) {}
}

impl Keymap {
    /// Compile a self-contained XKB text-v1 map; one wire-format trailing NUL is allowed.
    /// The caller owns FD acquisition/byte limits and atomically replaces its old map on success.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.is_empty() || bytes.len() > MAX_KEYMAP_BYTES {
            return Err(invalid());
        }
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        if bytes.contains(&0) {
            return Err(invalid());
        }
        check_expression_budget(bytes)?;
        let mut builder = Context::builder();
        builder.enable_environment(false);
        builder.enable_default_includes(false);
        builder.max_includes(0);
        builder.max_include_depth(0);
        builder.max_runtime(100_000);
        let mut diagnostics = Diagnostics::default();
        let map = builder
            .build()
            .keymap_from_bytes(&mut diagnostics, None, bytes)
            .map_err(|_| invalid())?;
        if diagnostics.0.get() {
            return Err(invalid());
        }
        let mut keys = Vec::new();
        for key in map.keys() {
            if keys.len() == MAX_KEYS {
                return Err(invalid());
            }
            keys.push(key.keycode().to_x11());
            for group in key.groups().flatten() {
                if group.ty().mappings().any(|m| m.level() >= MAX_LEVELS) {
                    return Err(invalid());
                }
                for (index, level) in group.levels().enumerate() {
                    if index >= MAX_LEVELS || level.symbols().len() > MAX_SYMBOLS {
                        return Err(invalid());
                    }
                }
            }
        }
        if keys.is_empty() {
            return Err(invalid());
        }
        keys.sort_unstable();
        let num_lock_mask = map
            .virtual_modifiers()
            .find(|modifier| modifier.name() == "NumLock")
            .map_or(0, |modifier| modifier.mask().0);
        Ok(Self {
            epoch: Arc::new(()),
            lookup: map.to_builder().build_lookup_table(),
            keys,
            modifiers: Modifiers::default(),
            num_lock_mask,
        })
    }

    pub(crate) fn matches(&self, epoch: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.epoch, epoch)
    }

    pub(crate) fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// wl_keyboard.modifiers owns state. Do not infer modifiers from press/release events.
    pub fn update_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// The focus reducer must produce the route immediately before translation.
    /// Repeat scheduling and terminal byte encoding are separate tasks.
    pub fn translate(
        &self,
        route: KeyRoute,
        repeat: bool,
    ) -> Result<Option<TranslatedKey>, AppError> {
        if repeat && !route.pressed {
            return Err(invalid());
        }
        let xkb_code = route.code.checked_add(8).ok_or_else(invalid)?;
        if self.keys.binary_search(&xkb_code).is_err() {
            return Ok(None);
        }
        let lookup = self
            .lookup
            .lookup(
                GroupIndex(self.modifiers.group),
                ModifierMask(self.modifiers.effective()),
                Keycode::from_x11(xkb_code),
            )
            .with_ctrl_transform(false)
            .with_ctrl_fallback(false)
            .with_caps_transform(false);
        let keysyms = lookup.into_iter().map(|symbol| symbol.keysym().0).collect();
        let unshifted_keysyms = self
            .lookup
            .lookup(
                GroupIndex(self.modifiers.group),
                ModifierMask(
                    self.modifiers.effective() & !(ModifierMask::SHIFT.0 | ModifierMask::LOCK.0),
                ),
                Keycode::from_x11(xkb_code),
            )
            .with_ctrl_transform(false)
            .with_ctrl_fallback(false)
            .with_caps_transform(false)
            .into_iter()
            .map(|symbol| symbol.keysym().to_lowercase().0)
            .collect();
        Ok(Some(TranslatedKey {
            target: route.target,
            physical_code: route.code,
            keysyms,
            unshifted_keysyms,
            modifiers: self.modifiers,
            num_lock: self.modifiers.effective() & self.num_lock_mask != 0,
            state: if repeat {
                KeyState::Repeated
            } else if route.pressed {
                KeyState::Pressed
            } else {
                KeyState::Released
            },
            repeatable: lookup.repeats(),
            focus_epoch: route.focus_epoch,
            keymap_epoch: self.epoch.clone(),
        }))
    }
}

/// KBVM limits nesting, but a long left-associative expression also creates a deep AST.
/// Match its string/key-name/comment boundaries and cap operators per statement before parsing.
fn check_expression_budget(bytes: &[u8]) -> Result<(), AppError> {
    let mut position = 0;
    let mut operators = 0;
    while let Some(&byte) = bytes.get(position) {
        position += 1;
        match byte {
            b'#' => {
                while bytes.get(position).is_some_and(|b| *b != b'\n') {
                    position += 1;
                }
            }
            b'/' if bytes.get(position) == Some(&b'/') => {
                while bytes.get(position).is_some_and(|b| *b != b'\n') {
                    position += 1;
                }
            }
            b'"' => loop {
                match bytes.get(position) {
                    None => return Err(invalid()),
                    Some(b'"') => {
                        position += 1;
                        break;
                    }
                    Some(b'\\') => position += 2,
                    Some(_) => position += 1,
                }
            },
            b'<' => loop {
                match bytes.get(position) {
                    Some(b'>') => {
                        position += 1;
                        break;
                    }
                    Some(b'!'..=b'~') => position += 1,
                    _ => return Err(invalid()),
                }
            },
            b';' => operators = 0,
            b'+' | b'-' | b'*' | b'/' | b'!' | b'~' => {
                operators += 1;
                if operators > MAX_EXPRESSION_OPERATORS {
                    return Err(invalid());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
