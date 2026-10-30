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
- 已有基礎型別、事件來源/reducer、測試用 effect recorders、clock/error、Wayland request/state models、座標/damage/frame models、Shell argv/environment 建立器、有界 XKB keymap 轉換、純邏輯 key-repeat scheduler，以及真正 Linux PTY spawn adapter、有界 output byte 讀取、per-session input byte queue、PTY row/column resize、單次 child exit → Tab event mapping，以及保留跨 chunk parser state 嘅 terminal core adapter 同 renderer 資料接口。
- `src/main.rs` 只印出應用程式名稱。PTY spawn/read/write/resize、terminal core 同 renderer 接口仍係 library API；尚未有 Wayland 連線/dispatch、keyboard/paste/IME 輸入接線、實際 renderer backend、Clipboard、IME 或 GUI Tabs 接線。
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
Status: DONE — implementation verified; post-test completion status approved
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
Ready for [x]: YES
Approval: user explicitly approved TASK-021 completion status after receiving
  its test results; checkbox updated before starting TASK-023
```

The PTY adapter returns a TabId-owned `PtySession` or a typed `PtySpawnError`.
A session owns its nonblocking CLOEXEC master and direct child; parent slave
handles close before return. Validation/open/spawn failures are isolated from
other sessions. The real fixtures verify controlling terminal and foreground
session setup, stdin/stdout/stderr TTYs, initial 31x97 dimensions, literal Unicode
and non-UTF-8 argv, cwd, TERM/COLORTERM policy, FD cleanup and child reaping.
At that TASK-021 checkpoint, the test helper read fixture output only; TASK-022
was not implemented. Its subsequent implementation is recorded below.

Test-first run: 2026-10-29 07:01:26.396–07:01:34.479 UTC (expected FAIL).
Final staged checks: 07:04:26.722–07:04:28.564 UTC (all PASS).
Final main-repository checks: 07:06:29.638–07:06:32.400 UTC (all PASS, same source).
Every build/test used Cargo jobs=2, test threads=2 and CPU affinity 0,1, inherited
by test children. No stress workload, system installation or app restart was used.
TASK-021's checkbox is now checked following explicit post-test status approval.

## TASK-022 Verification (2026-10-29)

```text
TASK: TASK-022
Status: DONE — implementation verified; post-test completion status approved
Dependencies: satisfied (TASK-021, checked after explicit post-test approval)
Changed files: src/pty.rs, src/pty/tests.rs, tests/pty_spawn.rs, README.md
Unit test: cargo test --workspace --locked --offline pty_read_preserves_byte_order
Result: PASS; test-first failure recorded before implementation
Workspace tests: PASS — 33 unit tests + 5 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 2 new real PTY read fixtures plus all 3 spawn fixtures PASS;
  real Wayland/Sway/Fcitx5, GUI terminal acceptance and benchmarks NOT RUN
Security / Privacy impact: bounded nonblocking reads; no payload logs, decoding,
  new dependencies, unsafe code, threads or system configuration changes
Known limitations: library read API only; no runtime readiness/event-loop wiring,
  write queue, terminal parser, dynamic resize or child-exit Tab events;
  per-tick fairness/flood scheduling remains a later task
Ready for [x]: YES
Approval: user explicitly approved TASK-021 and TASK-022 completion statuses
  after receiving test results; both checked before starting approved TASK-023
```

`PtySession::read_chunk` uses a caller-provided nonempty buffer without allocating
payload storage. Each call returns at most 16 KiB, preserving the exact byte order
received from the PTY master. It does not interpret UTF-8, ANSI, NUL, invalid UTF-8,
spaces or newlines. Existing kernel termios processing is unchanged; the fixed
read fixture disables output processing on its own PTY for predictable bytes.

Each result/error includes its TabId. `Data(length)` identifies only the valid
prefix of the caller buffer. A short read returns immediately. `WouldBlock`
requires waiting for readiness; after four interrupted attempts, `Interrupted`
returns control for later rescheduling. Neither status is EOF. Empty buffers are
rejected before I/O. Read-zero, Linux PTY EIO after slave closure, and explicitly
closed sessions yield sticky EOF. Other errors remain payload-free AppError
metadata and do not affect another session. EOF neither kills nor reaps the child;
bytes still in the master can be drained even after `try_wait` reports child exit.

The fake-reader tests split UTF-8/escape sequences, interleave EINTR/WouldBlock,
check bounded work and remaining bytes beyond the read cap, EOF latching, error
redaction and independent session state. New real PTY fixtures drain binary,
Unicode and escape bytes through a two-byte buffer after child exit, and verify
WouldBlock plus session isolation when another session is terminated. Fixtures
use bounded output/deadlines and only manage their own children.

Test-first run: 2026-10-29 07:27:48.564–07:27:50.348 UTC (expected FAIL).
Final staged checks: 07:28:55.005–07:28:56.974 UTC (all PASS).
Final main-repository checks: 07:30:56.143–07:30:59.110 UTC (all PASS, same source).
Every build/test used Cargo jobs=2, test threads=2 and CPU affinity 0,1, inherited
by test children. At the T22 implementation checkpoint, both completion statuses
were left pending. The user subsequently explicitly approved T21 and T22 after
receiving their results, so both checkboxes are now checked. TASK-023 implementation
was separately approved in that later request.

## TASK-023 Verification (2026-10-29)

```text
TASK: TASK-023
Status: DONE — implementation verified; post-test completion status approved
Dependencies: satisfied (TASK-021); T21/T22 checkboxes updated after explicit approval
Changed files: src/pty.rs, src/pty/write.rs, src/pty/write/tests.rs,
  tests/pty_spawn.rs, README.md; TASKS.md contains only approved T21/T22 status updates
Unit test: cargo test --workspace --locked --offline pty_write_handles_partial_writes
Result: PASS; test-first failure recorded before implementation
Workspace tests: PASS — 37 unit tests + 7 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 2 new real PTY write fixtures plus 5 prior fixtures PASS;
  real Wayland/Sway/Fcitx5, GUI terminal acceptance and benchmarks NOT RUN
Security / Privacy impact: per-session input cap and fallible allocation;
  metadata-only results/errors; no payload Debug/logging, new dependencies,
  unsafe code, threads or system configuration changes
Known limitations: library queue API only; no readiness/event-loop wiring,
  keyboard/paste/IME routing, terminal parser, dynamic resize or exit Tab events;
  producers must retain/retry rejected enqueue data without busy-polling
Ready for [x]: YES
Approval: user explicitly approved T23 completion status after its test results;
  checkbox updated before starting separately approved T24 implementation
```

`enqueue_write` atomically accepts literal bytes into that session's queue, with
a 64 KiB logical pending-byte limit. Exceeding remaining capacity returns QueueFull
without accepting a prefix or changing existing bytes. Allocation is attempted
before copying; failure returns metadata without partially accepting input. Empty
input is a no-op for an open session. No encoding or paste/newline transformation
is added. Kernel terminal processing is unchanged.

`flush_write` submits at most 16 KiB and makes at most four write attempts. A
successful partial write removes exactly that many leading bytes and immediately
returns Written with the remaining count. EINTR is retried within the attempt
budget; WouldBlock returns for writable readiness. Exhausted retries return
Interrupted. WriteZero and other errors return immediately with the queue intact.
Empty queues make no I/O call. This API does not use write_all or Write::flush and
does not contain an event loop. Callers must avoid busy-polling wait/error statuses.

All results/errors identify TabId and pending count, without input payloads.
The queue belongs to its PtySession and is not redirected to another Tab. After
explicit termination, enqueue/flush return Closed; unwritten bytes remain owned
until that session is dropped. No implicit replay, discard-on-error or descendant
process management is added.

Deterministic fake writers cover partial progress, appending after partial writes,
EINTR/WouldBlock, queue capacity and atomic rejection, reuse of drained storage,
byte/attempt bounds, WriteZero, error redaction and independent session state.
The real Linux fixture uses fixed commands with child-local raw/no-echo mode and
`/usr/bin/head -c 1024`; two sessions receive different 1,024-byte binary payloads
and return them exactly through bounded reads. A second fixture verifies pending
ownership and Closed errors after explicit termination. Partial-write/EINTR
patterns are forced by fake writers, not assumed from kernel scheduling. Fixtures
have output bounds/deadlines and only manage their own children.

Test-first run: 2026-10-29 07:39:42.018–07:39:44.030 UTC (expected FAIL).
Final staged checks: 07:41:17.249–07:41:19.599 UTC (all PASS).
Final main-repository checks: 07:43:07.444–07:43:10.599 UTC (all PASS, same source).
All build/test commands used Cargo jobs=2, test threads=2 and CPU affinity 0,1,
inherited by test children. T23's checkbox is now checked after explicit post-test
approval. At that implementation checkpoint T24 was analysis only; its subsequent
implementation is recorded below.

## TASK-024 Verification (2026-10-29)

```text
TASK: TASK-024
Status: DONE — final acceptance PASS; explicitly approved completion status applied
Dependencies: satisfied (TASK-021); verified T23 status approved and checked
Changed files: src/pty.rs, src/pty/tests.rs, tests/pty_spawn.rs,
  DEPENDENCIES.md, README.md, TASKS.md (approved T23/T24 statuses)
Unit test: cargo test --workspace --locked --offline pty_resize_sends_correct_dimensions
Result: PASS in final main-repository run; test-first failure recorded before implementation
Workspace tests: PASS in final main-repository run — 40 unit + 8 Linux PTY integration + 1 doc-test
Format / Clippy: PASS in final main-repository run — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: new real PTY dimensions/SIGWINCH fixture PASS;
  all 7 previous PTY fixtures PASS; Wayland/Sway/Fcitx5, GUI acceptance,
  and benchmarks NOT RUN
Security / Privacy impact: only owned PTY descriptors; metadata-only errors;
  shared ioctl safety boundary documented; no new dependencies, threads,
  global settings or unrelated process signals
Known limitations: explicit row/column library API; no GUI pixel-to-cell or
  event-loop routing; cache tracks last adapter-applied size, not child changes
Ready for [x]: YES
Approval: user explicitly approved T23 and T24 checkboxes and the scoped commit;
  T24 completion was applied only after all final main-repository checks passed
```

`PtySession::resize` rejects zero rows/columns and returns a typed Unchanged result
without I/O for an identical size. A changed valid size uses TIOCSWINSZ on the
session's owned master; success updates the cache and returns Resized with TabId
and dimensions. A failed ioctl leaves the cache unchanged and can be retried
explicitly. There is no internal retry loop. Terminated sessions return
NotConnected even if the requested dimensions match their cached size.

The cache begins with the initial spawn size. Initial and runtime sizing share
the same initialized winsize/FD helper, with the safety boundary recorded in
DEPENDENCIES.md. No manual SIGWINCH, pixel calculation or GUI resize is added.

Deterministic tests cover exact row/column requests, no-op suppression, zero and
u16 boundary values, failure/retry, closed sessions, payload-free errors and
session isolation. The new real Linux fixture installs a SIGWINCH trap before
signalling readiness, then reports the size observed by the child. The two
sessions independently observe 42x109 and 17x53, with the second remaining alive
and unaffected while the first resizes. Existing spawn/read/write tests regress
ownership, byte streams, pending queues and child cleanup.

Test-first run: 2026-10-29 07:53:14.554–07:53:16.604 UTC (expected FAIL).
Final staged checks: 07:54:08.301–07:54:10.612 UTC (all PASS).
Final main-repository checks: 07:56:42.877–07:56:46.266 UTC (all PASS, same source).
All build/test commands used Cargo jobs=2, test threads=2 and CPU affinity 0,1,
inherited by test children. T25 remains analysis only; no exit-event mapping is
implemented by this change.

## TASK-025 Verification (2026-10-29)

```text
TASK: TASK-025
Status: DONE — implementation verified; completion checkbox explicitly approved
Dependencies: satisfied (TASK-021, TASK-003)
Changed files: src/lib.rs, src/pty.rs, src/pty/tests.rs, tests/pty_spawn.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline pty_exit_does_not_exit_app
Result: PASS; test-first compilation failed on missing exit API before implementation
Workspace tests: PASS — 42 unit tests + 9 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: new real PTY exit/isolation/output fixture PASS;
  all prior PTY fixtures PASS; Sway/Fcitx5 and GUI acceptance NOT RUN
Security / Privacy impact: owned-child wait only; metadata-only errors;
  no new dependencies, unsafe code, threads, payload logs or system changes
Known limitations: library event mapping only; lifecycle wakeups/event-loop dispatch
  and Tab registry application remain unimplemented; no usable GUI Terminal
Ready for [x]: YES
Approval: user explicitly approved TASK-025 [x] and the scoped commit after test results
```

`PtySession::take_exit_event` checks its own child without blocking and returns
`AppEvent::PtyExited` once, retaining the session's `TabId` and `ExitStatus`.
The pure reducer preserves both in a Tab-scoped effect without stopping rendering.
Wait errors preserve retryability and expose only Tab/component/error-kind metadata.
Existing `try_wait` and `terminate` calls do not consume this event. Callers must
dispatch returned events; repeated calls do not replay them. A lifecycle wakeup
source must drive checks without busy-polling; this task adds no event loop.

Exit detection does not close the master, discard pending input, or mark read EOF.
Remaining output can still be drained. Deterministic tests cover running/exited
states, normal/signal statuses, duplicate suppression, per-session isolation,
rendering continuation and error retry/redaction. The real PTY fixture verifies
exit status 23, buffered output after exit, a second live child, events after
explicit termination and legacy waiting, and descriptor cleanup.

Build/test/Clippy used Cargo jobs=2, test threads=2, CPU affinity 0,1 and
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`. At completion approval, only the
checkbox and verification record changed; the passing source/tests remain unchanged.
Cargo checks were not rerun for this documentation-only status update. The user
authorized a scoped TASK-025 commit; no push was requested or performed.

## TASK-026 Verification (2026-10-29)

```text
TASK: TASK-026
Status: DONE — 實作及驗證完成；使用者已批准 checkbox 更新及 commit
Dependencies: satisfied (TASK-022 已 [x])
Changed files: Cargo.toml, Cargo.lock, src/lib.rs, src/terminal.rs,
  src/terminal/tests.rs, tests/pty_spawn.rs, DEPENDENCIES.md, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline term_feed_handles_split_utf8
Result: PASS；test-first compilation 因未有 Terminal API 而預期失敗
Workspace tests: PASS — 50 unit tests + 9 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 真 PTY 分段 output → core fixture PASS；
  Sway/Fcitx5、GUI acceptance 同 Benchmark NOT RUN
Security / Privacy impact: TabId 檢查、feed/grid/OSC/combining 上限；
  metadata-only errors；OSC 52 同 Kitty keyboard 停用；冇新增 app unsafe/FFI、
  threads、network、payload logs 或系統設定改動
Known limitations: library API only；history 暫設 0；sync updates 即時套用；
  core 外部 events（包括 PTY replies）暫時丟棄；未有 runtime 接線或 GUI
Ready for [x]: YES
Approval: 使用者批准 T026 實作及最小 OSC/combining 防護，並授權已批准 TASK
  通過檢查後自行標記 [x] 同 commit。TASK-026 已 [x]；TASK-099 保持 [ ]。
  未要求 push。
```

`Terminal` 為每個 Tab 保存獨立 `Term` 同持續存在嘅 VTE parser。
`feed` 接收 `TabId` 同原始 bytes，唔逐 chunk 轉字串。Caller 只可以餵入
PTY `Data(n)` 嘅有效 buffer prefix；WouldBlock／Interrupted／EOF 唔會產生
新文字，亦唔應重建 parser。Child exit 同 PTY EOF 仍係原有獨立事件。
`grid` 借用 core grid，唔複製畫面，亦唔開放 mutable core。

Deterministic tests 涵蓋每個 UTF-8／ANSI 切分點、逐 byte 輸入、空 chunk、
未完成 sequence、invalid UTF-8、明確 cells/colors/cursor 預期、Tab 隔離、
錯 target、尺寸／feed 上限、OSC 跨 chunk 上限及 terminators、ESC 後全部
256 種 byte 嘅 VTE OSC entry 核對、combining／wide spacer／wrap／REP 上限，
以及停用 Kitty 同即時 sync-update 行為。

既有真 PTY fixture 額外以兩-byte buffer 餵入 adapter，驗證 child 已退出後
仍可 drain UTF-8／ANSI bytes，並核對 replacement character、繁體中文同紅色 cell。
原有 PTY ownership、spawn/read/write/resize/exit regression 全部保留。

固定邊界：每次 feed 16 KiB、每 screen 32,768 cells、OSC 4,096 bytes、
每 cell 64 個 combining marks。錯 target／過大 call 會原子拒絕，保留原狀。
Stream OSC／combining 超額會回傳 InvalidData，釋放該 core；後續 feed
回傳 NotConnected，`grid` 回傳 None。Caller 必須處理停止狀態；唔可重試或
改送另一個 Tab。Adapter 本身唔殺 Shell。暫存配置同 source 邊界詳見
[DEPENDENCIES.md](DEPENDENCIES.md#dependency-review--task-026-2026-10-29)。

T026 完成時未實作 T027 mode API、T029 scrollback policy、T030 renderer snapshot、
Clipboard routing、IME、GUI Tabs 或 runtime event loop。T099 保持未完成；
本次最小 OSC 防護唔代表其完整 rejection/recovery policy 已驗收。
`TERM=dumb` 同現有環境策略保持不變。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1，
以及 `CARGO_TARGET_DIR=/tmp/ptyra-task012-target`。Checks 全部以 locked/offline
方式執行；只有 dependency 下載及 metadata review 使用網絡。

完成批准時只更新 checkbox 同驗證紀錄，source/tests 同上次通過檢查嘅版本一致，
因此冇為呢次文件更新重跑 Cargo checks。

## TASK-027 Verification (2026-10-29)

```text
TASK: TASK-027
Status: DONE
Dependencies: satisfied (TASK-026 已 [x])
Changed files: src/terminal.rs, src/terminal/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline terminal_modes_track_sequences
Result: PASS；test-first compilation 因未有 Terminal::mode 而預期失敗
Workspace tests: PASS — 52 unit tests + 9 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  Sway/Fcitx5、GUI acceptance 同 Benchmark NOT RUN
Security / Privacy impact: 唯讀查詢；冇新增 dependency、unsafe、I/O 或 payload logs
Known limitations: mode flags 反映 core state；未有 Clipboard/render runtime 接線，
  亦唔代表 Bracketed Paste 安全策略或整體 Terminal 功能已完成
Ready for [x]: YES
Approval: 使用者批准下一個 TASK，並授權通過檢查後自行標記 [x] 同 commit。
  TASK-027 已 [x]；未要求 push。
```

`Terminal::mode` 借用 core 現有 `TermMode`，供 caller 查詢 `ALT_SCREEN` 同
`BRACKETED_PASTE`。冇額外 mode cache；ANSI 解讀繼續由 core 負責。
Adapter 因資源上限停止後回傳 None，唔會將「無 core」誤報為兩個 mode 都關閉。
呢個 API 只反映 terminal state，唔宣稱 app 已實作所有相關功能。

主測試以分段 bytes 驗證 DEC private mode 1049／2004 嘅開關、重複 set/reset、
合併 parameters、RIS reset、main/alternate grid 對應，同兩個 instances 互不污染。
另一測試驗證錯 TabId／過大 call 保留原有 modes，stream 超額停止後唔暴露舊 flags。
既有 Kitty 停用測試改用同一個公開唯讀 API。

本次冇改變 parser、PTY 或 Clipboard policy。Build/test/Clippy 使用 Cargo jobs=2、
test threads=2、CPU affinity 0,1 同 `CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；
所有 Cargo build checks 均使用 locked/offline。

## TASK-028 Verification (2026-10-29)

```text
TASK: TASK-028
Status: DONE
Dependencies: satisfied (TASK-026 已 [x])
Changed files: src/terminal/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline cjk_combining_cells_align
Result: PASS；先加驗證測試，首次執行已通過現有 core，毋須 runtime 修正
Workspace tests: PASS — 55 unit tests + 9 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  字型繪製、Sway/Fcitx5 acceptance 同 Benchmark NOT RUN
Security / Privacy impact: 只新增測試及文件；冇改 runtime、dependencies 或系統設定
Known limitations: 驗證 core grid 嘅 cell width、combining attachment 同 cursor；
  唔代表 font fallback、glyph shaping 或 renderer pixel alignment 已完成
Ready for [x]: YES
Approval: 使用者批准下一個 TASK，並已授權通過檢查後自行標記 [x] 同 commit。
  TASK-028 已 [x]；未要求 push。
```

主測試用固定 column／width 預期驗證 ASCII、繁體中文、全形英文字／標點／空格，
以及四-byte CJK 字元。寬字佔兩格，spacer 保持獨立；combining marks 附加喺
原本 base cell，保留次序，同時唔推進 cursor。每個 UTF-8 byte 切分點都會重測。
測試冇呼叫同一套 width lookup 去生成預期結果。

另外兩個測試驗證右邊界：只剩一格時寬字移到下一行；剛好填滿一行時，
combining marks 保留喺當前 base cell，直到下一個有寬度字元先換行。
涵蓋 ASCII、CJK 同最小兩-column grid，並核對 wide/spacer/wrap flags。

T028 係現有 core 行為嘅驗證 TASK，新增 3 個 deterministic tests，冇新增 runtime code。
Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-029 Verification (2026-10-29)

```text
TASK: TASK-029
Status: DONE
Dependencies: satisfied (TASK-026 已 [x])
Changed files: src/terminal.rs, src/terminal/tests.rs, DEPENDENCIES.md, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline scrollback_discards_oldest_lines
Result: PASS；test-first compilation 因未有 with_scrollback API 而預期失敗
Workspace tests: PASS — 60 unit tests + 9 Linux PTY integration tests + 1 doc-test
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  GUI scrollback、Sway/Fcitx5 acceptance 同 PSS/RSS Benchmark NOT RUN
Security / Privacy impact: 每 Tab history 行數及總 cell slots 有上限；
  超額設定先拒絕；冇新增 dependency、unsafe、I/O、payload logs 或系統改動
Known limitations: library 層設定固定於建立時；未有 TOML config、scroll UI 或 runtime resize；
  cell slots 上限唔係實測 RAM budget
Ready for [x]: YES
Approval: 使用者明確批准 T029，並授權完成後標記 [x] 同 commit；未要求 push。
```

`Terminal::new` 保留原有 signature，預設保存 5,000 行 scrollback。
`Terminal::with_scrollback` 可以為個別 instance 指定 0–10,000 行；0 代表停用。
行數係 grid rows，包含 soft-wrapped rows。Core 保留最新行，超額只淘汰最舊行；
Unicode cells、combining marks 同 wrap flags 保持原樣。Alternate screen 唔保存 history。

兩個 screens、history 同最多 1,000 spare cache rows 合計限制為 2,000,000 cell slots。
較寬視窗可能需要較低 history 設定；超額回傳帶 TabId 嘅 InvalidInput，唔會自動縮減。
所有容量檢查都在配置 core 前完成。預算與 core cache 嘅 source review 見
[DEPENDENCIES.md](DEPENDENCIES.md#scrollback-allocation-review--task-029-2026-10-29)。

新增 5 個 deterministic tests：零／一／三行上限持續輸出 1,105 行並逐次核對次序；
預設 5,000 行上限；Unicode／soft-wrap 保存；Tab 隔離、alternate screen 同 reset；
容量邊界、極端值同 cache 預算檢查。現有 callers 使用原本 constructor，毋須改 signature。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-030 Verification (2026-10-29)

```text
TASK: TASK-030
Status: DONE
Dependencies: satisfied (TASK-010、TASK-026 已 [x])
Changed files: src/lib.rs, src/terminal.rs, src/renderer.rs,
  src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline renderer_accepts_terminal_snapshot
Result: PASS；test-first compilation 因未有 renderer 接口型別而預期失敗
Workspace tests: PASS — 63 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  renderer 真畫面、Sway/Fcitx5 acceptance 同 Benchmark NOT RUN
Security / Privacy impact: snapshot 借用有界 core state，唔複製 grid/history；
  payload 型別冇 Debug；errors 使用 metadata-only AppError；
  冇新增 dependency、unsafe、I/O、threads、logs 或系統改動
Known limitations: 同步借用式 library 接口；未有 font、draw commands、
  backend buffer、presentation 或 runtime 接線；Preedit overlay 尚未實作／驗證
Ready for [x]: YES
Approval: 使用者明確指定 T030，並授權完成後標記 [x] 同 commit；未要求 push。
```

`Terminal::snapshot` 連同 TabId、rows、columns 提供 core 原有 `RenderableContent`。
Snapshot 保留 visible-cell iterator、顏色、cursor、mode 同 display offset；
正常迭代只包含目前 viewport，唔遍歷整份 scrollback。借用期間唔可以修改 terminal；
adapter 停止後回傳 None。冇額外 grid/history allocation，亦冇開放 mutable core。

`RenderInput` 保存 WindowId、已確認嘅 logical window size 同 terminal snapshot。
`Renderer` 接收呢份資料，回傳 `RenderOutput<Frame>` 或 `AppError`。
Output 包含 WindowId、TabId、logical size 同 backend 自定嘅 frame data。
接口唔選定 SHM／GPU backend；實作者只準備 frame，PTY／Wayland I/O 同提交畫面
由外部 adapter 負責。現有 production callers、config 同 PTY fixtures 毋須改動。

Fake renderer 驗證 configure size、可見 cells 次序、CJK／combining marks、
soft-wrap flags、前景／背景、palette override、cursor 同 mode，並確認 grid 冇被改寫。
另外驗證 cell 借用 identity、main／alternate screen、Tab 隔離、stopped state，
以及 compile-fail borrow contract。既有 CJK、cursor、damage 同 frame regression 全部通過。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-031 Verification (2026-10-29)

```text
TASK: TASK-031
Status: DONE
Dependencies: satisfied (TASK-030、TASK-015 已 [x])
Changed files: src/renderer.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline cell_rect_rounding_is_stable
Result: PASS；test-first compilation 因未有 CellLayout 而預期失敗
Workspace tests: PASS — 66 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  renderer 真畫面、Sway/Fcitx5 acceptance 同 Benchmark NOT RUN
Security / Privacy impact: 尺寸先驗證，edges 先裁切再轉整數；
  無效輸入回傳 metadata-only renderer error；冇新增 allocation、dependency、
  unsafe、I/O、threads、payload logs 或系統改動
Known limitations: pure library geometry；integer scale、viewport 原點 (0,0)；
  caller 提供 cell advances，並先轉換 scrollback offset；
  font metrics、draw commands、Preedit overlay 同實際 backend 尚未實作／驗證
Ready for [x]: YES
Approval: 使用者授權下一個 TASK，並要求完成後標記 [x] 同 commit；
  已識別為 TASK-031。未要求 push。
```

`CellLayout::new` 重用 `WindowSize` 同 `Scale`，接收 logical window size 同
caller 提供嘅 logical cell width／height。縮放後每方向必須介乎 1 至 i32::MAX
physical pixels；NaN、infinity、非正值、過小／過大尺寸同 window scale overflow
都回傳 `Renderer: InvalidInput`。呢個 API 唔計算 font metrics。

`cell_rect` 將 zero-based viewport row／column 轉成 physical-pixel `Rect`。
每個 edge 獨立用同一公式 round-to-nearest，半像素向上取整，令相鄰 cells
共用邊界。邊緣 cell 裁切至 window；完全在外嘅 cell 回傳 None。
計算冇逐格累加或配置 buffer；極端 indices 亦先裁切，唔會造成整數 overflow。
Resize／scale 改變後由 caller 重新建立 layout；window resize 唔會拉伸 cell advances。

新增 3 個 deterministic tests，涵蓋明確半像素結果、scale 1–4、重複 resize、
相鄰邊界／完整覆蓋、window clipping、無效 floats、i32::MAX／usize::MAX，
以及真 terminal snapshot 嘅 CJK spacer 同 combining cell mapping。既有 snapshot、
CJK、cursor、damage、frame 同 PTY regression 全部通過。現有 callers、config、
exports 同 fixtures 毋須改動；T032 保持未完成。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-032 Verification (2026-10-29)

```text
TASK: TASK-032
Status: DONE
Dependencies: satisfied (TASK-031 已 [x])
Changed files: src/font.rs, src/lib.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline font_metrics_validate_dimensions
Result: PASS；test-first compilation 因未有 CellMetrics／FontMetrics 而預期失敗
Workspace tests: PASS — 70 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  真字型／hinting、renderer 真畫面、Sway/Fcitx5 acceptance 同 Benchmark NOT RUN
Security / Privacy impact: finite／正負值／結果範圍驗證；固定計算量、無 allocation；
  errors 只含 Renderer／InvalidInput；冇新增 dependency、unsafe、I/O、logs 或系統改動
Known limitations: pure library metrics；caller 提供所選 monospace face 嘅 em metrics；
  未有 font loading、字型是否等寬嘅偵測、hinting、rasterization 或 runtime 接線；
  i32::MAX 係座標上限，唔係未來 glyph buffer 嘅 allocation budget
Ready for [x]: YES
Approval: 使用者授權下一個 TASK，並要求完成後標記 [x] 同 commit；
  T032 中斷後繼續完成，冇跳到 T033。未要求 push。
```

`FontMetrics` 保存以 em 正規化嘅 fixed advance、ascent、descent 同 line gap。
Advance 必須正數；ascent 非負；descent 非正；line gap 非負；ascent／descent
不可同時為零。所有分量必須有限。`CellMetrics::new` 接收 logical pixels per em
嘅 font size，計出 `width = size × advance`、
`height = size × (ascent − descent + line_gap)` 同 `baseline = size × ascent`。
本接口將 line gap 全部放喺 descent 之後；baseline 從 cell 頂部量起。
高度公式及 advance 定義參考 [FreeType Glyph Metrics](https://freetype.org/freetype2/docs/glyphs/glyphs-3.html)。

Font size 同輸出 width／height 必須大於零、有限，且唔超過 i32::MAX。
保留 logical-pixel 小數；physical scale 同至少一個 physical pixel 嘅檢查仍由
`CellLayout` 負責。計算先縮放各個高度分量，再相加，避免有效細 pixel 結果被
未縮放 em 總和嘅 overflow 誤拒絕。零尺寸 underflow 同無界結果會回報錯誤。

新增 3 個 metrics tests，使用合成 face metrics 驗證整數／小數 font size、
baseline／line gap、非法 floats／符號、underflow、範圍邊界及大 em／細 size。
另有 1 個 `CellLayout` 接線測試，確認 scale 只套用一次、subpixel logical advance
由 physical scale 驗證，以及 scale 後超額仍拒絕。獨立 review 發現嘅 intermediate
overflow 已先重現測試失敗，再修正並通過全部 regression。原有 CJK、cursor、
damage 同 frame regression 全部保留；Preedit overlay 尚未實作／驗證。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-033 Verification (2026-10-29)

```text
TASK: TASK-033
Status: DONE
Dependencies: satisfied (TASK-032 已 [x])
Changed files: src/font.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline font_fallback_uses_cjk_candidate
Result: PASS；test-first compilation 因未有 select_font 而預期失敗
Workspace tests: PASS — 73 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  真字型 coverage／fallback 畫面、Sway/Fcitx5 acceptance 同 Benchmark NOT RUN
Security / Privacy impact: selector 無 allocation、無 cache；
  每次最多查詢 supplied slice 內每個 candidate 一次，命中即停止；
  冇新增 dependency、unsafe、I/O、payload logs 或系統改動
Known limitations: pure scalar 選擇 API；coverage callback 由 caller 提供；
  未有 font discovery/loading、真 glyph coverage 接線、sequence shaping 或 rasterization
Ready for [x]: YES
Approval: 使用者明確批准 T033，並授權完成後標記 [x] 同 commit；未要求 push。
```

`font::select_font` 接收一個 Unicode `char`、按優先順序排列嘅 candidate slice，
以及 `has_glyph` callback。Primary font 放在第一位；回傳第一個支援該 scalar
嘅 candidate index。空 list 或全部缺字回傳 None，唔會盲目揀第一個 font。
判斷依賴 coverage，唔依賴 font 名或 Unicode block；candidate 次序由 caller 決定。

Callback 必須查詢已載入嘅 coverage、排除 missing-glyph placeholder，並且唔做 I/O。
Selector 本身唔載入字型、唔改動輸入文字或 metrics，亦唔儲存含 PTY 字元嘅 cache。
組合序列嘅 font 選擇同 shaping 仍須 backend 處理；單個 scalar 嘅 coverage
唔代表整個 sequence 已可正確呈現。

新增 3 個 deterministic tests：fake coverage 驗證繁體字、非 BMP CJK、primary 優先、
candidate 次序、首個命中即停止、空 list／全缺字及重複呼叫；terminal snapshot
接線測試確認繁體字選 fallback，ASCII 選 primary，原本 wide spacer、cell geometry
同 baseline 保持原樣。既有 CJK／combining、cursor、damage、frame regression 全部通過；
Preedit overlay 尚未實作／驗證。獨立 read-only review 未發現實質問題。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-034 Verification (2026-10-29)

```text
TASK: TASK-034
Status: DONE
Dependencies: satisfied (TASK-032 已 [x])
Changed files: src/font.rs, src/font/cache.rs, src/font/cache/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline glyph_cache_evicts_by_limit
Result: PASS；test-first compilation 因未有 cache 型別而預期失敗
Workspace tests: PASS — 80 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  真 glyph/backend、GPU、PSS/RSS、cache hit-rate Benchmark 同 allocator failure injection NOT RUN
Security / Privacy impact: payload bytes + entry count 雙上限；空 glyph 亦計 entry；
  oversized insert 原子拒絕；無 glyph/key Debug、payload logs、dependency、unsafe 或平台 I/O
Known limitations: 純 CPU payload cache；FIFO 可以淘汰頻繁讀取嘅 glyph；
  caller 負責 glyph format、allocation budget 同 font-instance identity；未有 renderer 接線；
  payload budget 唔包括 metadata／allocator overhead，亦唔係 process RSS 上限
Ready for [x]: YES
Approval: 使用者明確批准 T034，並授權完成後標記 [x] 同 commit；未要求 push。
```

`font::cache::GlyphCache::new` 接收固定 payload byte budget 同 entry limit，
分別不可超過 16 MiB 同 4,096 entries；任何一個為零就停用 cache。
Cache 擁有 `Box<[u8]>`，按 slice 長度計算 payload bytes，避免 Vec spare capacity
漏計；glyph bytes 嘅格式由 caller 定義。Key 由 font-instance identity 同 glyph ID
組成，唔包含 TabId，因此一個 cache 可以供多個 Tabs 使用。Face、size、scale、style、
hinting 或 format 改變時，caller 必須更換 instance identity；唔可以只用 fallback index。

`get` 借用 payload，唔改動 FIFO。`insert` 超過 bytes／entry budget 時淘汰最舊項目；
replacement 先扣除舊 cost，並且只保留一個最新 queue entry。過大 payload、停用狀態，
或 metadata reservation 失敗時，回傳原本 Box，保留舊 cache，供 caller 直接使用 uncached
payload。Metadata 先 reserve，再進行 replacement／eviction；byte 比較用減法避免 overflow。
`clear` 釋放 glyph payload，同時保留 metadata storage 供重用。

預算限制係操作完成後 cache 持有嘅 payload bytes 同 entry count。Caller 已配置嘅
incoming payload 可以同舊 cache 同時存在；HashMap backing capacity 亦可能因 reserve
改變。實際 metadata／allocator footprint 同 rasterizer／GPU memory 要另行量度及限制。

新增 7 個 deterministic tests，涵蓋 byte／entry eviction、空 glyph、停用、exact limit、
多項淘汰、replacement cost／order、oversized 拒絕後嘅原 Box identity、font-instance
隔離、clear、無效／極端 budgets，以及 10,000 次插入後嘅 payload／entry／queue count。
獨立 review 未發現實質問題。既有 CJK／combining、cursor、damage、frame 同 PTY
regression 全部通過；Preedit overlay 尚未實作／驗證。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-035 Verification (2026-10-29)

```text
TASK: TASK-035
Status: DONE
Dependencies: satisfied (TASK-030、TASK-034 已 [x])
Changed files: src/renderer.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline draw_commands
  包括指定 draw_commands_preserve_colors 及兩個 Unicode／scrollback 測試
Result: PASS — 3 tests；test-first 指定測試因未有 draw_commands 而預期 compilation FAIL
Workspace tests: PASS — 83 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  真 renderer／font rasterization、Sway／Fcitx5 同 GPU／PSS／RSS Benchmark NOT RUN
Security / Privacy impact: lazy borrowed conversion 無額外 heap allocation；
  沿用有界 snapshot，checked scroll offset 同 clipped geometry；
  無 payload Debug／logs、新 dependency、unsafe 或平台 I/O
Known limitations: backend-neutral cell instructions；backend 須解讀 raw colors/styles、
  忽略 hidden/spacer glyph，先畫所有背景再畫 glyph；未有真 rasterization／cache 接線
Ready for [x]: YES
Approval: 使用者明確授權完成 T035 後標記 [x] 同 commit；未要求 push。
```

`renderer::draw_commands` 將 terminal snapshot 逐格轉成 `CellDrawCommand`。
Command 分別保留文字、原始前景／背景 `Color`、style flags 同 underline color。
Named、Indexed、RGB 同 reset 色唔互換；OSC palette overrides 同 combining marks
直接借用原資料。空格同 spacer 仍有背景 rectangle，避免丟失背景填色。

座標先加 display offset，再經現有 `CellLayout` 轉成 clipped physical pixels。
`rect` 係一格背景範圍；`glyph_rect` 對 WIDE_CHAR 合併相鄰兩格，保留 fractional
rounding 同 window clipping。Backend 負責 INVERSE、HIDDEN 同其他 style，亦須跳過
兩種 wide spacer 嘅 glyph；繪製時先完成全部背景，避免 spacer 背景遮蓋 CJK glyph。
Caller 保留 window／TabId routing；font 選擇、rasterization 同共享 GlyphCache 使用
仍由 backend 負責。今次無新增 cursor、overlay 或 dirty-frame scheduling。

新增 3 個 deterministic tests，涵蓋三種顏色、reset、空格背景、palette identity、
繁體中文、combining slice identity、跨行 wide spacer、style／underline color、
scrollback viewport offset 同右邊／底部 clipping。獨立 read-only review 未發現實質問題。
既有 CJK／combining、cursor snapshot、damage、frame 同 PTY regression 全部通過；
Preedit overlay 尚未實作／驗證。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-036 Verification (2026-10-29)

```text
TASK: TASK-036
Status: DONE
Dependencies: satisfied (TASK-035 已 [x])
Changed files: src/renderer.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline cursor_shape_matches_mode
Result: PASS；test-first 因未有 cursor_draw_command 而預期 compilation FAIL
Workspace tests: PASS — 87 unit tests + 9 Linux PTY integration tests + 2 compile-fail doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 既有 9 個真 PTY fixtures PASS；
  真 renderer／Sway cursor 呈現、HiDPI thickness calibration 同 Benchmark NOT RUN
Security / Privacy impact: 純 geometry conversion；無 heap allocation、payload logs、
  dependency、unsafe 或平台 I/O；沿用有界 snapshot、checked offset 同 clipped geometry
Known limitations: 只產生 cursor shape／rect；backend 負責顏色、block 下文字重繪及 outline；
  Beam／Underline 暫用 1 physical pixel；blink／focus scheduling 未實作；
  HollowBlock outline 尚需原始 cell 邊界再 clip，現有 command 只保留 clipped envelope
Ready for [x]: YES
Approval: 使用者明確授權完成 T036 後標記 [x] 同 commit；未要求 push。
```

`renderer::cursor_draw_command` 沿用 core 嘅 cursor shape 同已正規化嘅位置，
回傳 `CursorDrawCommand`。Block 覆蓋 cell；Beam 放左邊；Underline 放完整 cell
嘅最底一行 pixel。CJK spacer 嘅 cursor 由 core 移返 lead cell，Block／Underline
覆蓋兩格；combining sequence 維持一格。HollowBlock 保留 shape 供 backend 畫 outline。

Hidden cursor 回傳 None。座標先加 display offset，再檢查 grid viewport 同 window
bounds；即使 window 有額外空位，已移出 viewport 嘅 cursor 都唔會畫出。
Underline 先按未 clipped 嘅 cell bottom 定位，底線喺 window 外就省略，唔會移上
可見區域。Cursor cell lookup 最多掃描 snapshot 嘅 visible cells，唔複製 grid。

新增 4 個 deterministic tests，涵蓋 DECSCUSR 0–6、hide/show、非原點及 scale、
CJK／combining、fake scrollback、右邊／底部 clipping、1-pixel cells 同 i32::MAX
layout 邊界。既有文字／顏色、CJK、damage、frame 同 PTY regression 全部通過；
Preedit overlay 尚未實作／驗證。獨立 read-only review 確認三種指定 cursor shapes
無實質問題；HollowBlock 嘅 outline clipping 限制留待 backend 處理。

Build/test/Clippy 使用 Cargo jobs=2、test threads=2、CPU affinity 0,1 同
`CARGO_TARGET_DIR=/tmp/ptyra-task012-target`；所有 Cargo build checks 均使用 locked/offline。

## TASK-037 Verification (2026-10-30)

```text
TASK: TASK-037
Status: DONE
Dependencies: satisfied (TASK-035 已 [x])
Changed files: src/renderer.rs, src/renderer/overlay.rs,
  src/renderer/overlay/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline overlay_does_not_change_grid -- --test-threads=1
Result: PASS (1 test); overlay test group PASS (4 tests)
Workspace tests: PASS — 91 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: 9 existing real Linux PTY fixtures PASS;
  actual GUI/Wayland/Fcitx5 preedit rendering NOT RUN
Security / Privacy impact: borrowed preedit text is bounded to 4096 UTF-8 bytes,
  checked against TabId, and only returned as a render command; no PTY write,
  payload logging, dependency, unsafe, or platform I/O added
Known limitations: backend text shaping, painting, focus-epoch validation and
  real preedit display remain for later integration tasks
Ready for [x]: YES
Approval: user explicitly approved finishing TASK-037 and later requested its commit;
  no push requested
```

The independent overlay layer emits selection rectangles before a borrowed preedit
command. It clips viewport coordinates, covers a selected wide glyph once, rejects
wrong-tab or oversized preedit without truncation, and leaves the terminal grid,
cursor, palette and mode untouched. The current executable has no GUI renderer;
the tests validate render commands and state isolation, not visible output on Sway.

All build checks used `taskset -c 0,1` with one Cargo job and one test thread;
Cargo checks used `--locked --offline`.

## TASK-038 Verification (2026-10-30)

```text
TASK: TASK-038
Status: DONE
Dependencies: satisfied (TASK-017 and TASK-035 已 [x])
Changed files: src/damage.rs, src/frame.rs, src/renderer.rs,
  src/renderer/tests.rs, src/renderer/dirty.rs, src/renderer/dirty/tests.rs,
  README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline hidden_or_clean_view_skips_draw -- --test-threads=1
Result: PASS; test-first compilation failed before the dirty render API existed
Workspace tests: PASS — 94 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  real Wayland rendering, tab switch and idle CPU measurement NOT RUN
Security / Privacy impact: pure clipped geometry and metadata-only frame state;
  no dependencies, unsafe, PTY/Wayland I/O, payload logging or network access
Known limitations: no production renderer or Tab Manager wiring; caller must keep
  a scheduler per view, supply physical-pixel damage and scissor the backend
Ready for [x]: YES
Approval: user explicitly approved TASK-038, checkbox update and scoped commit;
  no push requested
```

`render_visible_dirty` skips clean or hidden views before invoking the renderer.
Hiding a view restores pending damage, so an absent frame callback cannot block
its next visible frame; a late callback is stale. Failed rendering also restores
damage. `dirty_draw_commands`, `dirty_cursor_draw_command` and
`dirty_overlay_commands` keep only geometry intersecting the physical-pixel
damage. They preserve original glyph and overlay coordinates, including a wide
glyph whose lead cell lies outside the dirty area; the backend must scissor to
the supplied damage rectangle rather than move the glyph origin.

Deterministic tests cover clean/hidden views, partial damage, 2x scale, wide
glyphs, cursor and preedit overlays, pending and cancelled frames, rendering
failure, and re-show after hidden updates. The current executable still has no
GUI renderer or Tab Manager; this verifies the render model, not live Sway output.

## TASK-039 Verification (2026-10-30)

```text
TASK: TASK-039
Status: DONE
Dependencies: satisfied (TASK-031 and TASK-014 已 [x])
Changed files: src/renderer.rs, src/renderer/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline hit_test_maps_cell_boundaries -- --test-threads=1
Result: PASS; test-first compilation failed before CellLayout::hit_test existed
Workspace tests: PASS — 95 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  live Wayland mouse/GUI hit testing NOT RUN
Security / Privacy impact: pure bounded-coordinate logic; no new dependency,
  unsafe, platform I/O, input logging or network access
Known limitations: caller translates surface pointer coordinates to grid-local
  physical pixels; Tab bar, selection routing and wide-glyph normalization
  belong to later tasks
Ready for [x]: YES
Approval: user explicitly approved TASK-039, checkbox update and scoped commit;
  no push requested
```

`CellLayout::hit_test` uses a binary search over the same rounded, clipped
`cell_rect` boundaries used for drawing. A point exactly on a shared edge maps
to the cell on its right/below; negative, outside-window, blank-margin and
zero-grid coordinates return None. The deterministic test covers a fake
`PointerClick.buffer`, 2x scale, fractional cell advances, exact edges,
resize, huge grid counts and invalid point/layout construction. It returns a
viewport row/column only; no Tab or mouse-drag state is changed.

## TASK-040 Verification (2026-10-30)

```text
TASK: TASK-040
Status: DONE
Dependencies: satisfied (TASK-039 已 [x])
Changed files: src/lib.rs, src/selection.rs, src/selection/tests.rs,
  README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline drag_selection_normalizes_direction -- --test-threads=1
Result: PASS; test-first compilation failed before DragSelection existed
Workspace tests: PASS — 97 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  live Wayland mouse/selection/GUI acceptance NOT RUN
Security / Privacy impact: per-Tab pure state with no selected text storage,
  platform I/O, unsafe, dependency, logging or network access
Known limitations: caller converts hit-tested viewport cells to grid points
  using current scroll offset; no mouse event/Tab Manager wiring, word selection,
  text extraction or Clipboard handling
Ready for [x]: YES
Approval: user explicitly approved TASK-040, checkbox update and scoped commit;
  no push requested
```

`DragSelection` stores one TabId, a drag anchor and current point. Forward and
reverse drags produce the same normalized `SelectionRange`, including across
lines and scrollback grid coordinates. Release finalizes the range; later motion
does nothing. Wrong-Tab events cannot change it. The caller cancels an unfinished
drag on focus/Tab change; a completed selection remains owned by its Tab.

## TASK-041 Verification (2026-10-30)

```text
TASK: TASK-041
Status: DONE
Dependencies: satisfied (TASK-040 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline double_click_selects_unicode_word -- --test-threads=1
Result: PASS; test-first compilation failed before double_click existed
Workspace tests: PASS — 99 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  real Wayland double-click and GUI selection NOT RUN
Security / Privacy impact: bounded, read-only grid scan with per-Tab state;
  no selected-text copying, logging, dependency, unsafe, I/O or network access
Known limitations: word selection stays within one grid row, even across a
  soft wrap; caller supplies grid coordinates, validates current Tab/focus and
  routes real double-click events in later integration tasks
Ready for [x]: YES
Approval: user explicitly approved TASK-041, checkbox update and scoped commit;
  no push requested
```

`DragSelection::double_click` returns and stores an inclusive `SelectionRange`.
ASCII letters/digits/underscore form one run. Other narrow Unicode
alphanumerics form a separate run, without language-specific segmentation.
Each wide glyph (including CJK and emoji), punctuation or whitespace selects
one cell. Combining marks remain attached to the base cell. Clicking a trailing
wide spacer selects its lead glyph; a wrap-leading placeholder returns None.
Invalid grid coordinates or a wrong Tab leave the existing selection unchanged.
Tests cover mixed ASCII/Chinese/Greek, combining marks, punctuation, blank
cells, spacers, wrap and row boundaries.

## TASK-042 Verification (2026-10-30)

```text
TASK: TASK-042
Status: DONE
Dependencies: satisfied (TASK-040 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline triple_click_selects_logical_line -- --test-threads=1
Result: PASS; test-first compilation failed before triple_click existed
Workspace tests: PASS — 101 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  real Wayland triple-click and GUI selection NOT RUN
Security / Privacy impact: bounded, read-only grid scan with per-Tab state;
  no selected-text copying, logging, dependency, unsafe, I/O or network access
Known limitations: caller supplies grid coordinates and routes real triple-click
  events; no mouse event-loop wiring or Clipboard text extraction yet
Ready for [x]: YES
Approval: user explicitly approved TASK-042, checkbox update and scoped commit;
  no push requested
```

`DragSelection::triple_click` selects the complete logical line. It expands
backward and forward through rows marked with `WRAPLINE`, stopping at hard
breaks and scrollback boundaries. Wrong-Tab and invalid grid points leave the
previous selection unchanged. Tests cover clicks on either wrapped row,
unwrapped lines, wrong-Tab/invalid points, and a wrap crossing the scrollback
boundary.

## TASK-043 Verification (2026-10-30)

```text
TASK: TASK-043
Status: DONE
Dependencies: satisfied (TASK-040, TASK-026 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline copy_soft_wrap_does_not_add_newline -- --test-threads=1
Result: PASS; test-first compilation failed before copy_logical_lines existed
Workspace tests: PASS — 103 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  real Wayland Clipboard, GUI Copy and Sway/Fcitx5 NOT RUN
Security / Privacy impact: pure grid read, no payload logging or I/O;
  copy rejects output above 16 MiB instead of silently truncating
Known limitations: returns separate logical-line fragments; TASK-044 will
  insert hard-break newlines. Caller must route the owning Tab/Grid and
  handle unsupported block selections. Clipboard transport is not wired.
Ready for [x]: YES
Approval: user explicitly approved TASK-043, checkbox update and scoped commit;
  no push requested
```

`copy_logical_lines` joins selected rows marked with `WRAPLINE` into one text
fragment without a newline. A hard break starts a new fragment for TASK-044
to join. It preserves selected spaces and combining marks, emits each wide
glyph once, and rejects stale, out-of-grid, inverted or block ranges. Tests
cover partial and reverse selections, Unicode, scrollback and hard-break
boundaries. It does not send data to a clipboard.

## TASK-044 Verification (2026-10-30)

```text
TASK: TASK-044
Status: DONE
Dependencies: satisfied (TASK-040, TASK-026 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline copy_hard_break_preserves_newline -- --test-threads=1
Result: PASS; test-first compilation failed before copy_text existed
Workspace tests: PASS — 105 unit tests + 9 Linux PTY integration tests + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: existing 9 real Linux PTY fixtures PASS;
  real Wayland Clipboard, GUI Copy and Sway/Fcitx5 NOT RUN
Security / Privacy impact: pure text conversion, no payload logging or I/O;
  inserted separators count toward the 16 MiB Copy limit, with rejection
  instead of silent truncation
Known limitations: selected blank Grid cells remain spaces, including on
  visually empty rows; Grid cannot distinguish untouched padding from an
  explicitly written space. No Clipboard transport or GUI wiring yet.
Ready for [x]: YES
Approval: user explicitly approved TASK-044, checkbox update and scoped commit;
  no push requested
```

`copy_text` joins logical-line fragments with `\n` only at hard breaks.
Tests cover mixed soft and hard wraps, partial selections, visually empty
rows, Unicode, scrollback and the separator-aware output size limit.

## TASK-045 Verification (2026-10-30)

```text
TASK: TASK-045
Status: DONE
Dependencies: satisfied (TASK-040, TASK-028 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline selection_preserves_fullwidth_glyph -- --test-threads=1
Result: PASS; test-first compilation failed before wide-range API existed
Workspace tests: PASS — 106 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland mouse/GUI/Clipboard NOT RUN
Security / Privacy impact: bounded, read-only Grid lookup; no I/O, payload logging, unsafe or dependency
Known limitations: caller must use range_preserving_wide for rendering; native event-loop wiring is pending
Ready for [x]: YES
Approval: user approved TASK-045 checkbox and scoped local commit; no push
```

Wide-range normalization expands a selected lead or trailing spacer to both
cells. A wrap-leading placeholder maps to the complete wide glyph on the next
row. Copy uses the same normalization; combining marks stay on the lead cell.

## TASK-046 Verification (2026-10-30)

```text
TASK: TASK-046
Status: DONE
Dependencies: satisfied (TASK-040, TASK-002 已 [x])
Changed files: src/selection.rs, src/selection/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline selection_is_scoped_to_tab -- --test-threads=1
Result: PASS; test-first compilation failed before TabSelections existed
Workspace tests: PASS — 107 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real GUI Tab switching/alternate-screen selection NOT RUN
Security / Privacy impact: per-Tab pure state, no text logging, I/O, unsafe or dependency
Known limitations: caller owns one TabSelections per Tab and maps the current terminal mode; no runtime Tab Manager wiring yet
Ready for [x]: YES
Approval: user approved TASK-046 checkbox and scoped local commit; no push
```

`TabSelections` keeps separate main and alternate-screen `DragSelection`
instances for one TabId. Wrong-Tab access is rejected. `Screen::from_mode`
maps the terminal core's ALT_SCREEN flag without changing the Grid.

## TASK-047 Verification (2026-10-30)

```text
TASK: TASK-047
Status: DONE
Dependencies: satisfied (TASK-002, TASK-005 已 [x])
Changed files: src/lib.rs, src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_state_rejects_stale_offer -- --test-threads=1
Result: PASS; test-first compilation failed before ClipboardState existed
Workspace tests: PASS — 108 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland Clipboard NOT RUN
Security / Privacy impact: pure identity state only; no clipboard payload, logging, I/O, unsafe or dependency
Known limitations: no wl_data_device/source/offer object binding or data transfer yet
Ready for [x]: YES
Approval: user approved TASK-047 checkbox and scoped local commit; no push
```

`ClipboardState` tracks one current owner and internal offer identities.
Only the newest live offer can supersede an owner; stale and cancelled
offers cannot take ownership. Local ownership invalidates older offers.
