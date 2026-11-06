use super::{Result, Runtime};
use crate::{
    TabId, WindowId,
    clock::Clock,
    ime::key::KeyOwner,
    input::KeyboardFocus,
    keymap::{Keymap, Modifiers, TranslatedKey},
    kitty::{KeyboardFlags, encode::KeyEncoder},
    repeat::RepeatScheduler,
};
use std::{fs::File, os::unix::fs::FileExt};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    protocol::{wl_keyboard, wl_seat},
};

#[derive(Default)]
pub(super) struct KeyboardInput {
    pub focus: KeyboardFocus,
    pub map: Option<Keymap>,
    pub repeat: RepeatScheduler,
    pub bindings: crate::config::bindings::Bindings,
    pub action: Option<crate::config::bindings::BindingAction>,
    pub(super) consumed: std::collections::HashSet<u32>,
}
impl KeyboardInput {
    pub(super) fn retarget(&mut self, tab: TabId) {
        self.repeat.cancel();
        if let Some(target) = self.focus.target() {
            self.focus.enter(target.window, tab);
        }
    }
    pub fn clear_focus(&mut self) {
        self.focus.clear();
        self.repeat.cancel();
        self.consumed.clear();
        self.action = None;
    }
    pub fn key(
        &mut self,
        code: u32,
        pressed: bool,
        application_cursor: bool,
        clock: &impl Clock,
    ) -> Result<Option<Vec<u8>>> {
        let Some(route) = self.focus.route_key(code, pressed) else {
            return Ok(None);
        };
        let Some(map) = &self.map else {
            return Ok(None);
        };
        let Some(key) = map
            .translate(route, false)
            .map_err(|_| "key translation failed")?
        else {
            return Ok(None);
        };
        self.encode(key, application_cursor, clock)
    }
    pub fn tick(
        &mut self,
        application_cursor: bool,
        clock: &impl Clock,
    ) -> Result<Option<Vec<u8>>> {
        let Some(map) = &self.map else {
            return Ok(None);
        };
        let Some(key) = self
            .repeat
            .poll(clock, &self.focus, map)
            .map_err(|_| "key repeat failed")?
        else {
            return Ok(None);
        };
        self.encode(key, application_cursor, clock)
    }
    fn encode(
        &mut self,
        key: TranslatedKey,
        application_cursor: bool,
        clock: &impl Clock,
    ) -> Result<Option<Vec<u8>>> {
        use crate::config::bindings::BindingAction;
        use crate::keymap::KeyState;
        if self.consumed.contains(&key.physical_code) {
            if key.state == KeyState::Released {
                self.consumed.remove(&key.physical_code);
            }
            return Ok(None);
        }
        if key.state == KeyState::Pressed {
            let action = [
                BindingAction::Copy,
                BindingAction::Paste,
                BindingAction::NewTab,
                BindingAction::CloseTab,
                BindingAction::NextTab,
                BindingAction::PreviousTab,
            ]
            .into_iter()
            .find(|action| {
                key.keysyms
                    .iter()
                    .any(|symbol| self.bindings.get(*action).matches(key.modifiers, *symbol))
            });
            if let Some(action) = action {
                self.repeat.cancel();
                self.consumed.insert(key.physical_code);
                self.action = Some(action);
                return Ok(None);
            }
        }
        let encoder = KeyEncoder {
            flags: KeyboardFlags::empty(),
            application_cursor,
        };
        let map = self.map.as_ref().ok_or("keymap unavailable")?;
        match encoder.route(
            key,
            KeyOwner::Terminal,
            &self.focus,
            map,
            &mut self.repeat,
            clock,
        ) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Ok(None),
            Err(_) => Err("key encoding failed"),
        }
    }
}

fn read_keymap(file: &File, size: usize) -> Result<Keymap> {
    if size == 0 || size > crate::keymap::MAX_KEYMAP_BYTES {
        return Err("unsupported Wayland keymap");
    }
    let mut bytes = vec![0; size];
    // SCM_RIGHTS descriptors can share an offset across clients; never advance it.
    file.read_exact_at(&mut bytes, 0)
        .map_err(|_| "keymap FD read failed")?;
    Keymap::from_bytes(&bytes).map_err(|_| "invalid Wayland keymap")
}

impl Runtime {
    pub(super) fn close_tab(&mut self, id: TabId) {
        let previous = self.tabs.active();
        if !super::sessions::close_tab(&mut self.tabs, id) {
            return;
        }
        self.ready_pty.retain(|ready| *ready != id);
        if let Some(active) = self.tabs.active() {
            if previous != Some(active) {
                self.input.retarget(active);
            }
        } else {
            self.input.clear_focus();
            self.running = false;
        }
        self.paint.dirty = true;
    }
    pub(super) fn activate_tab(&mut self, id: TabId) {
        if self.tabs.activate(id) {
            self.input.retarget(id);
            self.paint.dirty = true;
        }
    }
    fn dispatch_action(&mut self, action: crate::config::bindings::BindingAction) {
        use crate::config::bindings::BindingAction;
        match action {
            BindingAction::NewTab => {
                let size = self
                    .tabs
                    .active()
                    .and_then(|id| self.tabs.get(id))
                    .and_then(|tab| tab.pty())
                    .map(|pty| pty.current_size());
                if let Some(size) = size {
                    match self.new_tab(size) {
                        Ok(id) => self.activate_tab(id),
                        Err(error) => eprintln!("Ptyra: {error}"),
                    }
                }
            }
            BindingAction::CloseTab => {
                if let Some(id) = self.tabs.active() {
                    self.close_tab(id);
                }
            }
            BindingAction::NextTab | BindingAction::PreviousTab => {
                let ids: Vec<_> = self.tabs.ids().collect();
                if let Some(index) = ids.iter().position(|id| Some(*id) == self.tabs.active()) {
                    let next = if action == BindingAction::NextTab {
                        (index + 1) % ids.len()
                    } else {
                        (index + ids.len() - 1) % ids.len()
                    };
                    self.activate_tab(ids[next]);
                }
            }
            _ => eprintln!("Ptyra action unavailable: {action:?}"),
        }
    }
    pub(super) fn remove_keyboard(&mut self) {
        if let Some(keyboard) = self.keyboard.take() {
            keyboard.release();
        }
        self.input.clear_focus();
        self.input.map = None;
        self.paint.dirty = true;
    }
    fn keyboard_event(&mut self, event: wl_keyboard::Event) -> Result<()> {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                self.input.repeat.cancel();
                self.input.map = None;
                if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)
                    || size == 0
                    || size as usize > crate::keymap::MAX_KEYMAP_BYTES
                {
                    return Err("unsupported Wayland keymap");
                }
                self.input.map = Some(read_keymap(&File::from(fd), size as usize)?);
            }
            wl_keyboard::Event::Enter { surface, .. }
                if self.surface.as_ref() == Some(&surface) =>
            {
                self.input.clear_focus();
                self.paint.dirty = true;
                if let Some(id) = self.tabs.active() {
                    self.input.focus.enter(WindowId(1), id);
                }
            }
            wl_keyboard::Event::Leave { .. } => {
                self.input.clear_focus();
                self.paint.dirty = true;
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(map) = &mut self.input.map {
                    map.update_modifiers(Modifiers {
                        depressed: mods_depressed,
                        latched: mods_latched,
                        locked: mods_locked,
                        group,
                    });
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => self
                .input
                .repeat
                .configure(rate, delay)
                .map_err(|_| "invalid keyboard repeat settings")?,
            wl_keyboard::Event::Key { key, state, .. } => {
                let pressed = match state {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => true,
                    WEnum::Value(wl_keyboard::KeyState::Released) => false,
                    _ => return Ok(()),
                };
                if let Some(trace) = &mut self.input_trace {
                    trace.add(usize::from(!pressed), 1);
                }
                let bytes = self
                    .input
                    .key(key, pressed, self.application_cursor(), &self.clock)?;
                self.enqueue_input(bytes)?;
                if let Some(action) = self.input.action.take() {
                    self.dispatch_action(action);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for Runtime {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if state.seat.as_ref() != Some(seat) {
            return;
        }
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                if state.keyboard.is_none() {
                    state.keyboard = Some(seat.get_keyboard(qh, ()));
                }
            } else {
                state.remove_keyboard();
            }
        }
    }
}
impl Dispatch<wl_keyboard::WlKeyboard, ()> for Runtime {
    fn event(
        state: &mut Self,
        keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.keyboard.as_ref() != Some(keyboard) {
            return;
        }
        if let Err(error) = state.keyboard_event(event) {
            state.fail(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        clock::FakeClock,
        keymap::{Keymap, Modifiers},
    };
    #[test]
    fn runtime_tab_switch_retargets_keyboard() {
        let clock = FakeClock::default();
        let mut input = KeyboardInput {
            map: Some(
                Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap(),
            ),
            ..Default::default()
        };
        input.focus.enter(WindowId(1), TabId(1));
        input.repeat.configure(25, 100).unwrap();
        input.key(30, true, false, &clock).unwrap();
        let old = input
            .map
            .as_ref()
            .unwrap()
            .translate(input.focus.route_key(30, true).unwrap(), false)
            .unwrap()
            .unwrap();
        input.retarget(TabId(2));
        assert_eq!(input.focus.target().unwrap().tab, TabId(2));
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        assert!(input.tick(false, &clock).unwrap().is_none());
        assert!(input.encode(old, false, &clock).unwrap().is_none());
        assert_eq!(input.key(30, true, false, &clock).unwrap().unwrap(), b"a");
        input.clear_focus();
        input.retarget(TabId(1));
        assert!(input.focus.target().is_none());
    }
    #[test]
    fn runtime_bindings_route_actions_once() {
        use crate::config::{Config, bindings::BindingAction};
        let clock = FakeClock::default();
        let mut input = KeyboardInput {
            bindings: Config::from_toml(Some("[bindings]\nnew_tab='Ctrl+Shift+a'"))
                .unwrap()
                .bindings,
            map: Some(
                Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap(),
            ),
            ..Default::default()
        };
        input.focus.enter(WindowId(1), TabId(1));
        input.repeat.configure(25, 200).unwrap();
        input.map.as_mut().unwrap().update_modifiers(Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0 | kbvm::ModifierMask::SHIFT.0,
            ..Default::default()
        });
        assert!(input.key(30, true, false, &clock).unwrap().is_none());
        assert_eq!(input.action.take(), Some(BindingAction::NewTab));
        assert!(input.key(30, true, false, &clock).unwrap().is_none());
        assert!(input.action.take().is_none());
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        assert!(input.tick(false, &clock).unwrap().is_none());
        input
            .map
            .as_mut()
            .unwrap()
            .update_modifiers(Modifiers::default());
        assert!(input.key(30, false, false, &clock).unwrap().is_none());
        assert!(input.action.take().is_none());
        assert_eq!(input.key(30, true, false, &clock).unwrap().unwrap(), b"a");
        input.clear_focus();
        assert!(input.action.is_none());
        assert!(input.consumed.is_empty());
    }
    #[test]
    fn runtime_keymap_reads_shared_fd_from_zero() {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = File::from(
            nix::sys::memfd::memfd_create(c"keymap-test", nix::sys::memfd::MFdFlags::MFD_CLOEXEC)
                .unwrap(),
        );
        let data = include_bytes!("../../tests/fixtures/keyboard.xkb");
        file.write_all(data).unwrap();
        let duplicate = file.try_clone().unwrap();
        assert!(read_keymap(&file, data.len()).is_ok());
        assert!(read_keymap(&duplicate, data.len()).is_ok());
        assert_eq!(file.stream_position().unwrap(), data.len() as u64);
        file.seek(SeekFrom::Start(7)).unwrap();
        assert!(read_keymap(&duplicate, data.len()).is_ok());
        assert_eq!(file.stream_position().unwrap(), 7);
        assert!(read_keymap(&file, data.len() + 1).is_err());
    }
    #[test]
    fn runtime_tap_emits_once_and_cancels_repeat() {
        let mut input = KeyboardInput::default();
        let clock = FakeClock::default();
        input.map =
            Some(Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap());
        input.focus.enter(WindowId(1), TabId(1));
        input.repeat.configure(25, 600).unwrap();
        let mut bytes = Vec::new();
        for _ in 0..2 {
            bytes.extend(input.key(30, true, false, &clock).unwrap().unwrap());
            clock.advance(std::time::Duration::from_millis(50)).unwrap();
            assert!(input.key(30, false, false, &clock).unwrap().is_none());
            clock
                .advance(std::time::Duration::from_millis(700))
                .unwrap();
            assert!(input.tick(false, &clock).unwrap().is_none());
        }
        assert_eq!(bytes, b"aa", "two real taps produce exactly two bytes");
    }
    #[test]
    fn runtime_keyboard_routes_to_shell() {
        let mut input = KeyboardInput::default();
        let clock = FakeClock::default();
        input.map =
            Some(Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap());
        assert!(input.key(30, true, false, &clock).unwrap().is_none());
        input.focus.enter(WindowId(1), TabId(1));
        input.repeat.configure(25, 200).unwrap();
        assert_eq!(input.key(30, true, false, &clock).unwrap().unwrap(), b"a");
        input.key(30, false, false, &clock).unwrap();
        input.map.as_mut().unwrap().update_modifiers(Modifiers {
            depressed: kbvm::ModifierMask::SHIFT.0,
            ..Default::default()
        });
        assert_eq!(input.key(30, true, false, &clock).unwrap().unwrap(), b"A");
        input.key(30, false, false, &clock).unwrap();
        input.map.as_mut().unwrap().update_modifiers(Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0,
            ..Default::default()
        });
        assert_eq!(
            input.key(30, true, false, &clock).unwrap().unwrap(),
            b"\x01"
        );
        input
            .map
            .as_mut()
            .unwrap()
            .update_modifiers(Modifiers::default());
        assert_eq!(input.key(28, true, false, &clock).unwrap().unwrap(), b"\r");
        assert_eq!(
            input.key(103, true, false, &clock).unwrap().unwrap(),
            b"\x1b[A"
        );
        input.key(103, false, false, &clock).unwrap();
        assert_eq!(
            input.key(103, true, true, &clock).unwrap().unwrap(),
            b"\x1bOA"
        );
        clock
            .advance(std::time::Duration::from_millis(200))
            .unwrap();
        assert_eq!(input.tick(false, &clock).unwrap().unwrap(), b"\x1b[A");
        input.clear_focus();
        assert!(input.tick(false, &clock).unwrap().is_none());
        assert!(input.key(30, true, false, &clock).unwrap().is_none());
    }
}
