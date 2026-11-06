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

## TASK-144

- Dependencies satisfied. Selection anchors and click/drag ownership carry the terminal's content epoch. Conservative policy: any nonempty input-to-parser batch invalidates both screen selections, including cursor/control-only output and main→alt→main within one chunk. Changed grid size invalidates anchors; no-op resize and other Tabs do not. New selection starts against the current epoch; stale motion/release cannot rebuild it. Closed IDs are never reused; epoch exhaustion permanently disables selection for that Tab instead of wrapping.
- `runtime_selection_survives_only_valid_anchors` initially failed, then PASS. Additional regressions cover immediate click-count reset across screen round trips, epoch exhaustion, and the generic renderer snapshot. Review found and fixed the generic renderer's raw-selection accessor; its new regression failed before the fix.
- Final workspace regression (213 library tests in combined tree; 211 without T129), fmt and Clippy PASS on both trees. Native selection/pixels/lifecycle matrix PASS on both, including visible highlight removal after actual PTY output. Desktop acceptance remains NOT RUN.
- Deliberate limitation: selection is cleared by new output rather than maintained through reflow/scroll mutations. This prevents stale row/column anchors from copying unrelated text; stable history interaction remains TASK-159.

## TASK-145

- Dependencies satisfied. Runtime binds one data device to the selected seat and manager, with monotonic device identity. Seat/global removal releases the device and clears pending serial state; stale objects cannot change current state. Both actual keyboard press and pointer press serials are recorded against the originating seat/Tab; focus/tab changes invalidate them.
- Requires wl_data_device_manager v2 or newer so the device has an explicit release request. v1/missing manager leaves Clipboard unavailable without stopping the terminal. Manager itself has no protocol destructor. Incoming offers are explicitly destroyed until TASK-148 connects offer ownership; Copy/Paste remain later tasks.
- `runtime_data_device_tracks_seat_lifecycle`, full workspace, fmt and Clippy: PASS in combined and standalone staged trees. Native owned-headless matrix confirms exactly one device binding and existing pointer/selection/output/reconnect behavior on both trees.
- The first incremental rustc run crashed while serializing its compiler cache. Retried with `CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`; all final checks above passed. CPU affinity remained 0–1 and build jobs=2. No system settings changed. Desktop clipboard interoperability is NOT RUN.

## TASK-146

- Dependencies satisfied. Configured Copy validates the active focus, current selection, size limit and one fresh seat serial before creating a real data source. It advertises the two planned text MIME types and publishes the selection. No selection leaves the external Clipboard untouched. Replaced/cancelled sources release only their owned payload; stale source callbacks cannot revoke a newer source.
- Found and reproduced extra tab-filler spaces in the old extractor. Runtime Copy now queries current custom tab destinations through the core's public forward-tab handler, restores the cursor before extraction, and suppresses only blank filler. Selected trailing spaces, combining-on-space, CJK, hard/soft wraps, partial tab spans and right-edge clamping are tested. Grid, cursor, pending wrap and content epoch remain unchanged; the query adds only harmless damage metadata. Historical tab-stop settings are not stored by the core.
- `runtime_copy_publishes_selected_text_only` and `terminal_copy_preserves_tabstops_and_selected_spaces`: FAIL before implementation/fix, then PASS. Full workspace, fmt and Clippy PASS in combined and T129-free trees. Native `wl-copy`/`wl-paste --list-types` probe PASS on both: existing owner preserved without selection, exact MIME publication, and external replacement cancellation.
- FD delivery is deliberately TASK-147; this commit validates source publication, not end-to-end Copy transfer. Actual Firefox/Kitty desktop interoperability remains NOT RUN.

## TASK-147

- Dependencies satisfied. Source Send events validate the source object/MIME, own the supplied FD, preserve existing descriptor flags while enabling O_NONBLOCK, and enqueue a bounded transfer. The main poll loop registers writable descriptors and pumps each ready job once; no callback performs a blocking write.
- At most four transfers, each bounded by the validated Clipboard payload cap; one dispatch writes at most 4 × 16 KiB. Short writes, EINTR/EAGAIN, EPIPE and stale readiness retain/drop only the appropriate job. Source replacement/cancellation and seat/global cleanup cancel owned transfers and close their FDs. No payload logging or new dependencies/unsafe code.
- `runtime_clipboard_send_resumes_partial_writes` initially failed, then PASS. Additional real nonblocking-pipe and dispatch-budget tests PASS; they verify full-pipe backpressure, resumption, exact UTF-8 and EOF after completion. Full tests (219 combined / 217 standalone library tests), fmt and Clippy PASS. Read-only review found no actionable issue.
- Native `PTYRA_TEST_COPY_TRANSFER=1` matrix PASS on both combined and T129-free trees: wl-paste receives the exact selected CJK bytes for both advertised MIME types; replacement/cancellation and terminal input/reconnect regressions pass. Firefox/Kitty desktop and native slow-reader acceptance remain NOT RUN.

## TASK-148

- Dependencies satisfied. Real DataOffer/MIME/Selection events now populate a bounded object-to-offer identity book. The exact advertised text MIME is selected by the existing preference policy. Non-text offers remain non-text; stale/unknown/duplicate selection events cannot become a newer owner. Replacement, selection None, seat/global removal and keyboard focus loss destroy retired offers. Drag-and-drop offers never become Clipboard selections.
- Four live offer records maximum, with the existing 64-format / 256-byte MIME limits. Rejected MIME announcements retain a cancelled identity until Selection clears the old owner; capacity/identity failure clears external offers fail-closed. Focus loss clears external offers but preserves local Copy sources and outgoing transfers. Local source cancellation cannot clear a newer external owner.
- `runtime_clipboard_offers_ignore_stale_objects`, rejected-replacement and focus/local-owner regressions PASS. Review found the stale-owner-on-rejection and missing focus-loss cleanup cases; the rejected-offer regression first failed, both issues were fixed, and re-review found no remaining issue. Full workspace (222 combined / 220 standalone library tests, 13 integration tests, 2 doc tests), fmt and Clippy PASS. One pre-existing opt-in integration test remains ignored.
- Full native owned-headless-Sway matrix PASS on both combined and T129-free trees: text/non-text replacement, None, focus-loss cleanup/reacquisition, exact Unicode Copy transfer after focus loss, selection/pixels/output invalidation, hidden-output/tab-title/resize/click/exit and keyboard/reconnect behavior. Standalone separate CloseTab matrix PASS. CPU affinity 0–1, Cargo jobs=2, incremental compilation disabled as recorded above.
- Receive/Paste remains TASK-149 onward. Actual desktop Sway/Fcitx5/Firefox/Kitty acceptance is NOT RUN. T129 implementation remains uncommitted; no push or system configuration changes.

## TASK-149

- Dependencies satisfied. Paste now requests the exact preferred MIME from the selected native offer, sends a CLOEXEC pipe writer, closes the local writer, and registers the nonblocking reader in the existing poll loop. One receive job at a time, monotonic readiness identity, at most 16 KiB/four interrupted attempts per dispatch. Validated config clipboard cap is applied; overflow and invalid UTF-8 reject the entire payload rather than truncate.
- Completion, failure, replacement, cancelled offers, focus/tab changes, seat/global removal and a new receive release the old FD/payload. Offer validity is checked even without read readiness. Errors contain fixed metadata only. No new dependencies or unsafe code.
- `runtime_clipboard_receive_is_bounded_and_cancellable`: initially FAIL, then PASS. Additional tests verify stale readiness, owner replacement/rejection, bounded dispatch work and real nonblocking pipes with split UTF-8, EOF and reader close. Workspace: 226 combined / 224 standalone library tests, 13 integration tests and 2 doc tests PASS; one existing opt-in integration ignored. fmt and Clippy PASS on both trees.
- Full owned-headless-Sway matrix PASS on both trees, including real receive of a 72,000-byte Unicode payload, configured 131,072-byte cap rejection, invalid UTF-8/non-text rejection and a later successful receive. Review found a combined TRACE/RECEIVE test-ordering issue; fixed and both flags PASS together on combined and standalone trees. No production-code review findings.
- This task receives but deliberately does not deliver text to the PTY; delivery and multiline confirmation are TASK-150/151. Native stalled-transfer owner-replacement coverage and real Firefox/Kitty desktop acceptance are NOT RUN; deterministic cancellation and normal native lifecycle tests passed. T129 remains preserved/uncommitted. CPU affinity 0–1, Cargo jobs=2, no push/system changes.

## TASK-150

- Dependencies satisfied. The configured Paste action captures the exact Tab/window and keyboard focus epoch before receiving. Completion validates that original live target, snapshots its bracketed-paste mode once, and moves the unchanged bytes with at most one framing pair into that PTY's owned FIFO segment. No normalization, truncation or extra newline. Empty data and stale focus round trips are deterministic.
- The PTY reserves the paste's FIFO position at request time, so even keys received before clipboard EOF cannot overtake it. Existing ordinary input stays before it, subsequent input stays after it, and partial writes/EINTR/EAGAIN preserve progress. One moved payload per session (maximum configured Clipboard limit plus 12 framing bytes); ordinary queued input remains capped at 64 KiB. New paste requests are rejected while that target already has a paste, avoiding replacement of in-flight bytes.
- Waiting receive/confirmation reservations do not request POLLOUT, preventing writable busy loops. Receive failure, stale target, cancelled FD and empty completion release an unfilled reservation and preserve later ordinary keys on their original PTY. Review identified the pre-EOF ordering gap before commit; it was fixed and re-reviewed without remaining findings. Tests include a partial paste interrupted after bytes were written and empty-reservation completion.
- `runtime_paste_preserves_payload_and_target` and three FIFO/limit/reservation tests PASS; initial owned-queue regression first FAILed before implementation. Full workspace: 230 combined / 228 standalone library tests, 13 integration tests, 2 doctests PASS; one existing opt-in test ignored. fmt and Clippy PASS on both trees.
- Full native owned-headless-Sway matrix PASS on both final production trees, including exact raw PTY capture of 72,000 Unicode bytes plus one framing pair, a slow reader, a key sent immediately after the Paste shortcut, nonbracketed literal recovery and cap/UTF-8 rejection. All existing Clipboard/selection/tab/keyboard tests also PASS. No clipboard payload logs.
- Unbracketed multiline data is held without any paste bytes being sent; native confirmation UI is TASK-151. Cancellation after bytes enter the PTY queue is TASK-152; until then that queued paste finishes on its original PTY, never another Tab. Desktop interoperability remains NOT RUN. T129 edits preserved/uncommitted, CPU affinity 0–1, Cargo jobs=2, no push/system changes.

## TASK-151

- Dependencies satisfied. Unbracketed CR/LF payloads remain withheld behind the existing FIFO reservation. A native red warning strip displays only fixed metadata: “Multiline paste may run commands. Enter: paste Esc: cancel”. Normal typing is consumed while it is pending, and key repeat is stopped. Enter accepts only the exact current focus/instance token already submitted for presentation, with enough current geometry to display the entire warning; Escape cancels even when too small. No bracketed-paste sandbox claim.
- Focus loss/tab invalidation cancels the pending payload/reservation; stale or repeated decisions cannot approve a different instance. The prompt is redrawn/removed without modifying terminal cells. Pointer interactions over content are gated while the modal prompt is active; tab-strip navigation still follows existing routing.
- `runtime_multiline_paste_waits_for_user_decision`: initially FAIL, then PASS. Prompt fit tests PASS. Review reproduced a held-selection/release-inside-prompt bug: motion after Escape extended a phantom drag. The new native regression failed before the fix; cancelling the drag on confirmation entry fixed it. Enter also rechecks fit after resize. Re-review found no remaining actionable issue.
- Final full workspace: 232 combined / 230 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in test ignored. fmt and Clippy PASS on both trees. Full native owned-headless-Sway matrix PASS on both final trees: visible warning pixels, zero bytes before explicit decision, exact accepted text, Escape/focus cancellation, ordinary-key suppression, no phantom drag, plus all previous receive/copy/selection/tab/keyboard regressions.
- `/tmp/ptyra-t151-prompt.png` visually inspected: complete legible warning and keyboard actions, no Clipboard preview. Desktop Firefox/Kitty acceptance and native early/held decision or tab-switch-specific prompt cases are NOT RUN; token/epoch/fit logic has deterministic coverage. Cancellation after the payload is queue-owned remains TASK-152. T129 unchanged/uncommitted; no push/system changes; CPU affinity 0–1 and Cargo jobs=2.


## TASK-152

- Dependencies satisfied. Paste retains its original focus epoch after ownership moves to the PTY queue. Focus loss, Tab changes and keyboard removal cancel receive/confirmation and undelivered queued payload immediately. Closing a target drops its queue without routing to a successor.
- Explicit bracketed-mode metadata distinguishes payload from framing. Cancellation before any write sends nothing; after a partial write, only the incomplete start marker and/or remaining end marker finish on the original live PTY, before ordinary queued keys. Repeated cancellation preserves this cleanup. Already written bytes cannot be retracted; no rollback is claimed.
- `runtime_partial_paste_cancel_never_crosses_tabs`: first reproduced queued-paste non-cancellation, then PASS for every byte boundary of framed/unframed Unicode payload, repeated cancellation, closed target and unfilled receive/confirmation barrier.
- Workspace regression: 233 library tests, 13 integration tests, 2 doc tests PASS. Standalone staged tree excluding T129: 231 library tests, same integration/doc tests PASS. One pre-existing opt-in integration test remains ignored. Format and Clippy `-D warnings`: PASS on both trees.
- Owned headless Sway: `PTYRA_TEST_CLIPBOARD_RECEIVE=1 PTYRA_TEST_PASTE_CANCEL=1` PASS on both binaries. Gated raw readers force partial 120,000-byte paste; focus loss and Tab creation drop its unsent suffix, close only its original framing, preserve FIFO tail, and leave a new Shell's probe uncontaminated. Native multiline accept/Escape/focus cancellation also PASS.
- Full clipboard/selection/multi-tab native matrix PASS on both binaries, including MIME/FD send and receive, Unicode, original-target routing, hidden output, title, resize and child exit. Read-only review found no correctness/security defects. No Clipboard/key/PTY payload diagnostics added.
- CPUs 0–1; Cargo jobs=2. T129 independent files byte-compared unchanged and excluded. Actual desktop Firefox/Kitty/Fcitx5 acceptance: NOT RUN. No push or system-setting changes.


## TASK-156 (independent dependency-satisfied task)

- Dependencies TASK-029/TASK-030 satisfied. Terminal exposes bounded viewport scrolling through the core API, including delta/page/top/bottom operations and a changed-offset result. Extreme signed deltas clamp before core arithmetic; no Shell cursor, terminal cells, content epoch, or input bytes are changed by scrolling.
- `terminal_viewport_scroll_clamps_to_history`: first failed against a no-op stub, then PASS. Covers history bounds, signed extremes, zero history, alternate screen/main offset restoration, oldest-row eviction, resize, stopped core and unchanged cell/cursor/input state. A test-only raw Grid equality assumption was corrected to compare logical cells because the upstream ring-buffer equality requires normalized storage.
- Full workspace: 234 library + 13 integration + 2 doc tests PASS; clean standalone tree excluding T129: 232 library + same integration/doc tests PASS. One existing opt-in integration test ignored. Format and Clippy `-D warnings` PASS on both trees. Independent read-only review found no issues.
- Pure adapter-only API: native wheel/bindings, history pixels and history selection are not claimed here; TASK-157–159 remain unchecked. Native/desktop viewport acceptance NOT RUN because this task adds no runtime input path. CPU affinity 0–1, Cargo jobs=2; T129 edits remain uncommitted. No push or system-setting changes.


## TASK-153

- Dependencies satisfied. Optional Primary Selection has independent native manager/device/source/offer state and serial storage. Completed drag (including unchanged release), double-click and triple-click publish selected text without consuming ordinary Copy serials or replacing Clipboard. Middle-click inside the focused terminal uses only Primary.
- Shared bounded send/receive pumps retain selection-tagged readiness identity. Paste captures selection kind and original focus/Tab, using the existing payload cap, exact UTF-8/bracketed bytes, multiline confirmation and partial-PTY cancellation. Missing protocol/offer never falls back to Clipboard.
- `runtime_primary_selection_never_uses_clipboard_offer`: first failed before the independent offer constructor, then PASS. Additional deterministic tests cover distinct serial consumption, missing device, completion-triggered publication and Primary focus-bound safety.
- Full workspace: 238 library + 13 integration + 2 doc tests PASS; clean standalone tree without T129: 236 library + same integration/doc tests PASS. One existing opt-in integration test ignored. Format, Clippy `-D warnings`, build and expanded full headless native matrix PASS on both binaries.
- Native Primary cases PASS on both binaries: exact Unicode/bracketed middle paste; independent Clipboard/Primary content; cap/nontext/invalid UTF-8 recovery; multiline accept/Escape/focus cancellation; automatic drag/word/line CJK publication and source retained after focus loss; real backpressured partial paste cancellation on focus/Tab change for both selections. Actual Sway `primary_selection disabled` removes the global and confirms zero fallback bytes while ordinary Copy/Paste remains working.
- A new publication assertion initially raced motion vs release/compositor ownership; its test-only wait now checks the actual published Primary bytes. Final reruns PASS. Coordinator reviewed the transport, routing, lifecycle and isolated staging with no correctness/security defects found.
- The pinned wayland-protocols `unstable` feature and corresponding policy assertion are shared prerequisites independently required by Primary zv1. Only those overlapping T129 lines and a T153 dependency explanation are included; all IME-specific implementation/docs remain uncommitted and preserved. No package/version/lockfile change, native dependency, app unsafe, push, install or global setting change.
- CPUs 0–1; Cargo jobs=2. Real desktop Firefox/Kitty/Fcitx5 acceptance and performance benchmarking: NOT RUN. Logs: `/tmp/ptyra-t153-{test,clippy,native,cancel,no-primary}.log` and matching `clean-*` logs.


## TASK-154

- Dependencies satisfied. Top-level TOML `osc52_write = true` is the only runtime opt-in; omitted/false stays denied, non-boolean values fail validation. Config applies to existing and newly created Tab cores. Explicit registry-policy revocation clears pending writes on every live core and disables future Tab writes; config file reload is not implemented, so file changes take effect at restart.
- A PTY data dispatch drains at most the latest bounded write per selection, preserving the source Tab and explicit Clipboard/Primary kind. Native publication uses the existing bounded source/send implementation. Only the currently keyboard-focused source Tab with a real, unused seat input serial may publish; no focus, missing protocol, consumed/stale serial or configured byte-cap violation discards the request immediately. Rejected output is never queued for a later keypress. One input serial is independently consumable once per selection; output does not manufacture or refresh it.
- OSC encoded input remains bounded by the existing 4 KiB parser envelope and decoded payload also obeys `clipboard_bytes`. Empty explicit OSC writes are allowed; ordinary Copy-without-selection remains unchanged. Read queries remain disabled even with write opt-in; no clipboard read/reply is performed. Diagnostics are fixed metadata, without payload text.
- Config regression first failed before implementation. `runtime_osc52_respects_explicit_write_policy` PASS, covering config default/opt-in, independent kinds/serials, one-shot consumption, wrong Tab/seat/focus, absent capability, cap rejection, empty writes, revocation of existing pending writes, new-Tab policy, and read denial.
- Final workspace: 239 combined / 237 standalone library tests, 13 integration tests and 2 doc tests PASS. One pre-existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone tree excludes all unaccepted T129 implementation.
- Owned headless-Sway OSC fixtures PASS on both binaries with enabled, omitted and missing-Primary configurations: startup denial before focus/input; exact Unicode Clipboard/Primary publication; no-input second-write denial; raw-PTY read-query denial; configured-cap rejection; hidden-source rejection; newly created Tab inherits policy; successful later recovery. Initial startup assertion was corrected to accept either pre-focus denial or no-input-serial denial, both legitimate orderings.
- Existing full native Clipboard/Primary/selection/tab matrix and separate partial-paste cancellation matrix PASS on both binaries. T129 README/raster/IME fixture files byte-compare unchanged; IME runtime/keyboard hunks excluded by isolated staging. No push, install, dependency/unsafe addition or user/global settings changed. CPU affinity 0–1, Cargo jobs=2.
- User deferred real desktop Sway/Fcitx5/Firefox/Kitty acceptance until home; that acceptance and all blocked descendants remain NOT RUN/unchecked. Logs `/tmp/ptyra-t154-*` record combined/clean workspace, OSC/native and cancellation results.
