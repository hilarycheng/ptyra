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
