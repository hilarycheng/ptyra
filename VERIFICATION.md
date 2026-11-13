# Runtime task verification

## TASK-199（2026-11-13）

```text
TASK: TASK-199
Status: DONE
Dependencies: satisfied (TASK-106, TASK-149, TASK-160, TASK-175, TASK-179)
Changed files: src/runtime/ime.rs, src/runtime/ime/malformed_tests.rs,
  README.md, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline runtime_malformed_events_remain_bounded_and_isolated
Result: PASS — fixed seed 重播兩次；test-first 缺少 corpus，fixture ID 重用修正後通過
Workspace tests: PASS — all-features；336 unit、17 integration、2 doc；1 optional fish ignored
Format / Clippy: PASS — all-targets / all-features
Manual / Integration checks: deterministic fake adapters；真 120 秒五 Tabs soak 另見 TASK-198
Security / Privacy impact: 只新增 tests；無新增 runtime log/dependency/unsafe
Known limitations: 固定 corpus 唔係全面 fuzz；Wayland wire decoder／kernel races 未覆蓋
Ready for [x]: YES
```

256 組 event sequences、serial 0/MAX 邊界、現有 malformed byte corpus 同 128 組 seeded byte streams，各重播兩次。
覆蓋 Clipboard device generation、offer object reuse、stale readiness、FD cancellation、EINTR 每次最多四次、UTF-8／byte limits。
IME 測錯誤 preedit cursor、oversized commit、future/old done、切 Tab 後再返回、exactly-once commit、pending paste epoch。
PTY 測 duplicate/unknown ready IDs、四個 reads 上限、query flood／64 KiB FIFO、錯誤來源／長度／OSC、source failure cleanup。
健康 Tab 嘅 grid 同 input 不變；關閉 Tab ID 不可重用，stale readiness 無 dispatch。
Output/configure caps 保持 bounded；冇聲稱固定 fixtures 可以證明任意輸入都安全。
證據：`/tmp/ptyra-t199-{red,test,workspace}.log`、`/tmp/ptyra-final-clippy.log`。

最終 SHM 額外 regression：CJK／combining／styles compositor pixel checks PASS。
合併 fonts/styles/scale 初次 run 出現 timeout；screenshot 證實 headless software cursor 遮住取樣格。
`tests/native_scale.py` 加一行移開 fixture pointer，重跑完整合併 run PASS。
同時通過 output move/removal、mouse、CJK Copy、Tab click、keyboard/focus、reconnect 同正常 exit。
證據：`/tmp/ptyra-t196-native-final.log`；日常 desktop acceptance gates 不變。

三份量測已保存喺 `target/benchmarks/task-196-199/`：`renderer-shm.json`、`renderer-amd.json`、`soak.json`。
`target/` 係本機產物，唔提交 Git；每份 JSON 都保留當次 binary／harness hashes，同執行環境。

## TASK-198（2026-11-13）

```text
TASK: TASK-198
Status: DONE
Dependencies: satisfied (TASK-135, TASK-152, TASK-175, TASK-179, TASK-195)
Changed files: tools/soak.py, tools/soak_session.py, tests/soak_runner.py,
  tests/benchmark.rs, README.md, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline soak_runner_records_failures_and_cleanup
Result: PASS — 3 deterministic fake process/clock checks；test-first missing runner 後通過
Workspace tests: PASS — 335 unit、17 integration、2 doc；1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: 五 Tabs，seed 198，120.022 秒／571 operations／106 samples，cleanup PASS
Security / Privacy impact: 私有 Sway/D-Bus/Fcitx5；固定 workload；只記錄 metadata；無新 dependencies/unsafe
Known limitations: 短測試唔係長期驗收；未跑場景獨立記錄；無自動判斷 leak slope
Ready for [x]: YES
```

30 秒 smoke 完成 149 operations；最終 120 秒完成 output 81、resize 82、switch 82、reopen 82、input 81、Clipboard 81、IME 82 次。
同一 Ptyra process，samples 全部維持五個 children；Ptyra FD count 首／尾／peak 都係 11。
PSS 首 6,857,728 bytes，尾／peak 22,656,000 bytes；包含初次 history/glyph/cache 分配，唔據此宣稱無 memory leak。
每次 reopen 即時確認舊 child 消失；最後 close 亦核對所有 owned children 已退出。
最初 setup timeout 如實保留 ERROR／NOT RUN 報告，延長 startup 等待後重跑通過。
報告：`/tmp/ptyra-t198-soak.json`；regression：`/tmp/ptyra-t198-final-tests.log`。

## TASK-197（2026-11-13）

```text
TASK: TASK-197
Status: DONE
Dependencies: satisfied (TASK-126, TASK-167, TASK-196)
Changed files: Cargo.toml, src/runtime/renderer_probe.rs,
  src/runtime/renderer_probe/gpu.rs, src/runtime/renderer_probe/gpu/native.rs,
  DEPENDENCIES.md, TASKS.md, VERIFICATION.md, README.md
Unit test: cargo test --workspace --locked --offline gpu_renderer_probe_preserves_frame_and_resource_bounds
Result: PASS; test-first 缺少 Context/Gpu，實作後通過
Workspace tests: PASS — all-features，335 unit、16 integration、2 doc；1 optional fish ignored
Format / Clippy: PASS — all-targets / all-features
Manual / Integration checks: AMD RX 6600、OpenGL ES 3.2 Mesa 26.2.4，SHM/GLES 各 800 frames
Security / Privacy impact: optional system FFI；理由、pointer/lifecycle/resource 邊界見 DEPENDENCIES.md
Known limitations: offscreen hybrid CPU raster + GPU blit；GPU memory NOT AVAILABLE；未測 Sway GPU presentation
Ready for [x]: YES
```

Sandbox 內 EGL 初始化失敗；正常 OS 環境使用真 AMD driver，800 frames 全部 exact readback 一致。
PSS/RSS、單核 CPU、每 frame timings 同 driver metadata 記錄於 `/tmp/ptyra-t197-amd.json`。
Rough submit mean：SHM 0.615 ms，GLES 1.354 ms；呢組包含 resize/冷 cache，唔係 renderer 選型結論。
GPU PoC 唔替換預設 SHM，亦唔聲稱 GPU glyph 加速或長時間 desktop 驗收完成。
OS package metadata 係 Mesa 25.2.8；實際 GL driver 回報 Mesa 26.2.4，兩者分開記錄，唔混作同一版本。

## TASK-196（2026-11-13）

```text
TASK: TASK-196
Status: DONE
Dependencies: satisfied (TASK-030, TASK-195)
Changed files: Cargo.toml, examples/renderer_probe.rs, src/runtime.rs,
  src/runtime/renderer_probe.rs, tools/renderer_benchmark.py,
  tests/benchmark.rs, tests/renderer_benchmark.py, tests/native_scale.py,
  TASKS.md, VERIFICATION.md, README.md
Unit test: cargo test --workspace --locked --offline renderer_benchmark_replays_identical_frames
Result: PASS; test-first missing implementation, then idle-fixture failure, both resolved
Workspace tests: PASS (sandbox 外；sandbox owner/socket checks 被限制)
Format / Clippy: PASS
Manual / Integration checks: release replay，兩次 SHM 各 16 frames，exact readback PASS；最終 native pixel probes PASS
Security / Privacy impact: metadata only；無新 dependency/unsafe；預設 renderer 不變
Known limitations: offscreen CPU raster / SHM transfer；不量度 compositor/vsync
Ready for [x]: YES
```

固定 synthetic fonts、grid/input、dirty rows、scale=1/2、resize，同一 binary 重播。
SHM 重用 runtime `write_frame_rows`，兩個 memfd slots 保留 stale rows。
每 frame 先比較 partial/full raster，再逐 byte 核對 backend readback；digest 只用作 trace identity。
Python driver 重用 TASK-195 procfs 計算，記錄 CPU/PSS/RSS/GPU、timings、failure、hash 同平台。
CPU/resource 包括 validation；raster/submit timings 排除 full reference 同 readback。
未包含 Tab bar、selection、IME overlay、Wayland presentation；唔可用結果決定正式 backend。
證據：`/tmp/ptyra-t196-{red,test,workspace-native,clippy}.log`、`/tmp/ptyra-t196-benchmark.json`。

## TASK-194（2026-11-13）

先完成 TASK-193 同 checkbox 更新，再實作本項。
新增完整 default config 同部分 override example，測試先因檔案未存在而失敗，再驗證 parser 結果。
README 更新 config path、shortcuts、paste confirmation、caps、native dependencies、install／uninstall API 同能力表。
修正開頭過時嘅 Kitty／cursor／HiDPI 說明；有日期嘅舊 verification 紀錄保留。
Task count 依 TASKS.md 核對為 199 項，195 項已勾選；四項 backlog 同整體 desktop gates 保持未勾選。

```text
TASK: TASK-194
Status: DONE
Dependencies: satisfied (TASK-130, TASK-154, TASK-172, TASK-193)
Changed files: assets/config.default.toml, assets/config.example.toml,
  src/config.rs, README.md, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline documented_config_examples_parse_with_defaults
Result: PASS — full defaults equal Config::default; partial overrides preserve other defaults
Workspace tests: PASS — 333 unit, 15 integration, 2 doctests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: documented values checked against config/runtime owners;
  native dependencies checked against local build; packaging evidence recorded under TASK-193
Security / Privacy impact: documentation and fixtures only; no runtime policy or dependencies changed
Known limitations: actual desktop/AMD/Fcitx5/full TUI gates NOT RUN in this task;
  no persistent receipt or install CLI; no performance superiority claim
Ready for [x]: YES
Approval: APPROVE for TASK-192 to 194; existing checkbox authorization in TASKS.md
```

最終 workspace log：`/tmp/ptyra-task194-tests.log`。冇安裝／移除真正 user 或 system prefix，亦冇改全域設定。

## TASK-192–193（2026-11-13）

兩項逐項實作同驗證。沿用 TASKS.md 已記錄嘅 checkbox 更新批准，同本次 `APPROVE for TASK-192 to 194`。

TASK-192 receipt 只由成功 install 建立，欄位保持 private。
Uninstall 先核對完整三條 plan paths，再逐項檢查 device、inode、size、mode、owner、mtime 同 ctime。
沿用 directory FD、O_NOFOLLOW 同 private capture/recheck，保留 replacement、symlink、外部修改同 parent directories。
檔案已消失視為完成；個別拒絕唔阻止清理其他已核實檔案。重複 uninstall 可安全執行。
Prefix 須由 operator 控制；同 UID 惡意 concurrent writer 仍唔屬隔離範圍。

```text
TASK: TASK-192
Status: DONE
Dependencies: satisfied (TASK-191)
Changed files: src/install/adapter.rs, src/install/adapter/filesystem.rs,
  src/install/adapter/tests.rs, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline uninstall_preserves_unowned_or_changed_files
Result: PASS — fake receipt validation + real temporary prefix
Workspace tests: PASS — 331 unit, 14 integration, 2 doctests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: replacement, same-length edit, mode, symlink,
  parent symlink, unrelated files, missing file, foreign receipt and repeat PASS
Security / Privacy impact: no new dependency, unsafe, payload log or system writes
Known limitations: Linux GNU; in-memory receipt; directories remain; operator-controlled prefix
Ready for [x]: YES
```

TASK-193 嘅 installed desktop entry 使用已 escape 嘅 absolute Exec 同 TryExec。
依 [Desktop Entry Exec 規則](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html) 處理引號同 backslash；唔經 Shell。
Prefix 須為 UTF-8，而且唔可以包含 `=` 或 `%`；GIO 對含 `%` executable path 嘅實測載入失敗，installer 喺寫檔前拒絕。
Source terminfo 用 `tic` 重建後逐 byte 比對；installer 同 runtime 共用 `terminfo::COMPILED`。
保留已測試嘅 16 colors／256 pairs／op／bold 能力，冇新增未驗證 capabilities。

```text
TASK: TASK-193
Status: DONE
Dependencies: satisfied (TASK-020, TASK-123, TASK-124, TASK-191)
Changed files: src/install/adapter.rs, src/install/adapter/tests.rs, src/terminfo.rs,
  src/pty/terminfo.rs, tests/packaging.rs, tests/native_packaging.py, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline packaged_capabilities_match_runtime_support
Result: PASS — fake launcher/asset contract + compiled asset/install integration
Workspace tests: PASS — 332 unit, 15 integration, 2 doctests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: PTYRA_TEST_PACKAGING_NATIVE=1 with the same targeted command
  and -- --nocapture PASS; user-local/staged install, Gio launch, actual binary identity,
  foreign cwd, private TERMINFO, ncurses capabilities, cleanup and uninstall verified
Security / Privacy impact: no new runtime dependency, unsafe, network or system writes
Known limitations: receipt is not persistent; staged launcher is bound to its absolute prefix
  Actual Sway 1.10.1 desktop/AMD/Fcitx5/full TUI acceptance NOT RUN
Ready for [x]: YES
```

環境：Ubuntu 24.04.4、owned headless Sway 1.9／pixman、ncurses 6.4.20240113。
Native probe 額外用已安裝嘅 Python GI/GIO 同 desktop-file-validate；Ptyra runtime 冇引入 GIO／GTK。
先寫測試，再確認缺少實作時失敗。TASK-192 初次 disk test 受 sandbox owner mapping 阻擋；sandbox 外 PASS。
TASK-193 初次 probe 欠 mapping barrier；修正等待後再測，發現 `%` prefix 問題並加入拒絕測試。
最終兩種 prefix 完整 native probe PASS；測試 cleanup 有 terminate/kill fallback。
Workspace logs：`/tmp/ptyra-task192-tests.log`、`/tmp/ptyra-task193-tests.log`。

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


## TASK-155

- Dependencies satisfied. Recoverable Clipboard/Primary failures now display a bounded, non-modal amber metadata strip. Empty/no offer, non-text, payload cap, invalid UTF-8, owner replacement/disappearance, unavailable transport, absent selection/serial, changed target, busy transfer and generic FD failures have fixed outcomes. No caller error string or Clipboard/key/PTY payload is retained or rendered; unknown errors map to a fixed generic notice.
- Notices do not consume input, mutate terminal cells or terminate sessions. The next key/pointer press or focus/Tab transition dismisses them; a multiline confirmation takes visual precedence. No expiry timer or recurring idle redraw was added. Text wraps within bounded geometry; if it cannot fit at the current font/window size, a visible amber strip remains until resize reveals the metadata.
- Send pumps report failures while retaining unrelated jobs and source ownership. Receive failures release only their pending endpoint/FIFO reservation; a later Copy/Paste remains usable. A compositor offer disappearing during a stalled read is an owner-change result. Explicit user/focus/Tab cancellation and routine source replacement stay quiet. Empty text produces no Shell bytes and a clear empty notice.
- `runtime_clipboard_errors_do_not_leak_or_exit`: first FAIL against generic-only mapping, then PASS. Tests cover fixed metadata, arbitrary secret-bearing error suppression, geometry bounds, empty/failed/recovered receives, owner loss, silent explicit cancellation, failed-send isolation and subsequent successful sends.
- Workspace: 240 combined / 238 standalone library tests, 13 integration tests and 2 doctests PASS; one pre-existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes unaccepted T129.
- Owned headless Sway on both binaries: visible empty/no-offer, non-text, limit, UTF-8, stalled-owner disappearance and real EPIPE notices PASS. A private data-control fixture keeps the writer FD open while its owner disconnects, proving cancellation is not just EOF. Another requests native-source data with the read end already closed, then confirms a later transfer from the same source succeeds. Quiet focus cancellation, continued Shell input, unchanged later Paste and no payload logs also PASS.
- Existing full native Clipboard/Primary/selection/multi-tab matrix and separate partial-paste cancellation matrix PASS on both binaries. `/tmp/ptyra-t155-notice.png` visually checked: readable fixed error text, no Clipboard preview. New native flags are isolated from the existing TRACE/matrix cases.
- T129 preserved/excluded, no dependency or unsafe changes, no push/install/global configuration changes. CPU affinity 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t155-*`.


## TASK-157

- Dependencies satisfied. Four configurable bindings join the existing duplicate/IME-conflict validation: `scroll_page_up`/`scroll_page_down` default to Shift+Page_Up/Shift+Page_Down; `scroll_top`/`scroll_bottom` default to Shift+Home/Shift+End. The normal action router consumes them without PTY bytes or repeat leakage.
- Vertical wheel input inside terminal content scrolls only the captured active Tab. Ten Wayland logical units equal one line; fractional input accumulates, with finite/axis checks and a bounded per-event delta. Focus/pointer-enter/leave, Tab changes and keyboard operations reset residual motion. Tab-strip/modal-prompt events cannot scroll content. Application mouse modes reserve wheel events; alternate-screen history remains inactive. Explicit scroll keys remain available even with application mouse ownership.
- Follow-bottom policy: successfully queued nonempty legacy keyboard bytes (including repeat) return only the active viewport to the live cursor. New PTY output alone preserves the scrolled history position using the core policy, subject to bounded history eviction. Switching/new Tabs preserves other offsets. Changed viewports cancel an in-progress drag/click sequence and request a frame, without mutating cells/cursor or injecting scroll bytes into a Shell.
- Config regression first FAILed before bindings existed. `runtime_scroll_input_targets_active_viewport` PASS for fractional/large/invalid/horizontal wheel events, mouse/alt ownership, active-only operation, page/top/bottom bounds, unchanged cursor, history-output preservation, Tab isolation and residual reset. All four custom actions also pass the existing keyboard/IME-reservation tests.
- Full workspace: 241 combined / 239 standalone library tests, 13 integration tests and 2 doc tests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees.
- Owned headless Sway on both binaries: real wheel/fractional input, default page/bottom and custom Alt+Home mapping, no history spill into a new Tab, preserved old-Tab offset, output remaining above bottom, subsequent keyboard input following bottom and application mouse ownership all PASS. Opt-in trace records only Tab ID/display offset, no terminal text.
- Full Clipboard/Primary/selection/tab native matrix, partial-paste cancellation, T154 OSC policy and T155 visible-error/owner-loss/EPIPE recovery fixtures PASS on both final binaries. T129 source/docs preserved and excluded; no push/install/global configuration/dependency/unsafe changes. CPUs 0–1, Cargo jobs=2.
- This task validates input routing and actual viewport offsets. Correct historical row pixels/CJK/reflow and history selection/copy are TASK-158/TASK-159 and remain unchecked. IME viewport integration and real desktop acceptance remain deferred. Logs `/tmp/ptyra-t157-*`.


## TASK-158

- Dependencies TASK-126/TASK-156 satisfied. Both background and glyph passes map signed core rows through the snapshot display offset, with checked addition and visible-grid clipping. The live cursor uses the same transform and is hidden outside the grid even when the pixel buffer has spare rows. Whole-frame clearing still prevents stale scroll pixels; no cells, cursor position or PTY bytes are changed.
- `runtime_history_rows_use_display_offset`: first FAIL against collapsed history rows, then PASS. Pixel equality covers separate historical background/glyph rows, CJK/combining and soft-wrap placement, narrower-width reflow, top/bottom round trips, offscreen cursor including extra buffer padding, and an onscreen cursor shifted by one history row.
- Full workspace: 242 combined / 240 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone explicitly excludes unaccepted T129 additions.
- Owned headless-Sway native pixel fixtures PASS on both binaries with default and custom scroll bindings: original live rows equal their later historical glyph/background pixels, offscreen cursor produces no pixel changes, scroll round trip leaves no residual pixels, and a verified actual window/PTY column resize reflows history identically to freshly emitted reference text. CJK/combining glyphs and soft wrap were visually inspected in `/tmp/ptyra-t158-combined-history.png`. Sway configure/frame timing is awaited; clear-screen precedes clear-history to avoid accidentally retaining the test's command line.
- Full existing native Clipboard/Primary/selection/tab matrix and separate partial-paste cancellation PASS on both binaries. Read-only review found no raster defect; its combined-suite custom-shortcut finding was fixed and retested.
- T129 raster additions were reverse-compared with their preserved baseline; README/IME source/fixture byte comparisons passed. No dependency, unsafe, push, install or global settings changes. CPU affinity 0–1 and Cargo jobs=2.
- Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. This checks native SHM history pixels, not IME acceptance, history selection (TASK-159), HiDPI or GPU rendering. Logs `/tmp/ptyra-t158-*`.


## TASK-159

- Dependencies TASK-144/TASK-146/TASK-156/TASK-158 satisfied. Pointer hit-testing subtracts the active display offset after clamping to the visible grid. Drag, double/triple click, Copy and the already offset-aware overlay now share original historical coordinates.
- Selection policy: completed ranges remain tied to unchanged grid lines across viewport moves and Tab switches, including offscreen Copy. Runtime scroll cancels an active drag/click sequence. Any output or geometry change invalidates the content epoch, preventing history eviction/reflow from silently retargeting a selection.
- `runtime_history_selection_copies_original_lines`: first FAIL, copying live rows instead of history; then PASS. Covers negative anchors, hard/soft wrap, CJK/combining, exact spaces, historical overlay pixels, offscreen retention, Tab isolation, wide-spacer double click, logical-line triple click, cancelled drag, eviction and resize invalidation.
- Full workspace: 243 combined / 241 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129.
- Owned headless Sway on both binaries, with default/custom scroll bindings: negative-row pointer hits, complete CJK spacer Copy, exact soft-wrap/combining/spaces, historical highlight pixels, offscreen settled-range Copy, and output invalidation removing the highlight all PASS. Every Copy first replaces Clipboard with a verified sentinel, preventing a stale result from passing. The pointer sprite is moved outside the sampled surface for exact pixel comparison.
- Full native Clipboard/Primary/selection/tab and separate partial-paste cancellation matrices PASS on both binaries, plus T158 history/reflow/cursor pixels. Review found no production defect; its stale-Clipboard test gap was fixed and retested.
- T129 edits unchanged/uncommitted. No push/install, dependency/unsafe addition or global settings changes. CPUs 0–1; Cargo jobs=2. Real desktop Firefox/Kitty/Fcitx5 acceptance NOT RUN. Logs `/tmp/ptyra-t159-*`.


## TASK-160

- Dependencies TASK-023/TASK-026/TASK-132 satisfied. Core `Event::PtyWrite` is captured in a per-Terminal ordered byte queue capped at 64 KiB with bounded geometric allocation. One runtime data dispatch drains it once into the original Tab's existing 64 KiB regular-input FIFO. It never uses active keyboard focus, renders response bytes, bypasses a pending paste barrier, or writes directly to the PTY FD.
- Accepted responses remain ordered through partial writes, EINTR and WouldBlock. Collector overflow rejects the complete response and remaining batch suffix; shared FIFO rejection rejects the whole drained batch, preserving already accepted input. Rejections produce fixed visible metadata and are not silently retried or sent to a successor. The parser remains usable after reply overflow. General keyboard backpressure and PTY/stream failure isolation remain TASK-178/TASK-179.
- `runtime_terminal_replies_return_to_source_pty`: first FAIL against an empty collector, then PASS. Every split of DA/DSR/CPR produces exact ordered replies once on a hidden source, without changing cell text. Fake-writer tests exercise interrupted/short/blocked writes, paste reservation/framing, preceding/following input, full shared FIFO rejection and closed-source isolation. `core_reply_overflow_is_bounded_atomic_and_recovers` verifies the exact cap, complete-response rejection and recovery.
- Workspace: 245 combined / 243 standalone library tests, 13 integration tests and 2 doctests PASS; one pre-existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes unaccepted T129 implementation.
- Owned headless Sway on both binaries: exact split DA/DSR/CPR and 3,000-query bursts return once to two distinct raw PTYs, including a hidden Tab while the entire window is unfocused. Delayed raw reads exercise actual kernel backpressure. Full Clipboard/Primary/selection/multi-Tab matrix and separate partial-paste cancellation matrix PASS on both binaries. Read-only review found no T160 issue.
- T129 source/docs/fixture preserved and excluded. No dependency/unsafe addition, push, install or system settings changes. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t160-*`.


## TASK-161

- Dependencies TASK-136/TASK-160 satisfied. Bounded core `ColorRequest` and `TextAreaSizeRequest` callbacks format replies through T160's original-Tab FIFO. Ordinary callbacks drain before the next parser step. A narrow handler on OSC 4's terminating byte delegates parsing/mutation to VTE/Term while answering each color request before a later pair can modify the same index. The existing OSC cap also recognizes this dispatch boundary, including ignored C0 and BEL/ST/CAN/SUB behavior; no ANSI/color parser, unsafe code or dependency was added.
- Raster and queries share one resolved default/OSC-override palette for ANSI 0–255, foreground/background and nominal cursor color. Cursor strokes follow that color and OSC reset, preserving their prior default; focused block cursors retain the existing pixel-inversion policy. Unknown query indices/types receive no fabricated response. ClipboardLoad remains ignored, including with explicit OSC write opt-in.
- Pixel geometry uses the actual grid rows/columns and raster cell advances, excluding the Tab Bar and unused partial cells. Metrics initialize before grid resize attempts so new Tabs created during a rejected configure still answer for their retained valid grid. Absent/invalid metrics and areas not representable by the upstream u16 formatter are not reported; multiplication is checked first.
- `runtime_queries_report_actual_colors_and_geometry`: first FAIL, then PASS for every split, ordered DA/color/size/DSR, palette defaults/overrides/resets, actual cell/grid resize, same-OSC mixed query/update pairs, absent/invalid/overflow geometry, unknown events, bounded callback/reply floods and explicit ClipboardLoad denial. `terminal_palette_dispatch_matches_vte_boundary` compares all 256 inserted byte values and four dispatch terminators with VTE. The resize regression covers a new valid Tab under rejected resize. Deterministic raster pixels verify queried ANSI background and beam-cursor override/reset colors.
- Review identified the same-OSC ordering and rejected-resize initialization gaps; both were reproduced or covered by new regressions, corrected, inspected and retested with the full suites.
- Final workspace: 248 combined / 246 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes unaccepted T129.
- Owned headless Sway on both binaries: source-Tab defaults/overrides/resets and mixed OSC query/update replies match actual compositor marker-cell colors. Pixel/character replies match actual cell dimensions, content origin and both raw PTY sizes before/after resize. OSC 52 read queries return no data. T160 split/burst/source-isolation probes, history/reflow/cursor/selection pixels, full Clipboard/Primary/selection/Tab matrix and separate partial-paste cancellation all PASS on both binaries.
- T129 raster implementation blocks and complete preedit test remain byte-identical relative to their clean counterparts; independent README/IME/fixture comparisons pass. No push, install, system settings change, dependency or unsafe addition. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t161-*`.


## TASK-162

- Dependencies TASK-027/140/141/157/160 satisfied. Runtime routes DEC 1000 button/release, 1002 held-button cell motion, 1003 all cell motion, 1005 UTF-8 and 1006 SGR reports through the original active/focused Tab's existing bounded ordered PTY FIFO. Left/middle/right and vertical/horizontal wheel reports include actual one-based content cells and Ctrl/Alt modifiers. Same-cell motion is suppressed. Ten logical wheel units equal one tick, with fractional accumulation and a 64-report per-event cap.
- Shift starts a local selection/Primary gesture whose ownership lasts through release even if Shift changes. Shift-wheel uses local history; a normal app-owned middle click never also pastes. Fresh Tab Bar/padding-outside events do not report. Accepted app drags/releases clip at content edges; legacy/UTF-8 positions beyond their 223/2015 one-based range are not newly reported, while an accepted press's release clips to the encodable edge. SGR uses actual bounded grid coordinates. Unsupported X10, highlight tracking, urxvt and pixel-report modes are not implemented or advertised; terminfo remains conservative.
- Focus/Tab/pointer removal and observed tracking/encoding/screen-mode changes cancel captured ownership. Same-tab viewport scrolling cancels selection drag without discarding mouse button ownership. Rejected reports are dropped as whole events with fixed visible metadata; accepted presses are recorded only after enqueue succeeds, preventing orphan releases. Paste FIFO/barriers remain shared; no direct FD writes, retry/rerouting, payload logs, dependency or unsafe additions.
- `runtime_mouse_reports_only_enabled_modes` first FAILed against the empty router, then PASS. Covers enabled/disabled modes, exact button/release/motion/wheel/modifier bytes, local gesture ownership, strip/stale-target exclusion, finite/bounded wheel events, rejection and lifecycle reset. `mouse_encoding_bounds_are_exact` checks legacy/UTF-8 maxima and release clipping. `viewport_scroll_preserves_mouse_press_ownership` checks both local and app ownership through real core history movement plus the runtime cancellation helper.
- Independent review found the viewport cancellation bug: Shift-wheel/PageUp after an accepted press dropped its release; local selection could also leak 1003 motion. Native regression reproduced the missing releases before the fix. Both variants were corrected, re-reviewed with no remaining concrete findings, and retested.
- Final workspace: 251 combined / 249 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129.
- Owned headless Sway on both final binaries: exact raw-PTY mouse bytes, middle-click exclusivity, Shift selection/Primary paste, strip exclusion, focus cancellation, and both ownership cases across verified viewport changes PASS. Color/geometry queries, split/burst source replies, history/reflow/selection pixels, custom/default scroll bindings, full Clipboard/Primary/selection/Tab matrix and partial-paste cancellation also PASS. The previous scroll probe now owns a raw input reader while mouse mode is enabled, so valid mouse bytes never contaminate its Shell command line.
- T129 implementation/docs/fixtures remain preserved and excluded. No push, install or system settings changes. CPU affinity 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t162-*`.


## TASK-163

- Dependencies TASK-027/133/160 satisfied. A deduplicated runtime focus tracker emits `CSI I` / `CSI O` only when the corresponding source Tab currently enables DEC 1004. Keyboard window enter/leave, active-Tab switch/close successor and seat/keyboard removal update the tracker immediately. Duplicate/no-op events do not repeat reports; unrelated-surface leave events are ignored. Switching while unfocused remains quiet.
- Enabling/disabling the mode alone does not invent a focus transition. Actual transitions sample each source's current mode, including the old hidden Tab. Closed sources are skipped rather than redirected; the valid focused successor receives its own focus-in only when enabled. Both transitions use the existing bounded source-Tab FIFO and paste ordering. A whole-report enqueue failure shows fixed metadata and advances the tracker without retrying or suppressing the other Tab's transition.
- `runtime_focus_reports_follow_tab_focus`: first FAIL against the empty tracker, then PASS. Covers disabled/enabled modes, same-target deduplication, separate old/new source modes, repeated leave/removal, invalid/inactive targets, queue rejection without cross-Tab starvation/retry, and closed-source/successor isolation.
- Final workspace: 252 combined / 250 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes unaccepted T129.
- Owned headless Sway on both binaries: three raw PTYs verify exact focus streams across new/switch Tabs, window workspace leave/enter, repeated no-op focus and active-Tab close successor. The third Tab keeps reporting disabled across all transitions and receives no focus bytes. Existing mouse/gesture-ownership, palette/geometry, reply burst, history/selection/scroll, full Clipboard/Primary/Tab and partial-paste cancellation matrices PASS on both final binaries. Seat/keyboard-removal state is deterministic coverage, not a claimed separate native hot-unplug run.
- Self-reviewed the lifecycle call sites and existing FIFO/poll integration. T129 source/docs/fixtures remain preserved and excluded. No dependency/unsafe addition, push, install or global settings change. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t163-*`.


## TASK-164

- Dependencies TASK-012/027/128/130 satisfied. Runtime samples each active Tab's APP_KEYPAD and APP_CURSOR modes for both initial keys and repeat. XKB translation carries NumLock state from the compiled keymap's named virtual modifier, not an assumed Mod2 bit. The deterministic fixture deliberately maps NumLock to Mod3.
- Explicit legacy subset: normal keypad digits/operators/Enter/Tab/Space and navigation aliases map to their ordinary keys. Application keypad emits VT100/VT220 SS3 suffixes for digits/navigation, decimal, operators, equal, Enter, Tab and Space; PF1–4 keep the established F-key encoder. NumLock overrides application keypad. Shift/Ctrl use the ordinary modified-navigation/control path; Alt prefixes ESC. Navigation follows APP_CURSOR where applicable. Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.pdf. This is a documented subset, not full PC/VT220/xterm keypad compatibility.
- Existing Enter/arrows/Ctrl+C/F-key bytes are unchanged. Legacy Ctrl+Shift printable combinations use ordinary control bytes; Super combinations explicitly return Unsupported instead of unsolicited CSI-u. NumLock/ScrollLock modifier events emit nothing. Unknown multimedia keys and multi-symbol XKB results remain explicit Unsupported, without partial bytes. Negotiated Kitty keypad/event semantics remain later tasks, not a claimed legacy feature.
- `legacy_keypad_obeys_terminal_modes`: first FAIL for KP Enter, then PASS across normal/application cursor and keypad combinations, NumLock on/off and nonstandard mapping, modifiers, repeat mode changes, release and focus cancellation. `legacy_keypad_aliases_and_modifier_bytes_are_explicit` covers rare keypad aliases, modifier bytes and explicit unsupported cases.
- Final workspace: 254 combined / 252 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129. Independent read-only review found no concrete correctness, ownership or security regression; it did not independently execute tests.
- Owned headless Sway on both final binaries: actual XKB keypad keys and raw PTYs verify all mode combinations, NumLock, modified navigation, ordinary Enter/Shift+Enter/Ctrl+C/F1–5, no unsolicited Kitty bytes, distinct active-Tab modes and RIS reset. Existing mouse/focus/query/reply/history/selection/scroll probes, full Clipboard/Primary/Tab matrix and partial-paste cancellation PASS on both binaries.
- T129 source/docs/fixtures remain preserved and excluded. No dependency/unsafe addition, push, install or global settings change. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t164-*`.


## TASK-165

- Dependencies TASK-100/140/158 satisfied. The native pointer adapter calls the existing explicit Ctrl+left-press HTTP(S) gate only for the active keyboard-focused Tab, with the content origin removed and historical display offset respected. Output, hover, release, ordinary clicks, stale targets, unsupported schemes and outside/Tab-Bar coordinates never launch. Application mouse keeps ownership; Shift+Ctrl+click uses its existing local override without also sending mouse bytes or starting a selection.
- Activation must match the last submitted Tab/content epoch/display offset/content geometry. Notice/paste-covered submitted frames cannot activate links, including the click dismissing a notice. Review found that using a newer live grid or clearing a notice before hit-test could open an unseen target. The deterministic stale-scroll/output/geometry/Tab/overlay regression first failed, then passed after this guard; independent re-review found no remaining concrete issues.
- The adapter invokes `xdg-open` with exactly one literal URI argument, never a shell command. Stdin/stdout/stderr are null. At most four outstanding helpers have bounded 256 KiB-stack waiters; their one-byte status Unix sockets wake the existing poll loop. This narrowly justified blocking wait is off the UI thread because a desktop opener may wait for its browser, and avoids unbounded threads, zombies or periodic child polling. It does not constrain the external browser's resource usage or force-close it. A missing/failing opener or exhausted limit yields only fixed recoverable visible metadata; no URI or child output is logged.
- `runtime_hyperlink_open_requires_explicit_click`: first FAIL for missing behavior, then PASS. Tests historical hit-testing, exact URI preservation, focus/Tab/button/modifier/outside gates and metadata-only errors. `hyperlink_adapter_is_literal_bounded_and_recoverable` verifies argv, cap, completion/failure/closed-channel cleanup and reuse. The submitted-frame regression additionally covers the reviewed visibility risk.
- Final workspace: 257 combined / 255 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129.
- Owned headless Sway on both final binaries uses a temporary fake `xdg-open`, not a real browser/network target. Exact argv containing shell metacharacters remains literal; output/hover/invalid clicks stay inert; history clicks, application-mouse ownership, four slow helpers with keyboard input still draining, reaped helper PIDs, failure/dismiss/retry and payload-free logs all PASS. Existing keypad/focus/mouse/query/reply/history/selection/scroll probes, full Clipboard/Primary/Tab matrix and partial-paste cancellation PASS on both binaries.
- T129 implementation/docs/fixtures remain preserved and excluded. No dependency/unsafe addition, push, install or global settings change. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance and real browser handoff remain deferred and NOT RUN. Logs `/tmp/ptyra-t165-*`.


## TASK-166

- Dependencies TASK-006/017/026/127 satisfied. DEC 2026 is observed through the existing secondary VTE parser, alongside its keyboard protocol observer, without adding an ANSI parser. The terminal core intentionally continues immediate parsing, queries and bounded source-Tab replies. Presentation alone is held until end or a 150 ms monotonic deadline after runtime observes the begin. The former `NoSync` shortcut is replaced with this explicit presentation policy; VTE byte buffering remains disabled to preserve the verified one-byte OSC palette/query dispatch path.
- Each Tab owns its hold state. Hidden Tabs cannot hold the active Tab's frame; their deadlines still expire. Runtime takes the earliest sync/repeat deadline in its existing poll, with no periodic wakeup after expiry. PTY draining, keyboard input and query replies continue while a frame is held. DECRQM 2026 reports the actual held/reset state through a narrow parsed-query handler and the existing reply FIFO. RIS and ordinary end release the requested mode.
- Repeated begins, end+begin or RIS+begin before any frame submission retain the first deadline, preventing indefinite extension. After a timeout and actual submitted frame, a new end+begin can rearm even when both arrive in one PTY read. Expired repeated begins alone cannot rearm. Local selection is cancelled while synchronized pixels are unsubmitted, including the interval between deadline expiry and actual submission; this prevents copying unseen replacement text. Application-mouse routing and release ownership remain earlier and unchanged; T165's submitted-link guard remains effective.
- `runtime_sync_update_timeout_releases_frame`: first FAIL for the missing deadline, then PASS with fake clock, immediate grid/replies, repeat-begin bound, active/hidden Tab isolation, end and reset. `sync_sequences_preserve_queries_and_split_boundaries` covers every split, mixed sync/DSR/same-OSC palette queries, unsupported/cancelled sequences, actual query state and clock overflow. `sync_reset_cannot_extend_hold_or_select_unseen_text` covers selection, reset deadline, post-expiry pre-submission visibility, and batched restart after actual submission.
- Independent review identified reset-based deadline extension, unseen local selection, and expired-state restart across coalesced end+begin. The selection regression reproduced the defect before correction; all three have deterministic coverage, were corrected and re-reviewed with no remaining concrete findings. The reviewer did not execute tests.
- Final workspace: 260 combined / 258 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129.
- Owned headless Sway on both binaries: actual compositor pixels remain unchanged while the new core state and exact mode/DSR replies are already available; keyboard bytes drain during the hold. Early end, silent missing-end timeout, repeated begin and repeated pre-presentation RIS+begin release correctly. The silent case needs the poll deadline alone; post-timeout idle CPU sampling finds no busy loop (not a formal performance benchmark). Existing hyperlink/keypad/focus/mouse/query/reply/history/selection/scroll probes, full Clipboard/Primary/Tab matrix and partial-paste cancellation PASS on both binaries.
- T129 implementation/docs/fixtures remain preserved and excluded. No dependency/unsafe addition, push, install or global settings change. CPUs 0–1; Cargo jobs=2. Actual presentation still waits for compositor/frame/buffer readiness; 150 ms is the terminal's hold limit, not a compositor latency guarantee. Real desktop Sway/Fcitx5/Firefox/Kitty acceptance remains deferred and NOT RUN. Logs `/tmp/ptyra-t166-*`.


## TASK-167

- Dependencies TASK-015/031/125/126/140 satisfied. Runtime negotiates compositor up to v6 and output up to v4. A fresh `preferred_buffer_scale` takes precedence; otherwise it uses the maximum completed scale of entered outputs, or 1 with none. Output scale updates apply at `done`. Surface enter/leave, output identity and global removal are tracked without polling. Ordinary enter/leave retains a valid surface preference. Removing an entered output invalidates a potentially stale preference: native Sway 1.9 sent global removal and a new enter without a replacement preference during this test.
- Logical configure size is separate from physical buffer size. One checked conversion drives SHM dimensions/stride/damage, raster metrics, content origin, Tab Bar, query cell pixels, pointer conversion and paste-prompt geometry. The buffer scale is committed with its matching buffer. Logical cell advances stay stable across integer scales; glyph size/baseline scale with them, and a scale change clears cached masks. The 8-logical-unit Tab Bar padding scales too. Local hit tests wait for submitted geometry, while accepted application-mouse releases retain ownership. Double/triple-click motion tolerance remains 4 logical units at every scale.
- Supported buffer scale is integer 1–4, reduced to the highest scale fitting the existing 4,194,304-pixel and 256x512 physical-cell caps before allocation. Reduced resolution is a valid fallback; it does not promise fractional-scale sharpness. No fractional-scale/viewport protocol is implemented. At most 64 live outputs are bound. v3+ outputs receive `release`; v1/v2 use local backend retirement and a non-replenishing lifetime limit of 64 bindings because the legacy protocol has no release request. Beyond that legacy limit, new outputs use the remaining surface preference or scale-1 fallback. General oversized logical configure handling remains TASK-174.
- `runtime_scale_keeps_logical_and_buffer_coordinates_consistent`: first FAIL for missing implementation, then PASS for output enter/leave/done/removal, preferred-scale precedence, legacy binding limits, checked geometry/overflow/caps and pointer conversion. Additional deterministic tests cover scaled Tab boundaries, glyph-cache invalidation and exact scale-1 pixel restoration, and equivalent jittered word/line-selection gestures at 1/2/1. Configure invalidates unsubmitted local hit-test geometry.
- Review found the fixed-pixel multi-click tolerance and legacy object-retirement gaps. Native testing exposed the stale preference on output removal; review narrowed the workaround to removal rather than ordinary enter/leave. All were corrected and independently re-reviewed with no further concrete findings. The reviewer did not run builds/tests.
- Final workspace: 264 combined / 262 standalone library tests, 13 integration tests and 2 doctests PASS; one existing opt-in integration ignored. Format, Clippy `-D warnings` and build PASS on both trees. Standalone excludes deferred T129.
- Owned headless Sway on both final binaries: 1→2→1 scale changes, moving to a second scaled output and removing it verify doubled actual compositor marker pixels/cell metrics, unchanged logical rows/columns, exact application-mouse bytes, scaled Tab clicks and jittered CJK selection/Copy. A temporary standalone build capped negotiation at compositor v4/output v2 passes the same native sequence, exercising absence of preferred-scale events and legacy output retirement; normal negotiation was restored and rebuilt afterward.
- Synchronized-update, hyperlink, keypad, focus, mouse, query/reply, history/selection/scroll probes and the full Clipboard/Primary/Tab/partial-paste matrices PASS on both final binaries. One standalone matrix initially timed out because its old `cat` plus fixed sleep allowed the shell prompt to arrive after selection/publication. The fixture now holds raw PTY output behind a real DSR readiness barrier; the corrected complete matrix and separate partial-paste runs PASS on both binaries. The original failure log is retained as `/tmp/ptyra-t167-clean-native-before-fixture-fix.log`.
- T129 raster implementation blocks and complete preedit test, README, IME source and native fixture remain byte-preserved and excluded. No dependency/unsafe addition, push, install or global settings change. CPUs 0–1; Cargo jobs=2. Real desktop Sway/Fcitx5/Firefox/Kitty/AMD acceptance remains deferred and NOT RUN, including IME scale/candidate geometry reserved for TASK-176. Logs/scripts `/tmp/ptyra-t167-*`.

## TASK-173 follow-up: ncurses / htop color contract

- Real htop 3.3.0 reproduced the user's unreadable black-on-black fields in both the then-current debug binary and the older release binary. The embedded standalone terminfo entry omitted `op`. ncurses therefore rejected `use_default_colors()` and htop's normal `init_pair(..., foreground, -1)` requests, leaving those pairs black/black. Raising the pair count or adding bold alone did not fix them. The 8-color declaration also made htop select ANSI black for its gray pair instead of index 8. Pinned upstream evidence: [htop default-color initialization](https://github.com/htop-dev/htop/blob/68c970c7ef4a0682760ed570b3d82388ae7ccf54/CRT.c#L1039-L1042), [htop pair/gray selection](https://github.com/htop-dev/htop/blob/68c970c7ef4a0682760ed570b3d82388ae7ccf54/CRT.c#L1109-L1127), and [ncurses default-color prerequisite](https://github.com/ThomasDickey/ncurses-snapshots/blob/f94b0bf6e093e63f799ddfa04d2be2d0afe05e34/ncurses/base/lib_dft_fgbg.c#L69-L95).
- The fix only updates the source and embedded compiled terminfo to advertise existing support: 16 ANSI colors, 256 pairs, `op=\E[39;49m`, `bold=\E[1m`, and proper normal/bright foreground and background sequences. No raster policy, palette, user config or TERM override was changed. In particular, explicit black remains black; bold does not implicitly remap colors. This is a focused compatibility correction, not completion of the still-unchecked installation task TASK-193.
- The extended deterministic `terminfo_does_not_claim_unsupported_features` test failed against the old entry, then passed. It verifies the capability allowlist, all normal/bright core indices and that `op` resets colors while preserving bold. The new opt-in `PTYRA_TEST_TERMINFO=1` probe uses the actual private embedded TERMINFO, real ncurses default-background pairs, pair 255, all 16 `tparm` expansions and exact compositor pixels for foreground/background/bright black and default-color reset with bold. Every native harness launch also recompiles the source with `tic` and checks byte parity with the embedded entry.
- Full workspace checks: combined 279 / isolated 277 library tests, 13 integration tests and 2 doctests PASS; the opt-in fish integration is ignored by the aggregate run and was then explicitly run on both trees. `fish_multiline_prompt_redraw_keeps_one_character` PASS with `--no-config` and an owned deterministic prompt, without loading user startup files (`/tmp/ptyra-htop-{combined,clean}-fish.log`). Format, Clippy `-D warnings`, combined/isolated debug builds and the requested release rebuild PASS. Logs: `/tmp/ptyra-htop-{combined,clean}-{test,fmt,clippy}.log`, `/tmp/ptyra-htop-build.log`, `/tmp/ptyra-htop-clean-build.log`, `/tmp/ptyra-htop-release-build.log`. The standalone tree excludes all deferred TASK-129 changes.
- Controlled before/control htop captures used only an owned headless Sway, a private HTOPRC and an owned harmless process filter. Inspected baseline/control screenshots: `/tmp/ptyra-htop-diagnosis/debug-current/baseline-before.png` and `/tmp/ptyra-htop-diagnosis/debug-op16/baseline-before.png`. Actual SGR/pixel checks separated named/indexed black, explicit bright black, bold, dim, inverse and foreground/background. A temporary `op`-only entry restored normal colors; adding truthful 16-color support restored gray, without a renderer workaround.
- An initial throwaway capture wrapper incorrectly inherited its own SHELL through util-linux `script`; it was corrected to use `/bin/sh`, all exact-path residual probe processes were terminated and their absence was verified. No evidence from that invalid attempt counted. An initial style/scale invocation combined synthetic-font and normal-font fixture assumptions; style checks passed but the scale probe timed out. The unchanged established separate configurations were rerun. The invalid-combination log is retained at `/tmp/ptyra-htop-incompatible-font-scale-flags.log`; neither setup problem was treated as a production regression.
- Original TASK-129 tracked changes are byte-identical to the pre-fix diff; both untracked IME files retain their original hashes. No TASK checkbox, dependency, unsafe code, installation, push, desktop setting or other global configuration was changed. Checks ran on CPUs 0–1 with Cargo jobs=2. Real desktop/Fcitx5/AMD acceptance remains separate and NOT RUN. Restarting the entire Ptyra process is necessary to use its new embedded entry; opening a new Tab in an old running process does not update the executable.
- Final owned native matrix PASS: actual ncurses/default-color/16-color pixels on combined, isolated and rebuilt release binaries; CJK/combining/missing-glyph fonts, SGR styles, cursor/blink/partial damage, scale/output migration, configured palette/OSC precedence and ordinary keyboard/reconnect on combined and isolated binaries. Summary `/tmp/ptyra-htop-native-checks.log`; detailed `/tmp/ptyra-htop-{combined,clean}-{terminfo,styles,scale,palette}.log` and `/tmp/ptyra-htop-release-terminfo.log`. Final real htop captures on both rebuilt binaries were inspected. The throwaway harness reused the filename `before.png`; the preserved pre-fix files are `baseline-before.png` (21:04 +08:00), while `/tmp/ptyra-htop-diagnosis/debug-current/before.png` (21:13:17, SHA-256 `9c98b2ad3e0af4639c2aa4ee248b099ab973b5eb9ae6aa2e32a3148ba86c8dbf`) and `/tmp/ptyra-htop-diagnosis/release-current/before.png` (21:13:25, SHA-256 `76e6342979d52fd03acf4229cb52e63a4ed2f34c24c59f0f40f72b76b485b34b`) are the actual post-fix screenshots. Both restore missing labels, bars and gray percentages with the same user font-only config, no palette override. Exact synthetic color samples also agree between debug and release (`/tmp/ptyra-htop-actual-after.log`).
- The old release artifact had mtime 2026-11-06 18:05:32 +08:00 and SHA-256 `00b889bbb2f1448ad87397a0f0df9a1db9e76421b37c01df73b7274da901937d`; its synthetic DIM pixels differed from current source. The rebuilt `target/release/ptyra` has mtime 2026-11-06 21:08:44 +08:00 and SHA-256 `f8436e4a4871a8a1d59afadf88fe8babcd385c8b0a7cf3751a6616c988b41e20`, verified with the fixed embedded entry. Combined binaries include the existing uncommitted T129 implementation. These artifact checks do not identify which binary any pre-existing user desktop process is running. No user process was restarted.


## TASK-181 — encoder coverage contract

- The complete ready-to-negotiate subset is **bit 1 only** (disambiguation), exported as `READY_KEYBOARD_FLAGS`. Press and repeat have identical bytes; release is suppressed. `KeyboardProtocol` and the live encoder still use zero. TASK-182 may enable only this ready subset; TASK-183 owns enhanced native IME/shortcut acceptance. Existing TASK-111 bit-2 event primitives remain accepted and tested separately, but are **not ready for negotiation**. Bits 4/8/16, every combination containing them, and unknown bits fail atomically.
- Verified 2026-11-06 against the [published Kitty protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/), its [reference encoder](https://raw.githubusercontent.com/kovidgoyal/kitty/master/kitty/key_encoding.c), [reference tests](https://raw.githubusercontent.com/kovidgoyal/kitty/master/kitty_tests/keys.py), and [XKB adapter](https://raw.githubusercontent.com/kovidgoyal/kitty/master/glfw/xkb_glfw.c). Literal fixtures cover every accepted functional identity: navigation, F1–F35, Print/Pause/Menu, available media keys, numeric/operator/navigation keypad keys and aliases. XKB AudioPlay means MEDIA_PLAY. No XKB identity is invented for MEDIA_PLAY_PAUSE or MEDIA_REVERSE; private-use Unicode text stays text.
- Printable keys preserve their actual shifted UTF-8 on press/repeat; release emits nothing. Non-text reports use the unshifted identity, including shifted media/function/control keys and Ctrl/Alt/Super/Hyper/Meta combinations; resolved keypad navigation retains XKB NumLock/Shift selection. Actual printable text is preserved even when the base is functional or unknown. Non-text repeat/release adds `:2`/`:3` with bit 2. Without bit 2, repeat equals press and release is suppressed. Plain/lock-only Enter/Tab/Backspace retain reset-friendly bytes and suppress release; modified forms, including Shift+Enter, have identical CSI press/repeat bytes with ready bit 1, and CSI release only in the unready bit-2 primitive. Non-text keypad identities are distinct with bit 1; bit 2 alone uses normal-key equivalents. Enhanced modes ignore legacy application cursor/keypad encoding. Lock state is included in CSI, not raw text. Modifier-only keys (including locks and ISO level switches) stay silent without bit 8.
- Bit-2 primitive limitations, not part of the ready subset: the published text-event rule suppresses printable releases, unlike current Kitty XKB reference tests that emit a CSI release for a textless release event. Bit-2-only Escape repeats/releases carry CSI event fields, unlike the reference's early raw-Escape shortcut. Modified reset-control releases follow the reference's distinction from plain reset keys. More decisively, current modifiers cannot identify a previously encoded press: Ctrl+A followed by releasing Ctrl before A, or Shift+Enter followed by releasing Shift before Enter, loses the matching key release. Independent review identified this and `kitty_event_primitive_is_not_ready_without_press_ownership` records it. Stateful press ownership must be completed before bit 2 can join the ready subset. No existing TASK-111 test was removed or weakened; this is not a claim of byte-identical Kitty behavior or full protocol support.
- Hyper/Meta come from distinct named XKB modifier bits after excluding conventional Shift/Ctrl/Lock/Alt/Super and mapped NumLock; overlapping Hyper takes precedence over Meta. Aliases do not duplicate Alt/Super. Named Hyper/Meta state is ignored in legacy mode, preserving its established bytes; the prior legacy Super restriction remains. Effective depressed/latched/locked state is used, and all eight protocol bits fit the maximum encoded value 256. Missing named mappings mean no invented Hyper/Meta state. Existing XKB multi-symbol keys and unmapped non-text symbols remain explicitly unsupported rather than partially written. IME commits remain a separate UTF-8 owner.
- Test-first failure: `/tmp/ptyra-t181-red.log` rejects the first enhanced keypad Home fixture before implementation. `kitty_supported_flags_match_complete_encoder_coverage` and companion fixtures cover each accepted key family, all modifier bit combinations, flags 0/1/2/3, press/repeat/release, actual XKB Shift/Unicode/remapped locks/aliases, unsupported combinations, and truthful zero query replies. A second red fixture, `/tmp/ptyra-t181-lock-escape-red.log`, exposed missing CapsLock on a bit-2 Escape press; the primitive now preserves it. A third red fixture, `/tmp/ptyra-t181-unshifted-red.log`, reproduced Shift+Play incorrectly reporting Pause; two-level media/function/control/text/keypad fixtures now verify canonical identity and preservation of actual text. Full verification results are recorded below after final checks.

- 2026-11-06 resume：核對 `ccd68a7` 同中斷前 baseline 一致。三個 TASK-181 source/test files 與先前獨立 checkout 逐 byte 相同；TASK-129 七個既有改動檔案嘅 SHA-256 全部吻合保留紀錄。今次只補驗證、TASK 狀態同 commit，冇再改 encoder。
- 重新執行目標 test：PASS。完整 workspace 首次喺 sandbox 內因 `UnixStream::pair()` 回傳 EPERM 而 FAIL；sandbox 外重跑 PASS：311 unit tests、13 integration tests、2 doctests。另明確執行 ignored fish regression：PASS。Format、Clippy `-D warnings`、`git diff --check`：PASS。全部 Cargo checks 用 `--locked --offline`，CPU affinity 0–1、build jobs=2、test threads=2。
- 先前相同 source 嘅獨立 checkout 驗證：307 unit tests、13 integration tests、2 doctests、額外 fish regression、Format、Clippy 同 build 全部 PASS。Log：`/tmp/ptyra-t181-checks.log` 同 `/tmp/ptyra-t181-{combined,clean}-test_--workspace.log`。今次冇重跑已通過而 source 未變嘅 native matrix。
- 先前 owned headless Sway matrix 喺 combined／獨立 binary 全部 PASS：Kitty query bursts/pressure、失敗 Tab 隔離、PTY backpressure、configure recovery、fonts/styles/cursor、scale、keyboard/keypad/focus/mouse、history/selection、Clipboard/Primary、Tabs 同 pending-paste cancellation。Combined binary 嘅 private Fcitx5 倉頡 preedit/commit、focus/restart，同 IME backpressure 亦 PASS。Logs：`/tmp/ptyra-t181-final-native.log`、`/tmp/ptyra-t181-final-{combined,clean}-*.log`；呢啲結果唔代替真 desktop acceptance。
- Release build、native terminfo/keyboard/query pressure 同兩種 stream-failure probes 先前全部 PASS；今次核對 `target/release/ptyra` SHA-256 仍為 `86b0100ebde0c7134bbcdde8da3f3898c893c108461881315a25832cd154dcf8`。Logs：`/tmp/ptyra-t181-final-release-build.log`、`/tmp/ptyra-t181-final-release-native.log`、`/tmp/ptyra-t181-final-release-failure-{osc,combining}.log`。

```text
TASK: TASK-181
Status: DONE
Dependencies: satisfied — TASK-108, TASK-110, TASK-111, TASK-164
Changed files: src/keymap.rs, src/kitty/encode.rs, src/kitty/encode_coverage.rs,
  TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline kitty_supported_flags_match_complete_encoder_coverage
Result: PASS
Workspace tests: PASS — 311 unit, 13 integration, 2 doctests；額外 fish regression PASS
Format / Clippy: PASS
Manual / Integration checks: 相同 source 嘅 owned headless Sway/private Fcitx5 matrix PASS
  真 desktop、enhanced runtime Kitty、AMD/Mesa acceptance：NOT RUN
Security / Privacy impact: 冇新增 dependency、unsafe、payload logging 或系統設定改動
Known limitations: 只有 bit 1 ready；bit 2 欠 press/release ownership；其他 flags 拒絕
  Runtime 仍宣告 0 flags，negotiation 屬 TASK-182；TASK-129 保持未提交及未勾選
Ready for [x]: YES — 沿用已批准嘅 checkbox 更新同逐項 commit
```

## TASK-182 — Runtime Kitty negotiation（2026-11-07）

- 預設保持 0 flags；app 要求 enhancement 後，只接受 TASK-181 已驗證嘅 bit 1。要求 31 flags 時，query 如實回覆 1。Runtime press／repeat 每次直接讀取 active Tab 嘅當前 mode；冇新增另一份 cached flags。Shift+Enter 喺 bit 1 下輸出 `CSI 13;2u`，未 opt-in 時維持 legacy bytes。
- 沿用每 Tab 嘅 main／alt stacks 同 32-entry 上限；set／add／remove、push／pop、screen switch 同 RIS reset 共用同一 state。未完成嘅 bits 2／4／8／16 不參與 negotiation，release 維持不輸出。Protocol 依據：[Kitty keyboard progressive enhancement](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#progressive-enhancement)。
- `runtime_kitty_negotiation_controls_active_encoder` 使用固定 font/XKB fixtures、無 PTY 嘅 Tab registry 同 fake clock。逐 byte feed 驗證 query 同 actual encoder bytes、malformed mode、main／alt 隔離、mode 改變後嘅 repeat、切 Tab 取消 repeat、hidden Tab negotiation、close 同新 Tab reset。先測試失敗：`/tmp/ptyra-t182-red.log` 顯示要求 bit 1 後仍回覆 0；接線後 PASS。
- 更新已有 query fixtures，讓 negotiated flags 同 encoder 一致；native raw-child fixture 結束前重設 mode。新 native probe 逐步核對 Ctrl／Alt keys、Shift+Enter、F1、arrows、keypad Enter、Esc、普通文字、repeat 同 consumed shortcuts；兩個真 PTY 驗證 hidden Tab mode 改變唔影響 active Tab。首次 native run 嘅 Kitty assertions 已 PASS，但舊 harness 假設全程只發生一次 Copy；改為核對該步驟增加一次後，整套重跑 PASS。
- Combined workspace：312 unit tests、13 integration tests、2 doctests PASS；ignored fish regression 另行執行 PASS。獨立 checkout 排除 TASK-129：308 unit tests、13 integration tests、2 doctests，加跑 fish regression 全部 PASS。兩份 source 嘅目標 test、Format、Clippy `-D warnings` 同 build 均 PASS。Cargo 使用 `--locked --offline`、CPU affinity 0–1、jobs/test threads=2。Logs：`/tmp/ptyra-t182-{focused,workspace,fish,fmt,clippy}.log` 同 `/tmp/ptyra-t182-clean-*.log`。
- Combined／獨立 debug binaries 喺 Ubuntu 24.04.4、owned headless Sway 1.9 + pixman 通過新 Kitty probe、legacy keypad、query flood/backpressure、4096-query bursts、keyboard/focus/reconnect 同 font pixels。Combined 額外通過 Clipboard／Primary／Selection／Tabs matrix；private Fcitx5 5.1.7 通過既有 legacy-mode 倉頡 preedit、一次 UTF-8 commit、focus、restart regression。Logs：`/tmp/ptyra-t182-native-final.log`、`/tmp/ptyra-t182-clean-native.log`、`/tmp/ptyra-t182-clipboard.log`、`/tmp/ptyra-t182-ime.log`。
- Release build 同 native Kitty／query／ordinary keyboard probes PASS；`target/release/ptyra` SHA-256：`d878d4369b13b04130e6ca1d56052240ebd97b79011bf72ce028310e5c92a1d3`。Logs：`/tmp/ptyra-t182-release-build.log`、`/tmp/ptyra-t182-release-native.log`。冇重新啟動使用者現有 Ptyra。
- 冇新增 dependency、unsafe、FFI、payload logging 或系統設定改動。TASK-129 獨立檔案逐 byte 保留；共用 runtime/keyboard 檔案嘅原有 IME hunks 亦核對相同，唔包含喺本次 commit。未執行真 Sway 1.10.1 desktop、AMD/Mesa 或 enhanced multi-Tab IME acceptance；TASK-175／183 同其他 gates 保持未完成。

```text
TASK: TASK-182
Status: DONE
Dependencies: satisfied — TASK-112, TASK-113, TASK-130, TASK-133, TASK-180, TASK-181
Changed files: src/kitty/{encode.rs,encode_coverage.rs,protocol.rs}, src/terminal.rs,
  src/runtime.rs, src/runtime/{keyboard.rs,kitty_tests.rs,failure_tests.rs},
  src/pty/write/tests.rs, src/terminfo.rs,
  tests/{native_kitty.py,native_replies.py,native_runtime.py}, TASKS.md, VERIFICATION.md
Unit test: cargo test --workspace --locked --offline runtime_kitty_negotiation_controls_active_encoder
Result: PASS
Workspace tests: PASS — combined 312 unit；獨立提交 308 unit；各有 13 integration、2 doctests
  ignored fish regression 兩份 source 均另行執行 PASS
Format / Clippy: PASS
Manual / Integration checks: 隔離 Sway Kitty/legacy probes、Clipboard/Primary/Tabs、legacy Fcitx5 PASS
  release build/native probes PASS；真 desktop/enhanced multi-Tab IME acceptance NOT RUN
Security / Privacy impact: 只協商已驗證 bit 1；原有 resource limits、Tab ownership 同 logging policy 保留
Known limitations: bits 2/4/8/16 不支援 negotiation；未宣稱完整 Kitty Keyboard 支援
  TASK-129 保持未提交及未勾選；enhanced IME ownership 屬 TASK-183
Ready for [x]: YES — 沿用已批准嘅 checkbox 更新同逐項 commit
```
