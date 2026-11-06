# Runtime task verification

## TASK-130

- Dependencies: TASK-096 and TASK-128 satisfied.
- Configured Copy/Paste/New/Close/Next/Previous shortcuts route once on press; matching releases and repeat are consumed. IME toggle remains compositor-owned and forwarded unclaimed keys keep their prior path.
- Not-yet-wired actions emit an explicit unavailable diagnostic, without recording input payloads.
- `cargo test --workspace runtime_bindings_route_actions_once`: PASS.
- `cargo test --workspace`: PASS (196 library tests, 13 integration tests, 2 doc tests; 1 existing opt-in test ignored).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `python3 tests/native_runtime.py target/debug/ptyra`: PASS in an owned headless Sway session, including configured shortcut hold/release, ordinary keyboard, repeat/focus, rendered output and reconnect.
- CPU affinity restricted to CPUs 0–1; Cargo build jobs limited to 2.
- Pre-existing TASK-129 changes are not included in this commit. Actual desktop IME acceptance remains incomplete.

## TASK-131

- Dependencies satisfied. Every runtime terminal and PTY, including the initial Shell, is owned by TabRegistry. Spawn failure rolls back only the new reservation; IDs remain monotonic. Existing tab and cell caps apply.
- `runtime_new_tab_spawns_one_session`, workspace tests (197 library tests), fmt and Clippy: PASS.
- `PTYRA_TEST_NEW_TAB=1 python3 tests/native_runtime.py target/debug/ptyra`: PASS; one held shortcut creates exactly one additional real child, and original Shell input still works.
- Active-tab switching and background PTY drainage are separate following tasks. T129 compatibility edits remain uncommitted.

## TASK-132

- Dependencies satisfied. One poll loop registers every live PTY and routes readiness by TabId. The existing rotating drain limits each dispatch to four sessions and 64 KiB; hidden output does not redraw the active grid. Pending writes are flushed in the same bounded service pass.
- `runtime_multi_pty_poll_is_fair`, workspace regression, fmt, Clippy: PASS.
- `PTYRA_TEST_NEW_TAB=1 PTYRA_TEST_MULTI_PTY=1 python3 tests/native_runtime.py target/debug/ptyra`: PASS; hidden Shell drains 256 KiB and active keyboard remains responsive.
- Per-tab EOF handling remains TASK-135.

## TASK-133

- Dependencies satisfied. New/Next/Previous and direct activation share one active Tab; focus epochs and repeat are reset, other sessions remain unchanged, and rendering reads only the active grid.
- `runtime_tab_switch_retargets_keyboard`, workspace regression, fmt, Clippy: PASS.
- Native test with `PTYRA_TEST_NEW_TAB=1 PTYRA_TEST_MULTI_PTY=1 PTYRA_TEST_TAB_SWITCH=1`: PASS; exact Shell PIDs verify new/previous Tab keyboard isolation without extra spawning.
- T129 remains uncommitted. Its single-tab IME is disabled outside original TabId(1), rather than pretending multi-tab IME is complete; TASK-175 remains required.

## TASK-134

- Dependencies satisfied. Close removes only the named session/readiness entry, reaps that child, selects one successor, resets input focus when needed, and exits on last close. Stale IDs are no-ops and never reused.
- `runtime_close_tab_preserves_other_sessions`, workspace regression, fmt, Clippy: PASS.
- Native test additionally with `PTYRA_TEST_CLOSE_TAB=1`: PASS; closing second Tab removes only its exact child PID and the original Shell remains interactive.

## TASK-135

- Dependencies satisfied. Exit policy: drain through EOF, capture/reap that child, retain bounded TabId/exit-status metadata, and close only its Tab. The last Tab exits the app. A child that closes its PTY while still running is terminated under the same close policy.
- `runtime_child_exit_is_per_tab`, workspace regression, fmt, Clippy: PASS.
- Native multi-tab test with `PTYRA_TEST_EXIT_TAB=1`: PASS; exit 7 from second Shell preserves final output processing, reaps its child, and leaves original Shell interactive. Last exit and reconnect still pass.
