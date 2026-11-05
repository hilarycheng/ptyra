use super::{TabPty, TabRegistry};
use crate::{
    clock::Clock,
    damage::Rect,
    error::{AppError, Component},
    frame::FrameScheduler,
    geometry::Scale,
    renderer::{
        Renderer,
        dirty::{PreparedFrame, render_visible_dirty},
    },
    selection::Screen,
    window::WindowSize,
};
use std::io::ErrorKind;

impl<P: TabPty> TabRegistry<P> {
    /// Use one registry and scheduler per Window; hidden Tab damage never schedules a terminal frame.
    pub fn render_active<R: Renderer>(
        &mut self,
        frames: &mut FrameScheduler,
        clock: &impl Clock,
        renderer: &mut R,
        size: WindowSize,
        scale: Scale,
    ) -> Result<Option<PreparedFrame<R::Frame>>, AppError> {
        let physical = scale.to_physical(size)?;
        frames.resize(physical);
        let switched = self.render_target != self.active;
        if switched {
            frames.suspend();
            self.render_target = self.active;
        }
        let Some(id) = self.active else {
            return Ok(None);
        };
        let tab = self
            .get_mut(id)
            .ok_or_else(|| AppError::new(Component::Renderer, ErrorKind::NotFound))?;
        if switched || tab.dirty {
            frames.damage(Rect {
                x: 0,
                y: 0,
                width: physical.width(),
                height: physical.height(),
            });
        }
        let mut snapshot = tab
            .terminal
            .snapshot()
            .ok_or_else(|| AppError::new(Component::Renderer, ErrorKind::NotConnected))?;
        snapshot.content.selection = tab
            .terminal
            .mode()
            .and_then(|mode| tab.selections.range(id, Screen::from_mode(mode)));
        let result = render_visible_dirty(frames, clock, true, renderer, size, snapshot)?;
        if result.is_some() {
            tab.dirty = false;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        clock::FakeClock,
        renderer::{RenderInput, RenderOutput},
        tabs::tests::FakePty,
    };
    #[derive(Default)]
    struct Backend {
        drawn: Vec<TabId>,
        fail: bool,
    }
    impl Renderer for Backend {
        type Frame = ();
        fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<()>, AppError> {
            if self.fail {
                return Err(AppError::new(Component::Renderer, ErrorKind::Other));
            }
            self.drawn.push(input.terminal.tab);
            Ok(RenderOutput {
                window: input.window,
                tab: input.terminal.tab,
                size: input.size,
                frame: (),
            })
        }
    }

    #[test]
    fn only_active_tab_is_rendered() {
        let size = WindowSize::new(160, 40).unwrap();
        let scale = Scale::new(2).unwrap();
        let mut frames = FrameScheduler::new(WindowId(1), scale.to_physical(size).unwrap());
        let clock = FakeClock::default();
        let mut backend = Backend::default();
        let mut tabs = TabRegistry::<FakePty>::default();
        assert!(
            tabs.render_active(&mut frames, &clock, &mut backend, size, scale)
                .unwrap()
                .is_none()
        );
        for id in [1, 2] {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        let (first, _) = tabs
            .render_active(&mut frames, &clock, &mut backend, size, scale)
            .unwrap()
            .unwrap();
        assert_eq!(
            first.damage,
            Rect {
                x: 0,
                y: 0,
                width: 320,
                height: 80
            }
        );
        frames.frame_done(first.callback);
        tabs.get_mut(TabId(2))
            .unwrap()
            .terminal_mut()
            .feed(TabId(2), b"hidden")
            .unwrap();
        assert!(
            tabs.render_active(&mut frames, &clock, &mut backend, size, scale)
                .unwrap()
                .is_none()
        );
        tabs.activate(TabId(2));
        let (second, _) = tabs
            .render_active(&mut frames, &clock, &mut backend, size, scale)
            .unwrap()
            .unwrap();
        assert_eq!(backend.drawn, [TabId(1), TabId(2)]);
        tabs.activate(TabId(1));
        let (third, _) = tabs
            .render_active(&mut frames, &clock, &mut backend, size, scale)
            .unwrap()
            .unwrap();
        assert!(!frames.frame_done(second.callback));
        assert!(frames.frame_done(third.callback));
        assert!(
            tabs.render_active(&mut frames, &clock, &mut backend, size, scale)
                .unwrap()
                .is_none()
        );
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"changed")
            .unwrap();
        backend.fail = true;
        assert!(
            tabs.render_active(&mut frames, &clock, &mut backend, size, scale)
                .is_err()
        );
        backend.fail = false;
        assert!(
            tabs.render_active(&mut frames, &clock, &mut backend, size, scale)
                .unwrap()
                .is_some()
        );
        assert_eq!(backend.drawn, [TabId(1), TabId(2), TabId(1), TabId(1)]);
    }
}
