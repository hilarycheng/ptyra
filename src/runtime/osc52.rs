//! Output cannot authorize itself: explicit config plus a real, unused focused input serial.
use super::{
    Runtime,
    clipboard::{Binding, CopyPlan},
};
use crate::{
    clipboard::{SeatId, SelectionKind},
    input::FocusTarget,
    terminal::Osc52Write,
};
use wayland_client::QueueHandle;

fn prepare(
    write: Osc52Write,
    enabled: bool,
    binding: &mut Binding,
    seat: Option<SeatId>,
    focus: Option<FocusTarget>,
    limit: usize,
) -> super::Result<CopyPlan> {
    if !enabled {
        return Err("write disabled");
    }
    let target = focus
        .filter(|target| target.tab == write.tab)
        .ok_or("source is not focused")?;
    let seat = seat.ok_or("selection unavailable")?;
    if binding.current.is_none() {
        return Err("selection unavailable");
    }
    if write.text.len() > limit {
        return Err("write exceeds clipboard limit");
    }
    let requests = crate::clipboard::plan_data_source(&mut binding.serials, seat, target)
        .ok_or("no valid input serial")?;
    Ok(CopyPlan {
        text: write.text,
        requests,
    })
}

impl Runtime {
    pub(super) fn publish_osc52(&mut self, id: crate::TabId, qh: &QueueHandle<Self>) {
        // At most one bounded, latest write per selection per PTY dispatch. Rejections
        // are discarded now, never replayed on a later keypress or focus change.
        for selection in [SelectionKind::Clipboard, SelectionKind::Primary] {
            let Some(write) = self
                .tabs
                .get_mut(id)
                .and_then(|tab| tab.terminal_mut().take_osc52_write(selection))
            else {
                continue;
            };
            let enabled = self.tabs.osc52_write_enabled();
            let seat = self.seat_name.map(SeatId);
            let focus = self.input.focus.target();
            let result = match write.selection {
                SelectionKind::Clipboard => prepare(
                    write,
                    enabled,
                    &mut self.clipboard.binding,
                    seat,
                    focus,
                    self.clipboard.limit,
                )
                .and_then(|plan| self.clipboard.publish(id, plan, qh)),
                SelectionKind::Primary => prepare(
                    write,
                    enabled,
                    &mut self.primary.binding,
                    seat,
                    focus,
                    self.primary.limit,
                )
                .and_then(|plan| self.primary.publish(id, plan, qh)),
            };
            if let Err(error) = result {
                eprintln!("Ptyra OSC 52: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        clipboard::{DataSourceRequest, InputSerialEvent},
        config::Config,
        tabs::TabPty,
    };
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    #[test]
    fn runtime_osc52_respects_explicit_write_policy() {
        let mut tabs = Config::default().tab_registry::<Fake>().unwrap();
        tabs.insert(TabId(1), 2, 8).unwrap();
        let payload = b"\x1b]52;c;5LitIAo=\x07\x1b]52;p;UEFTVEU=\x07";
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), payload)
            .unwrap();
        assert!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .terminal_mut()
                .take_osc52_write(SelectionKind::Clipboard)
                .is_none()
        );
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(1),
        };
        let seat = SeatId(2);
        let mut clipboard = Binding::default();
        clipboard.sync(Some((3, seat)));
        let mut primary = Binding::default();
        primary.sync(Some((4, seat)));
        tabs.set_osc52_write_enabled(true);
        for binding in [&mut clipboard, &mut primary] {
            assert!(binding.record(seat, target, 42, InputSerialEvent::KeyPress));
        }
        let terminal = tabs.get_mut(TabId(1)).unwrap().terminal_mut();
        terminal.feed(TabId(1), payload).unwrap();
        for (kind, binding, expected) in [
            (SelectionKind::Clipboard, &mut clipboard, "中 \n"),
            (SelectionKind::Primary, &mut primary, "PASTE"),
        ] {
            let write = terminal.take_osc52_write(kind).unwrap();
            assert_eq!(write.selection, kind);
            let plan = prepare(write, true, binding, Some(seat), Some(target), 1024).unwrap();
            assert_eq!(plan.text, expected);
            assert!(
                matches!(plan.requests[3], DataSourceRequest::SetSelection { serial, .. } if serial.0 == 42)
            );
            assert!(terminal.take_osc52_write(kind).is_none());
        }
        let write = |tab, text: &str| Osc52Write {
            tab,
            selection: SelectionKind::Clipboard,
            text: text.into(),
        };
        assert!(
            prepare(
                write(TabId(1), "secret"),
                true,
                &mut clipboard,
                Some(seat),
                Some(target),
                1024
            )
            .is_err()
        );
        clipboard.record(seat, target, 43, InputSerialEvent::KeyPress);
        for (enabled, tab, focus, current_seat, limit) in [
            (false, TabId(1), Some(target), Some(seat), 1024),
            (true, TabId(2), Some(target), Some(seat), 1024),
            (true, TabId(1), None, Some(seat), 1024),
            (true, TabId(1), Some(target), None, 1024),
            (true, TabId(1), Some(target), Some(SeatId(99)), 1024),
            (true, TabId(1), Some(target), Some(seat), 2),
        ] {
            assert!(
                prepare(
                    write(tab, "secret"),
                    enabled,
                    &mut clipboard,
                    current_seat,
                    focus,
                    limit
                )
                .is_err()
            );
        }
        // Failure before publication does not consume an unrelated selection serial.
        assert!(
            prepare(
                write(TabId(1), ""),
                true,
                &mut clipboard,
                Some(seat),
                Some(target),
                1024
            )
            .unwrap()
            .text
            .is_empty()
        );
        clipboard.record(seat, target, 44, InputSerialEvent::KeyPress);
        clipboard.focus(None);
        assert!(
            prepare(
                write(TabId(1), "stale"),
                true,
                &mut clipboard,
                Some(seat),
                Some(target),
                1024
            )
            .is_err()
        );
        primary.sync(None);
        assert!(
            prepare(
                write(TabId(1), "missing"),
                true,
                &mut primary,
                Some(seat),
                Some(target),
                1024
            )
            .is_err()
        );
        tabs.insert(TabId(2), 2, 8).unwrap();
        for id in [TabId(1), TabId(2)] {
            tabs.get_mut(id)
                .unwrap()
                .terminal_mut()
                .feed(id, payload)
                .unwrap();
        }
        tabs.set_osc52_write_enabled(false);
        tabs.insert(TabId(3), 2, 8).unwrap();
        for id in [TabId(1), TabId(2), TabId(3)] {
            let terminal = tabs.get_mut(id).unwrap().terminal_mut();
            for kind in [SelectionKind::Clipboard, SelectionKind::Primary] {
                assert!(terminal.take_osc52_write(kind).is_none());
            }
            terminal.feed(id, payload).unwrap();
            for kind in [SelectionKind::Clipboard, SelectionKind::Primary] {
                assert!(terminal.take_osc52_write(kind).is_none());
            }
        }
        tabs.set_osc52_write_enabled(true);
        let terminal = tabs.get_mut(TabId(1)).unwrap().terminal_mut();
        terminal
            .feed(TabId(1), b"\x1b]52;c;?\x07\x1b]52;p;?\x07")
            .unwrap();
        assert!(
            terminal
                .take_osc52_write(SelectionKind::Clipboard)
                .is_none()
        );
        assert!(terminal.take_osc52_write(SelectionKind::Primary).is_none());
    }
}
