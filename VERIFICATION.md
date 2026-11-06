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

## TASK-136

- Dependencies satisfied. All grids and cell reservations are validated together before resizing; over-budget requests preserve prior dimensions/reservations and report metadata once per rejected size. Active and hidden PTYs receive the accepted size.
- Terminal content deducts one font-height plus 8 pixels for the tab strip; terminal drawing uses the remaining buffer.
- `runtime_resize_updates_tab_budgets`, workspace regression, fmt, Clippy: PASS.
- Native multi-tab test additionally with `PTYRA_TEST_RESIZE_TABS=1`: PASS; both real Shells report identical resized PTY dimensions, and input/exit isolation still pass.

## TASK-137

- Dependencies satisfied. The native buffer draws tab numbers/titles and a blue active state in a reserved strip above terminal rows. Bounded pages always include the active Tab; Next/Previous reaches off-page tabs. No compositor-tabbed substitute is used.
- `runtime_tab_bar_layout_reserves_content_area`, workspace regression, fmt, Clippy: PASS.
- Native multi-tab test with `PTYRA_TEST_TAB_BAR=1`: PASS, including compositor pixel evidence and resize. Screenshot visually inspected; strip cursor artifact removed and regression rerun.
- T140 precedes T138 because pointer binding is a declared dependency of tab clicking.

## TASK-140 (before TASK-138 by dependency)

- Dependencies satisfied. Seat pointer capability binds/releases wl_pointer; surface enter/leave, motion, button and axis events capture TabId and surface coordinates. Input serials are retained by the existing seat policy; enter serial cannot authorize clipboard publication. Capability removal, focus loss and tab changes cancel drag/serial state. Stale pointer objects/surfaces and stale routed targets are ignored.
- `runtime_pointer_lifecycle_preserves_serials`, workspace regression, fmt, Clippy: PASS.
- Native test with `PTYRA_TEST_POINTER=1`: PASS; actual virtual-pointer enter/press/release/axis events reach the native adapter. Trace is opt-in and contains event categories only.
- Scale remains the existing runtime scale=1; output scaling is separately TASK-167. Selection and application mouse reporting remain later tasks.

## TASK-138

- Dependencies satisfied, including TASK-140. Left press hit-tests the same layout that was last submitted to the compositor and routes through common Tab activation. Resized layouts and removed IDs reject stale clicks; terminal-area clicks do not activate tabs.
- `runtime_tab_bar_click_selects_exact_tab`, workspace regression, fmt, Clippy: PASS.
- Native test with `PTYRA_TEST_POINTER=1 PTYRA_TEST_TAB_CLICK=1`: PASS; real clicks on first/second labels route a PID probe to exactly the corresponding Shell.

## TASK-139

- Dependencies satisfied. Each drained terminal refreshes its sanitized, bounded label; title changes redraw the tab strip. Only the current active label updates xdg-toplevel title; switching tabs updates it too. Title text is never executed.
- `runtime_titles_are_sanitized_and_tab_scoped`, workspace regression (205 library tests), fmt, Clippy: PASS.
- Native multi-tab test with `PTYRA_TEST_TITLES=1`: PASS; active, delayed hidden title, switch and title-stack ResetTitle event update the correct window/tab. The terminal core does not emit ResetTitle on RIS; the tested reset is title-stack restoration.

## Final regression review — TASK-130–140

- Reproduced and fixed three edge cases: shifted digit/Tab shortcut aliases, pointer serial focus after drag cancellation, and retained terminal row capacity after shrinking. Added regressions passed after first failing as expected.
- Shortcut matching prefers explicit translated symbols, then unshifted symbols; reserved IME-toggle matches remain compositor-owned. Every configured action is covered.
- Resize accounting now reserves conservative high-water rows/columns until Tab close. Shrink does not free budget while the core retains row capacities.
- Shared `unicode-width` direct dependency and review metadata are included for tab labels; IME-specific protocol feature and implementation remain uncommitted.
- Final worktree: 207 library tests + 13 integration tests + 2 doc tests PASS; one pre-existing opt-in integration test ignored. Standalone staged tree without T129: 205 library tests + the same integration/doc tests PASS. Both fmt and Clippy PASS.
- Full owned-headless-Sway matrix PASS on both trees: held NewTab, exact Shell PID switching/clicks, genuinely hidden 256 KiB flood, native strip pixels, active/hidden/reset titles, both PTYs resize, per-tab exit, last-tab Close, repeat/focus and reconnect.
- CPU affinity remained CPUs 0–1; Cargo jobs=2. No push, system configuration changes, or T141+ implementation. Actual desktop/IME candidate acceptance remains NOT RUN.

## TASK-141

- Dependencies satisfied. Native left-button press/motion/release now update the original Tab and screen selection, deducting the tab strip origin. Reverse ranges expand to whole CJK glyphs and preserve combining marks. Starts outside content are ignored; ongoing drags clip to the nearest grid edge. Pointer leave, focus loss, tab switch/close and screen mismatch cancel the drag. No PTY bytes are generated.
- `runtime_drag_selects_whole_glyphs`: initially failed before implementation, then PASS. Workspace regression, fmt and Clippy: PASS in the worktree and the staged tree excluding T129.
- `PTYRA_TEST_POINTER=1 PTYRA_TEST_SELECTION=1 python3 tests/native_runtime.py ...`: PASS in owned headless Sway, including real native drag events and keyboard/reconnect regression.
- CPU affinity 0–1 and Cargo jobs=2. Selection highlight is TASK-143; screen/content mutation invalidation is TASK-144. Actual desktop acceptance: NOT RUN. T129 edits remain uncommitted and preserved.

## TASK-142

- Dependencies satisfied. An injectable monotonic clock groups up to three clicks within 400 ms and a 4-pixel tolerance. Double click uses the existing Unicode word policy; triple click selects the complete soft-wrapped logical line. Timeout, pointer displacement, focus/tab cancellation and screen identity reset the count. A fourth click starts a new single-cell drag.
- `runtime_click_count_selects_word_or_line`: initially failed, then PASS, including ASCII/CJK/combining, wrapped rows, timeout, displacement, Tab and screen resets. Full workspace, fmt and Clippy pass both combined and T129-free staged trees.
- Owned-headless native matrix with `PTYRA_TEST_POINTER=1 PTYRA_TEST_SELECTION=1 PTYRA_TEST_MULTI_CLICK=1`: PASS on both trees, checking actual ASCII word, whole CJK and multi-row logical-line selection. Opt-in tracing contains coordinates/cell geometry only, not selected text. Desktop acceptance remains NOT RUN.

## TASK-143

- Dependencies satisfied. The active Tab's whole-glyph selection range is attached to a renderer snapshot and consumed by the existing independent overlay iterator. A bounded RGB inversion layer preserves glyph pixels and alpha; it never changes terminal cells, cursor, or PTY input. Cleared selections redraw from the base frame.
- `runtime_selection_overlay_matches_range`: initially failed, then PASS. Tests cover two-cell CJK spans, unchanged outside pixels/alpha/cursor/text, clearing and malformed buffer size. Workspace, fmt and Clippy PASS in both worktree and T129-free staged tree.
- Native pointer/multi-click matrix plus `PTYRA_TEST_SELECTION_PIXELS=1`: PASS on both trees. Compositor pixels verify the full two-cell highlight; `/tmp/ptyra-t143-clean-selection.png` visually inspected. This is headless Sway evidence, not desktop/font/IME acceptance.
