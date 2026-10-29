# Ptyra

> **開發中 · 狀態核對：2026-10-29**

Ptyra 係一個開發中嘅 Rust Native Wayland Terminal；目前只有基礎 library，未有可用 GUI Terminal。

目標：低 RAM、低 Idle CPU、可靠 Copy & Paste、Fcitx5 中文輸入、GUI Tabs。Kitty Keyboard / Graphics 放喺後續獨立 TASK。

## Planning Files

- [`SCOPE.md`](SCOPE.md) — 定義產品、架構、安全、性能與驗收邊界。
- [`TASKS.md`](TASKS.md) — **124 個 Atomic TASK**，用 Checkbox 記錄狀態，設有 Dependencies、Unit Test、Done Criteria。
- [`AGENTS.md`](AGENTS.md) — Coding Agent 必須遵守嘅執行、測試、安全同批准規則。

## Status

- `TASKS.md` 係唯一 TASK 狀態來源。規格文件嘅原始 Planning Draft 字句唔代表目前實作進度；批准規則仍按 `AGENTS.md` 執行。
- 已有基礎型別、事件來源/reducer、測試用 effect recorders、clock/error、Wayland request/state models、座標/damage/frame models、Shell argv/environment 建立器、有界 XKB keymap 轉換、純邏輯 key-repeat scheduler，以及真正 Linux PTY spawn adapter。
- `src/main.rs` 只印出應用程式名稱。PTY spawn 仍係 library API；尚未有 Wayland 連線/dispatch、PTY output/write queues、terminal core、renderer、Clipboard、IME 或 GUI Tabs 接線。
- Unit/fake tests 驗證 library 行為，唔代表 Sway、Fcitx5、PTY 或日常 Terminal 驗收通過。完整產品仍須通過 `SCOPE.md` 嘅 Delivery Gates。
- 不會改動現有 Sway、Fcitx5、Mesa 或 Terminal 設定。

## 2026-10-29 Audit

開始本次工作時，以 `175b7db` 核對 source、測試同 TASK Done Criteria：TASK-001–011、014–020 符合所列基礎/純邏輯範圍，未發現需要撤回勾選嘅實證；其中 TASK-005 只係測試 recorder，TASK-009 只產生 requests，TASK-011 只阻止失焦後產生 key route，TASK-018 只處理已收到嘅 disconnect event，TASK-019/020 唔會啟動 child。

開始審核時，TASK-012 未有 keymap 實作；TASK-013 依賴 012。014–020 走獨立 dependency 路徑，因此之前完成佢哋唔代表已完成鍵盤輸入。TASK-012 完成時，TASK-013 仍未開始；之後使用者批准 TASK-013 實作，驗證紀錄見下。

本次重新執行：`cargo test --workspace --locked --offline` 通過 18 個 unit tests 及 1 個 compile-fail doc-test；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通過。Cargo jobs/test threads 同 CPU affinity 均限制為 2。真 Wayland/PTY/Fcitx5、Integration/Acceptance 同 Benchmark：**NOT RUN**。

後續接線仍需明確覆蓋 Wayland connection/dispatch、request 執行、event loop、glyph/buffer presentation 同 keyboard→PTY 路徑；不得單靠純邏輯 TASK 勾選宣稱產品已可用。

## TASK-012 Verification (2026-10-29)

```text
TASK: TASK-012
Status: DONE — implementation and final checks verified; completion status approved
Dependencies: satisfied (TASK-011)
Changed files: Cargo.toml, Cargo.lock, src/lib.rs, src/keymap.rs,
  src/keymap/tests.rs, tests/fixtures/greek.xkb, DEPENDENCIES.md, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline keymap_translates_modifiers
Result: PASS on final rerun; test-first failure recorded before implementation
Workspace tests: PASS on final rerun — 21 unit tests + 1 compile-fail doc-test
Format / Clippy: PASS on final rerun — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: offline libxkbcommon US/Greek fixture PASS (prior run);
  real Wayland/PTY/Fcitx5 integration/acceptance NOT RUN
Security / Privacy impact: bounded keymap parsing; no external includes or
  payload diagnostics; dependency review and lockfile in DEPENDENCIES.md
Known limitations: library conversion only; no runtime keymap-FD/Wayland dispatch,
  repeat timing, IME/Compose or terminal byte encoding; no usable GUI Terminal
Ready for [x]: YES
Approval: user authorized finishing TASK-012 and its completion status
Commit status at this checkpoint: uncommitted; no push
```

`kbvm` 0.2.0 requires Rust 1.95 or later (verified with 1.97.1). Tests preserve Greek/Unicode keysyms, physical key codes, Ctrl/Shift/Alt masks, layout group, press/release/repeat and per-key repeat capability. Invalid/oversized/include-bearing maps and excessive expressions fail with payload-free errors. See [dependency and input-boundary review](DEPENDENCIES.md).

Every Cargo invocation used `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2` and `taskset -c 0,1`; builds stayed under `/tmp`. TASK-012 is checked after final verification and user approval. TASK-013 was not started under that approval; its later implementation was separately approved.

## TASK-013 Verification (2026-10-29)

```text
TASK: TASK-013
Status: DONE — implementation and final checks verified; completion status approved
Dependencies: satisfied (TASK-006, TASK-012)
Changed files for this task: src/repeat.rs, src/repeat/tests.rs, src/input.rs,
  src/keymap.rs, src/lib.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline key_repeat_stops_on_release
Result: PASS in main checkout; test-first failure recorded before implementation
Workspace tests: PASS in main checkout — 28 unit tests + 1 compile-fail doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland/PTY/Fcitx5 integration/acceptance NOT RUN
Security / Privacy impact: one held key/deadline per scheduler; checked arithmetic;
  no new dependencies, unsafe code, threads, I/O or key-payload logging
Known limitations: pure scheduler only; event-loop/Wayland/PTY/IME integration remains
  absent; no live compositor-repeat ownership or desktop acceptance validation
Ready for [x]: YES
Approval: user explicitly approved TASK-013 completion status after verification
Commit status at this checkpoint: uncommitted; no push; TASK-013 checked
```

Seven deterministic tests cover initial delay/interval, matching release (before and after
repetition), focus leave/re-enter even between polls, Tab/window changes, keyboard removal,
keymap replacement, independent keyboards, stale release/wakeup rejection, last repeatable
key priority, modifier changes, disabled/invalid settings, time overflow and late wakeups.
Releasing the winning key does not revive older held keys. A duplicate press does not
postpone its deadline; non-repeatable keys and repeated events do not arm a new hold.

`RepeatScheduler` defaults to disabled. Valid nonzero rates are 1–1000 Hz with delays
0–60000 ms; invalid configuration disables repeat with a metadata-only error. Rate zero
disables client repeat (its nonnegative delay is ignored). Configuration changes cancel
the current hold. Intervals round up to nanoseconds. A poll emits at most one event and
schedules the next interval from the current clock; missed intervals are never replayed.
Time overflow cancels the pending hold instead of leaving an immediately due timer.

Focus and keymap lifetime tokens prevent old input from matching a new focus/map, including
returning to the same Tab. Each repeat is translated using the current modifier masks and
layout group. The future event-loop owner must keep one scheduler, focus and keymap per
keyboard, feed events in dispatch order, process releases/focus changes before polling,
query `deadline` before waiting, and call `cancel` when another input handler consumes the
hold. Enter must not synthesize presses for already-held keys. With compositor-owned
repetition, disable client repeat and forward compositor repeats only once. See the
[Wayland keyboard protocol](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_keyboard).
These are adapter obligations, not claims of completed runtime integration.

Final Cargo checks used `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, `taskset -c 0,1`
and `CARGO_TARGET_DIR=/tmp/ptyra-task012-target`. Existing focus/routing/keymap regressions
passed; real Clipboard/IME/PTY regressions do not yet exist and were not substituted with
fake platform claims. The TASK-012 report above records its earlier validation checkpoint. At TASK-013 status
approval, all source, test and manifest files still matched the final tested snapshot;
only TASKS.md and this completion record were updated. The checks above are the previous
validation results; no Cargo build/test/fmt/Clippy was rerun for the status-only update.

## TASK-014 Closeout and T12–T14 Final Checks (2026-10-29)

```text
TASK: TASK-014
Status: DONE — existing implementation reviewed; closeout approved
Dependencies: satisfied (TASK-010)
Changed files for this closeout: README.md only; no T14 implementation/status changes
Unit test: cargo test --workspace --locked --offline pointer_coordinates_are_scaled
Result: PASS — 1 targeted test, 27 filtered out
Workspace tests: PASS — 28 unit tests + 1 compile-fail doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland/PTY/Fcitx5 integration/acceptance NOT RUN
Security / Privacy impact: no new code or dependencies in this closeout
Known limitations: pointer state/coordinate model only; live pointer dispatch,
  fractional scaling, terminal hit-testing and selection integration remain absent
Ready for [x]: YES — existing checked state retained after review and user approval
Commit: user authorized a local T12–T14 closeout commit; no push
```

These final checks ran in the main checkout from 05:33:57 to 05:33:58 UTC on 2026-10-29,
using the cached build under `/tmp/ptyra-task012-target`. They were sequential with
`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2` and `taskset -c 0,1`. No source changed after
the checks. The earlier verification sections are historical checkpoints.

T14 already preserves Window, surface-local position, scaled buffer position, button code
and pressed/released state. Its test covers motion, focus leave/clear, unrelated Window
leave, negative coordinates, non-finite input and scale overflow. This satisfies its
listed model-level Done Criteria; it does not establish a working GUI mouse/selection path.

The scoped commit includes T12's bounded XKB translation, fixture, dependency review and
lockfile; T13's pure repeat scheduler and lifetime protection; approved task statuses and
these validation records. T14 needs no additional implementation to satisfy its existing
contract. No unrelated task implementation or system changes are included.

## How to Use

1. 讀 `SCOPE.md`。
2. 喺 `TASKS.md` 選一個 Dependencies 已完成嘅 `TASK-XXX`。
3. 按 `AGENTS.md` 建立 Unit Test 同最小實作。
4. Unit Test、Regression 同所需 Acceptance Checks 通過後，先把 `[ ]` 改為 `[x]`。

## TASK-021 Verification (2026-10-29)

```text
TASK: TASK-021
Status: TODO — implementation verified; post-test checkbox approval pending
Dependencies: satisfied (TASK-005, TASK-019, TASK-020)
Changed files: Cargo.toml, Cargo.lock, src/lib.rs, src/pty.rs, src/pty/tests.rs,
  tests/pty_spawn.rs, DEPENDENCIES.md, README.md
Unit test: cargo test --workspace --locked --offline pty_spawn_failure_is_isolated
Result: PASS; test-first failure recorded before implementation
Workspace tests: PASS — 30 unit tests + 3 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 3 bounded real Linux PTY fixtures PASS;
  real Wayland/Sway/Fcitx5, GUI terminal acceptance and benchmarks NOT RUN
Security / Privacy impact: bounded literal argv/cwd, metadata-only errors;
  per-session FD/child ownership, no system/global environment changes;
  dependency/license/advisory and unsafe-boundary review in DEPENDENCIES.md
Known limitations: explicit absolute executable/cwd; no default-shell resolver,
  output stream, write queue, runtime resize or child-exit Tab routing;
  synchronous direct-child cleanup may wait on kernel teardown;
  no recursive descendant cleanup or shell sandbox; no usable GUI Terminal
Ready for [x]: YES technically; separate completion-status approval still pending
Approval: user approved TASK-021 implementation and local commit;
  AGENTS.md section 4 step 8 requires requesting checkbox approval after checks
```

The PTY adapter returns a TabId-owned `PtySession` or a typed `PtySpawnError`.
A session owns its nonblocking CLOEXEC master and direct child; parent slave
handles close before return. Validation/open/spawn failures are isolated from
other sessions. The real fixtures verify controlling terminal and foreground
session setup, stdin/stdout/stderr TTYs, initial 31x97 dimensions, literal Unicode
and non-UTF-8 argv, cwd, TERM/COLORTERM policy, FD cleanup and child reaping.
The test helper reads fixture output only; TASK-022 remains unimplemented.

Test-first run: 2026-10-29 07:01:26.396–07:01:34.479 UTC (expected FAIL).
Final staged checks: 07:04:26.722–07:04:28.564 UTC (all PASS).
Final main-repository checks: 07:06:29.638–07:06:32.400 UTC (all PASS, same source).
Every build/test used Cargo jobs=2, test threads=2 and CPU affinity 0,1, inherited
by test children. No stress workload, system installation or app restart was used.
TASK-021's checkbox is unchanged pending the required post-test status approval.
