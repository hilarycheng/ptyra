use super::{Result, Runtime};
use crate::{
    WindowId,
    clipboard::{PasteSink, SeatId},
    ime::{ImeEvent, ImeFocus, ImeRequest, ImeState, preedit::PreeditState},
    input::KeyboardFocus,
    wayland::{Capabilities, Global},
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    self, ContentHint, ContentPurpose,
};
pub(super) use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3 as TextInputManager,
    zwp_text_input_v3::ZwpTextInputV3 as TextInput,
};

pub(super) struct ImeInput {
    pub state: ImeState,
    pub preedit: Option<PreeditState>,
    focus_serial: u32,
    batch_focus: Option<ImeFocus>,
    state_is_current: bool,
}

#[derive(Default)]
struct Done {
    changed: bool,
    rejected: bool,
    committed: bool,
}

impl ImeInput {
    pub(super) fn damage(
        &mut self,
        terminal: &crate::terminal::Terminal,
        layout: &crate::renderer::CellLayout,
        keyboard: &KeyboardFocus,
    ) -> Result<Option<super::damage::Band>> {
        use crate::renderer::overlay::{OverlayCommand, Preedit, overlay_commands};
        let Some(preedit) = &mut self.preedit else {
            return Ok(None);
        };
        let preedit = preedit.preedit(&self.state, keyboard);
        let commands = overlay_commands(
            terminal.snapshot().ok_or("terminal unavailable")?,
            layout,
            Some(Preedit {
                tab: terminal.tab(),
                text: &preedit.text,
            }),
        )
        .map_err(|_| "invalid IME overlay")?;
        Ok(commands
            .filter_map(|command| match command {
                OverlayCommand::Preedit { clip, .. } => {
                    super::damage::Band::new(clip.y as u32, clip.y as u32 + clip.height)
                }
                _ => None,
            })
            .next())
    }

    fn new(seat: u32, manager: u32) -> Self {
        let capabilities = Capabilities::from_globals(&[Global {
            name: manager,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        Self {
            state: ImeState::new(SeatId(seat), &capabilities),
            preedit: None,
            focus_serial: 0,
            batch_focus: None,
            state_is_current: true,
        }
    }

    fn sync_focus(&mut self, keyboard: &KeyboardFocus) -> &'static [ImeRequest] {
        let requests = self.state.sync_enabled(self.state.seat(), keyboard, true);
        if !requests.is_empty() {
            self.preedit = keyboard.target().map(PreeditState::new);
            self.focus_serial = self.state.commit_serial();
            self.batch_focus = None;
            self.state_is_current = true;
        }
        if let Some(preedit) = &mut self.preedit {
            preedit.sync_focus(&self.state, keyboard);
        }
        requests
    }

    fn stage(&mut self, keyboard: &KeyboardFocus, event: ImeEvent<'_>) {
        let focus = self.batch_focus.get_or_insert_with(|| {
            self.state
                .capture_focus(self.state.seat(), WindowId(1), keyboard)
        });
        // The reducer invalidates malformed batches without logging input text.
        let _ = self.state.stage(focus, keyboard, event);
    }

    fn done(
        &mut self,
        keyboard: &KeyboardFocus,
        serial: u32,
        sink: &mut impl PasteSink,
    ) -> Result<Done> {
        let focus = self.batch_focus.take().unwrap_or_else(|| {
            self.state
                .capture_focus(self.state.seat(), WindowId(1), keyboard)
        });
        let Some(mut update) = self.state.done(&focus, keyboard, serial) else {
            return Ok(Done::default());
        };
        // Accept older cursor updates only within this focus transaction, including serial wrap.
        if serial.wrapping_sub(self.focus_serial)
            > self.state.commit_serial().wrapping_sub(self.focus_serial)
        {
            return Ok(Done::default());
        }
        self.state_is_current = update.state_is_current;
        // The transaction is consumed once, including a visibly rejected commit.
        let (committed, rejected) = match update.enqueue_commit(&self.state, keyboard, sink) {
            Ok(committed) => (committed, false),
            Err(_) => (false, true),
        };
        if let Some(preedit) = &mut self.preedit {
            preedit
                .apply(&mut update, &self.state, keyboard)
                .map_err(|_| "invalid IME overlay")?;
        }
        Ok(Done {
            changed: true,
            rejected,
            committed,
        })
    }
}

fn send_requests(input: &TextInput, requests: &[ImeRequest]) {
    for request in requests {
        match request {
            ImeRequest::Enable => {
                input.enable();
                input.set_content_type(ContentHint::None, ContentPurpose::Terminal);
            }
            ImeRequest::Disable => input.disable(),
            ImeRequest::Commit => input.commit(),
            ImeRequest::SetCursorRectangle(rect) => {
                input.set_cursor_rectangle(rect.x, rect.y, rect.width as i32, rect.height as i32)
            }
        }
    }
}

impl Runtime {
    pub(super) fn bind_text_input(&mut self, qh: &QueueHandle<Self>) {
        if self.text_input.is_none()
            && let (Some((name, manager)), Some(seat), Some(seat_name)) =
                (&self.text_input_manager, &self.seat, self.seat_name)
        {
            self.ime = Some(ImeInput::new(seat_name, *name));
            self.text_input = Some(manager.get_text_input(seat, qh, ()));
        }
    }

    pub(super) fn remove_text_input(&mut self) {
        if let Some(input) = self.text_input.take() {
            input.destroy();
        }
        self.ime = None;
        self.paint.dirty = true;
    }

    pub(super) fn sync_ime_focus(&mut self) {
        if let (Some(ime), Some(input)) = (&mut self.ime, &self.text_input) {
            let no_focus = KeyboardFocus::default();
            let focus = if self
                .tabs
                .active()
                .and_then(|id| self.tabs.get(id))
                .is_some_and(|tab| tab.pty().is_some())
            {
                &self.input.focus
            } else {
                &no_focus
            };
            send_requests(input, ime.sync_focus(focus));
        }
    }

    pub(super) fn update_ime_cursor(&mut self) -> Result<()> {
        if self
            .tabs
            .active()
            .and_then(|id| self.tabs.get(id))
            .is_some_and(|tab| tab.failure().is_some())
        {
            self.sync_ime_focus();
        }
        let (bar_height, content_size) = self.content_geometry();
        if let (Some(ime), Some(input)) = (&mut self.ime, &self.text_input)
            && ime.state_is_current
        {
            let layout = self.raster.layout(content_size)?;
            let Some(tab) = self.tabs.active().and_then(|id| self.tabs.get(id)) else {
                return Ok(());
            };
            let snapshot = tab.terminal().snapshot().ok_or("terminal unavailable")?;
            if let Some(mut requests) = ime.state.update_cursor_rectangle(
                ime.state.seat(),
                &self.input.focus,
                snapshot,
                &layout,
            ) {
                if let ImeRequest::SetCursorRectangle(rect) = &mut requests[0] {
                    rect.y += bar_height as i32;
                }
                send_requests(input, &requests);
            }
        }
        Ok(())
    }

    fn text_input_event(&mut self, event: zwp_text_input_v3::Event) -> Result<()> {
        use zwp_text_input_v3::Event;
        if matches!(event, Event::Done { .. }) {
            self.sync_terminal()?;
        }
        let Some(ime) = &mut self.ime else {
            return Ok(());
        };
        match event {
            Event::Enter { surface } if self.surface.as_ref() == Some(&surface) => {
                ime.batch_focus = None;
                ime.state.enter(ime.state.seat(), WindowId(1));
                self.sync_ime_focus();
                self.paint.dirty = true;
            }
            Event::Leave { surface } if self.surface.as_ref() == Some(&surface) => {
                ime.batch_focus = None;
                ime.state.leave(ime.state.seat(), WindowId(1));
                self.sync_ime_focus();
                self.paint.dirty = true;
            }
            Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => ime.stage(
                &self.input.focus,
                ImeEvent::Preedit {
                    text: text.as_deref(),
                    cursor_begin,
                    cursor_end,
                },
            ),
            Event::CommitString { text } => ime.stage(
                &self.input.focus,
                ImeEvent::Commit {
                    text: text.as_deref(),
                },
            ),
            Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => ime.stage(
                &self.input.focus,
                ImeEvent::DeleteSurroundingText {
                    before_length,
                    after_length,
                },
            ),
            Event::Done { serial } => {
                if let Some(pty) = self
                    .tabs
                    .active()
                    .and_then(|id| self.tabs.get_mut(id))
                    .and_then(|tab| tab.pty_mut())
                {
                    let done = ime.done(&self.input.focus, serial, pty)?;
                    self.paint.dirty |= done.changed;
                    if done.rejected {
                        self.input.repeat.cancel();
                        if self.clipboard_notice != Some(super::notice::Notice::InputRejected) {
                            self.show_clipboard_notice(super::notice::Notice::InputRejected);
                        }
                    } else if done.committed
                        && self.clipboard_notice == Some(super::notice::Notice::InputRejected)
                    {
                        self.clear_clipboard_notice();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Dispatch<TextInput, ()> for Runtime {
    fn event(
        state: &mut Self,
        input: &TextInput,
        event: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.text_input.as_ref() == Some(input)
            && let Err(error) = state.text_input_event(event)
        {
            state.fail(error);
        }
    }
}
delegate_noop!(Runtime: ignore TextInputManager);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId, clipboard::PasteSink, ime::ImeEvent, input::KeyboardFocus};

    #[test]
    fn runtime_ime_tab_switch_invalidates_old_batches() {
        struct Sink(TabId, Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                self.0
            }
            fn enqueue(&mut self, bytes: &[u8]) -> std::result::Result<(), Self::Error> {
                self.1.extend_from_slice(bytes);
                Ok(())
            }
        }
        let mut input = ImeInput::new(7, 3);
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(7));
        input.state.enter(input.state.seat(), WindowId(1));
        assert_eq!(
            input.sync_focus(&focus),
            &[ImeRequest::Enable, ImeRequest::Commit]
        );
        let mut first = Sink(TabId(7), Vec::new());
        let mut second = Sink(TabId(8), Vec::new());
        let old = input.state.commit_serial();
        input.stage(
            &focus,
            ImeEvent::Preedit {
                text: Some("舊"),
                cursor_begin: 3,
                cursor_end: 3,
            },
        );
        input.done(&focus, old, &mut first).unwrap();
        assert_eq!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &focus)
                .text,
            "舊"
        );
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("取消")
            },
        );
        focus.enter(WindowId(1), TabId(8));
        assert_eq!(
            input.sync_focus(&focus),
            &[
                ImeRequest::Disable,
                ImeRequest::Commit,
                ImeRequest::Enable,
                ImeRequest::Commit
            ]
        );
        assert!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &focus)
                .text
                .is_empty()
        );
        assert!(!input.done(&focus, old, &mut second).unwrap().committed);
        // Entire old batches can arrive after the local Tab switch.
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("遲到")
            },
        );
        assert!(!input.done(&focus, old, &mut second).unwrap().committed);
        let current = input.state.commit_serial();
        let terminal = crate::terminal::Terminal::new(TabId(8), 4, 8).unwrap();
        let layout = crate::renderer::CellLayout::new(
            crate::window::WindowSize::new(80, 80).unwrap(),
            crate::geometry::Scale::new(1).unwrap(),
            10.0,
            20.0,
        )
        .unwrap();
        input
            .state
            .update_cursor_rectangle(
                input.state.seat(),
                &focus,
                terminal.snapshot().unwrap(),
                &layout,
            )
            .unwrap();
        // Cursor updates do not invalidate text from the same focus transaction.
        input.stage(
            &focus,
            ImeEvent::Preedit {
                text: Some("新"),
                cursor_begin: 3,
                cursor_end: 3,
            },
        );
        input.done(&focus, current, &mut second).unwrap();
        assert!(!input.state_is_current);
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("遲到")
            },
        );
        assert!(!input.done(&focus, old, &mut second).unwrap().committed);
        assert_eq!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &focus)
                .text,
            "新"
        );
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("中文")
            },
        );
        assert!(input.done(&focus, current, &mut second).unwrap().committed);
        assert!(!input.done(&focus, current, &mut second).unwrap().committed);
        assert_eq!(second.1, "中文".as_bytes());
        // Closing the active Tab retargets focus; queued events cannot reach its sibling.
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("關閉")
            },
        );
        focus.enter(WindowId(1), TabId(7));
        input.sync_focus(&focus);
        input.done(&focus, current, &mut first).unwrap();
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("返回")
            },
        );
        let serial = input.state.commit_serial();
        assert!(input.done(&focus, serial, &mut first).unwrap().committed);
        assert_eq!(first.1, "返回".as_bytes());
        assert_eq!(second.1, "中文".as_bytes());
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("失焦")
            },
        );
        focus.clear();
        input.sync_focus(&focus);
        input.done(&focus, serial, &mut first).unwrap();
        focus.enter(WindowId(1), TabId(7));
        input.sync_focus(&focus);
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("舊焦點"),
            },
        );
        input.done(&focus, serial, &mut first).unwrap();
        assert_eq!(first.1, "返回".as_bytes());
    }

    #[test]
    fn runtime_ime_preedit_damage_restores_on_focus_loss() {
        use super::super::damage::{Band, State, View};
        let mut input = ImeInput::new(7, 3);
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        input.state.enter(input.state.seat(), WindowId(1));
        input.sync_focus(&focus);
        let terminal = crate::terminal::Terminal::new(TabId(1), 4, 8).unwrap();
        let layout = crate::renderer::CellLayout::new(
            crate::window::WindowSize::new(80, 80).unwrap(),
            crate::geometry::Scale::new(1).unwrap(),
            10.0,
            20.0,
        )
        .unwrap();
        let view = || View {
            tab: TabId(1),
            size: (80, 80),
            cell: (10, 20),
            scale: 1,
            origin: 0,
            titles: Vec::new(),
        };
        let mut damage = State::default();
        damage.prepare(view(), None, None);
        input.stage(
            &focus,
            ImeEvent::Preedit {
                text: Some("中文"),
                cursor_begin: 6,
                cursor_end: 6,
            },
        );
        let captured = input.batch_focus.take().unwrap();
        let mut update = input.state.done(&captured, &focus, 1).unwrap();
        input
            .preedit
            .as_mut()
            .unwrap()
            .apply(&mut update, &input.state, &focus)
            .unwrap();
        let band = input.damage(&terminal, &layout, &focus).unwrap();
        assert_eq!(band, Band::new(0, 20));
        assert_eq!(damage.prepare(view(), None, band).content, band);
        focus.clear();
        input.sync_focus(&focus);
        let cleared = input.damage(&terminal, &layout, &focus).unwrap();
        assert_eq!(cleared, None);
        assert_eq!(damage.prepare(view(), None, cleared).content, band);
        assert_eq!(damage.prepare(view(), None, None).content, None);
        assert_eq!(terminal.grid().unwrap().cursor.point.column.0, 0);
    }

    #[test]
    fn runtime_bad_stream_does_not_stop_other_tabs_ime() {
        struct Sink(Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                TabId(1)
            }
            fn enqueue(&mut self, bytes: &[u8]) -> std::result::Result<(), Self::Error> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let mut input = ImeInput::new(7, 3);
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        input.state.enter(input.state.seat(), WindowId(1));
        input.sync_focus(&focus);
        input.stage(
            &focus,
            ImeEvent::Commit {
                text: Some("not replayed"),
            },
        );
        // Runtime removes IME eligibility once that Tab has no PTY, even while
        // keyboard focus remains on its diagnostic so shortcuts still work.
        assert_eq!(
            input.sync_focus(&KeyboardFocus::default()),
            &[ImeRequest::Disable, ImeRequest::Commit]
        );
        let mut sink = Sink(Vec::new());
        assert!(
            !input
                .done(&focus, input.state.commit_serial(), &mut sink)
                .unwrap()
                .committed
        );
        assert!(sink.0.is_empty());
        assert!(input.preedit.is_none());
        focus.enter(WindowId(1), TabId(2));
        input.sync_focus(&focus);
        assert!(
            !input
                .done(&focus, input.state.commit_serial(), &mut sink)
                .unwrap()
                .committed
        );
        assert!(sink.0.is_empty());
    }
    #[test]
    fn runtime_input_backpressure_preserves_order_without_global_exit_ime() {
        struct Full;
        impl PasteSink for Full {
            type Error = ();
            fn tab(&self) -> TabId {
                TabId(1)
            }
            fn enqueue(&mut self, _: &[u8]) -> std::result::Result<(), Self::Error> {
                Err(())
            }
        }
        let mut input = ImeInput::new(7, 3);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(WindowId(1), TabId(1));
        input.state.enter(input.state.seat(), WindowId(1));
        input.sync_focus(&keyboard);
        input.stage(
            &keyboard,
            ImeEvent::Preedit {
                text: Some("候選"),
                cursor_begin: 0,
                cursor_end: 6,
            },
        );
        assert!(!input.done(&keyboard, 1, &mut Full).unwrap().rejected);
        assert_eq!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &keyboard)
                .text,
            "候選"
        );
        input.stage(
            &keyboard,
            ImeEvent::Commit {
                text: Some("拒絕")
            },
        );
        let rejected = input
            .done(&keyboard, 1, &mut Full)
            .expect("recoverable rejection");
        assert!(rejected.changed && rejected.rejected);
        assert!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &keyboard)
                .text
                .is_empty(),
            "rejection still applies the done overlay"
        );
        let duplicate = input.done(&keyboard, 1, &mut Full).unwrap();
        assert!(
            !duplicate.rejected,
            "a consumed rejected commit is never replayed"
        );
    }

    #[test]
    fn runtime_ime_commits_to_shell() {
        struct Sink(Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                TabId(1)
            }
            fn enqueue(&mut self, bytes: &[u8]) -> std::result::Result<(), Self::Error> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let mut input = ImeInput::new(7, 3);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(WindowId(1), TabId(1));
        input.state.enter(input.state.seat(), WindowId(1));
        assert_eq!(
            input.sync_focus(&keyboard),
            &[ImeRequest::Enable, ImeRequest::Commit]
        );
        let mut sink = Sink(Vec::new());
        input.stage(
            &keyboard,
            ImeEvent::Preedit {
                text: Some("繁"),
                cursor_begin: 3,
                cursor_end: 3,
            },
        );
        assert!(sink.0.is_empty());
        input.done(&keyboard, 1, &mut sink).unwrap();
        assert_eq!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &keyboard)
                .text,
            "繁"
        );
        assert!(sink.0.is_empty());
        input.stage(
            &keyboard,
            ImeEvent::Commit {
                text: Some("繁體中文"),
            },
        );
        assert!(sink.0.is_empty());
        input.done(&keyboard, 1, &mut sink).unwrap();
        input.done(&keyboard, 1, &mut sink).unwrap();
        assert_eq!(sink.0, "繁體中文".as_bytes());
        assert!(
            input
                .preedit
                .as_mut()
                .unwrap()
                .preedit(&input.state, &keyboard)
                .text
                .is_empty()
        );
        // Repeated serials are valid independent batches, not duplicate text.
        input.stage(&keyboard, ImeEvent::Commit { text: Some("字") });
        input.done(&keyboard, 1, &mut sink).unwrap();
        assert_eq!(sink.0, "繁體中文字".as_bytes());
        input.stage(&keyboard, ImeEvent::Commit { text: Some("舊") });
        keyboard.clear();
        input.sync_focus(&keyboard);
        keyboard.enter(WindowId(1), TabId(1));
        input.sync_focus(&keyboard);
        input.stage(&keyboard, ImeEvent::Commit { text: Some("新") });
        input
            .done(&keyboard, input.state.commit_serial(), &mut sink)
            .unwrap();
        assert_eq!(sink.0, "繁體中文字新".as_bytes());
        assert!(input.state_is_current);
        input
            .done(&keyboard, input.state.commit_serial(), &mut sink)
            .unwrap();
        assert!(input.state_is_current);
        input.stage(&keyboard, ImeEvent::Commit { text: Some("舊") });
        keyboard.clear();
        assert_eq!(
            input.sync_focus(&keyboard),
            &[ImeRequest::Disable, ImeRequest::Commit]
        );
        keyboard.enter(WindowId(1), TabId(1));
        input.sync_focus(&keyboard);
        input.done(&keyboard, 1, &mut sink).unwrap();
        assert_eq!(sink.0, "繁體中文字新".as_bytes());
        // Invalid UTF-8 cursor boundaries discard the complete batch.
        input.stage(&keyboard, ImeEvent::Commit { text: Some("壞") });
        input.stage(
            &keyboard,
            ImeEvent::Preedit {
                text: Some("中"),
                cursor_begin: 1,
                cursor_end: 1,
            },
        );
        input.done(&keyboard, 3, &mut sink).unwrap();
        assert_eq!(sink.0, "繁體中文字新".as_bytes());
        input.stage(
            &keyboard,
            ImeEvent::DeleteSurroundingText {
                before_length: 3,
                after_length: 3,
            },
        );
        input.done(&keyboard, 3, &mut sink).unwrap();
        assert_eq!(sink.0, "繁體中文字新".as_bytes());
    }
}
