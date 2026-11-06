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
