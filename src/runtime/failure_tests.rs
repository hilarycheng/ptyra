//! Exercise the actual runtime error handlers without a compositor or global setup.
use super::*;
use crate::{TabId, WindowId, clipboard::SelectionKind, tabs::SessionFailure};
use std::{ffi::OsStr, path::Path, sync::Arc};

fn runtime() -> Runtime {
    let config = crate::config::Config {
        scrollback_lines: 0,
        ..Default::default()
    };
    let font = ab_glyph::FontVec::try_from_vec(
        include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
    )
    .unwrap();
    let raster = raster::Raster::new(vec![font], &config).unwrap();
    let mut app = new_runtime(config, raster).unwrap();
    for _ in 0..2 {
        sessions::spawn_tab(&mut app.tabs, 2, 40, |r| {
            PtySession::spawn(PtySpawnRequest {
                tab: r.tab,
                program: OsStr::new("/bin/sleep"),
                args: &["60".into()],
                cwd: Path::new("/tmp"),
                size: PtySize {
                    rows: 2,
                    columns: 40,
                },
            })
            .map_err(|_| "fixture child failed")
        })
        .unwrap();
    }
    app.paint.configured = true;
    app.input.focus.enter(WindowId(1), TabId(1));
    app.input.map = Some(
        crate::keymap::Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb"))
            .unwrap(),
    );
    app.input.repeat.configure(20, 100).unwrap();
    app.input
        .key(30, true, false, false, app.keyboard_flags(), &app.clock)
        .unwrap();
    assert!(
        app.input
            .repeat
            .deadline(&app.input.focus, app.input.map.as_ref().unwrap())
            .is_some()
    );
    app.paste =
        Some(paste::Paste::capture_selection(&app.input.focus, SelectionKind::Clipboard).unwrap());
    app.presented_paste = Some(Arc::new(()));
    app.ready_pty = vec![TabId(1), TabId(2)];
    app
}

#[test]
fn runtime_bad_stream_does_not_stop_other_tabs_production_io() {
    for write in [true, false] {
        let mut app = runtime();
        let epoch = app.input.focus.epoch();
        let pty = app.tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap();
        pty.enqueue_write(b"accepted-but-never-retargeted").unwrap();
        pty.terminate().unwrap(); // Queue remains owned; actual flush/resize now fails Closed.
        if write {
            app.flush_tab(TabId(1));
        } else {
            app.sync_terminal().unwrap();
        }
        assert!(app.running && app.failure.is_none());
        assert_eq!(
            app.tabs.get(TabId(1)).unwrap().failure(),
            Some(SessionFailure::Io)
        );
        assert!(app.tabs.get(TabId(1)).unwrap().pty().is_none());
        assert!(app.paste.is_none() && app.presented_paste.is_none());
        assert!(
            app.input
                .repeat
                .deadline(&app.input.focus, app.input.map.as_ref().unwrap())
                .is_none()
        );
        assert!(!Arc::ptr_eq(&epoch, &app.input.focus.epoch()));
        assert_eq!(app.ready_pty, [TabId(2)]);
        let pty = app.tabs.get_mut(TabId(2)).unwrap().pty_mut().unwrap();
        assert_eq!(pty.pending_write_bytes(), 0);
        pty.enqueue_write(b"other").unwrap();
        app.flush_tab(TabId(2));
        assert_eq!(
            app.tabs
                .get(TabId(2))
                .unwrap()
                .pty()
                .unwrap()
                .pending_write_bytes(),
            0
        );
        app.activate_tab(TabId(2));
        app.sync_terminal().unwrap();
        assert!(app.tabs.get(TabId(2)).unwrap().failure().is_none());
    }
}
