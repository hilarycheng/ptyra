# Ptyra

> **開發中 · 狀態核對：2026-11-13**

Ptyra 係一個開發中嘅 Rust Native Wayland Terminal，支援單視窗 GUI Tabs、Shell、Clipboard 同 Fcitx5 輸入。

目標係低 RAM、低 Idle CPU 同穩定日常使用。Kitty Keyboard／Graphics 已接入有限 subset，未宣稱完整兼容。

## Planning Files

- [`SCOPE.md`](SCOPE.md) — 定義產品、架構、安全、性能與驗收邊界。
- [`TASKS.md`](TASKS.md) — **199 個 Atomic TASK**，用 Checkbox 記錄狀態，設有 Dependencies、Unit Test、Done Criteria。
- [`AGENTS.md`](AGENTS.md) — Coding Agent 必須遵守嘅執行、測試、安全同批准規則。

## Status

- `TASKS.md` 係唯一 TASK 狀態來源。規格文件嘅原始 Planning Draft 字句唔代表目前實作進度；批准規則仍按 `AGENTS.md` 執行。
- 199 項 TASK 已全部勾選。TASK 範圍同整體 acceptance gates 分開計，完整產品驗收仍未完成。
- 已有基礎型別、事件來源/reducer、測試用 effect recorders、clock/error、Wayland request/state models、座標/damage/frame models、Shell argv/environment 建立器、有界 XKB keymap 轉換、純邏輯 key-repeat scheduler，以及真正 Linux PTY spawn adapter、有界 output byte 讀取、per-session input byte queue、PTY row/column resize、單次 child exit → Tab event mapping，以及保留跨 chunk parser state 嘅 terminal core adapter 同 renderer 資料接口。
- `src/main.rs` 會載入 config，再連接 Wayland compositor。Runtime 用有界 SHM buffers、software glyph rasterization、真 PTY 同阻塞 poll event loop；keyboard 經現有 focus/keymap/encoder/repeat 模組送到 Shell。
- 已驗證 Enter、Backspace、arrows、Ctrl+C、focus/repeat、PTY resize 同 close cleanup。IME、Clipboard、Selection、GUI Tabs 已接入 runtime；個別 native probes 唔代表整體 desktop acceptance 完成。
- Kitty Graphics direct PNG/RGBA 已接入 runtime；TASK-184–190 驗證同支援範圍見文末。整體 Delivery Gates 仍以 `TASKS.md` 為準。
- Child 用 `TERM=ptyra` 同私有 `TERMINFO` directory，唔需要預先全域安裝 entry。Cursor 支援 Block／Beam／Underline 同 app-requested blink。已有 CJK fallback、combining、styles、palette 同 integer scale；完整 shaping、指定 desktop HiDPI 同效能仍待驗收。
- Unit/fake tests 驗證 library 行為，唔代表 Sway、Fcitx5、PTY 或日常 Terminal 驗收通過。完整產品仍須通過 `SCOPE.md` 嘅 Delivery Gates。
- 不會改動現有 Sway、Fcitx5、Mesa 或 Terminal 設定。

## 使用同 Config

Build／run 指令見下方。直接指定完整 config：

```sh
./target/release/ptyra --config /absolute/path/to/ptyra/assets/config.default.toml
```

[完整 defaults](assets/config.default.toml) 同 [部分設定例子](assets/config.example.toml) 都有 parser regression。
未指定 `--config` 時，程式先用 absolute `$XDG_CONFIG_HOME/ptyra/config.toml`；否則用 `$HOME/.config/ptyra/config.toml`。
檔案不存在或欄位未指定時使用 defaults。設定只喺啟動時載入，冇 live reload。
檔案須為 UTF-8 TOML，最多 64 KiB；未知欄位、無效型別、超出範圍或重複 shortcuts 都會拒絕啟動。
Error 只顯示 path／field／reason 等 metadata，唔輸出 config 值，亦唔改寫原檔。

| 設定 | Default | 可用範圍／用途 |
|---|---|---|
| `font_family` | `monospace` | 非空 Fontconfig family／pattern，最多 256 bytes，禁止 control characters |
| `font_size` | `14.0` | 1–256 logical pixels |
| `scrollback_lines` | `5000` | 每 Tab 0–10000 行；另受總 cell budget 限制 |
| `clipboard_bytes` | `16777216` | Receive／paste limit，1–16777216 bytes；超出會拒絕，唔截斷 |
| `osc52_write` | `false` | 明確 opt-in 先容許 PTY output 寫 Clipboard；read 仍拒絕 |
| `glyph_cache_bytes` | `16777216` | 共用 glyph cache，0–16777216 bytes；0 停用 cache |
| `image_bytes` | `16777216` | 1–16777216 bytes；每次 transfer、decoded image 同每 Tab store 限額 |
| `image_pixels` | `4194304` | 1–4194304 pixels／image |
| `[colors]` | 見 defaults | `foreground`、`background`、`cursor`，值必須係 `#RRGGBB` |
| `[colors.ansi]` | 見 defaults | 16 個具名 ANSI colors；未指定嘅顏色保持 default |

其他固定上限包括 64 Tabs、每 Tab 32768 visible cells／2000000 logical cells，同全部 Tabs 8000000 logical cells。
實際 allocation 另有保守 budget，所以未到 Tab 數目上限亦可能拒絕新增或 resize。
Graphics 共用 128 MiB peak reserve；每 Tab 最多 64 images／256 placements。
Font discovery 最多保留 16 faces、合共 48 MiB font bytes；glyph cache 最多 4096 entries。
呢啲係個別資源限制，唔係整個 process 嘅 RSS 保證。

| `[bindings]` action | Default shortcut |
|---|---|
| `copy`／`paste` | `Ctrl+Shift+c`／`Ctrl+Shift+v` |
| `new_tab`／`close_tab` | `Ctrl+Shift+t`／`Ctrl+Shift+w` |
| `next_tab`／`previous_tab` | `Ctrl+Page_Down`／`Ctrl+Page_Up` |
| `scroll_page_up`／`scroll_page_down` | `Shift+Page_Up`／`Shift+Page_Down` |
| `scroll_top`／`scroll_bottom` | `Shift+Home`／`Shift+End` |
| `ime_toggle` | `Ctrl+space`，保留畀 compositor／Fcitx5 |

Shortcut 用 `Ctrl`／`Shift`／`Alt`／`Super` 加 XKB keysym name，各 action 不可重複。
`ime_toggle` 只保留 ownership，唔會替你改 Fcitx5 hotkey；外部設定要配合。
Tab bar 可點擊切換；最後一個 Tab 關閉時視窗退出。Hidden Tabs 繼續處理 PTY output。
Mouse 支援拖曳、double／triple click selection；app 啟用 mouse reporting 時，Shift 可選擇本機 selection。

## Clipboard 同中文輸入

Copy 保留 Unicode、空格同 hard newline；soft wrap 唔額外加 newline。冇 selection 時唔會清空外部 Clipboard。
Middle-click 使用獨立 Primary Selection，須 compositor 支援；唔會改用普通 Clipboard。
Pending paste 綁定原本 Tab；切 Tab、失焦或關 Tab 會按現有 focus policy 取消，唔會轉送另一個 Shell。
App 開啟 bracketed paste 時使用 framing；未開啟而內容有多行時，先顯示確認。
確認提示顯示後，Enter 接受、Esc 取消。Bracketed paste 唔保證 Shell 唔執行內容。
預設拒絕 OSC 52 read／write。`osc52_write = true` 會容許該 process 嘅 PTY output 改 Clipboard，仍有大小同 target 檢查。

IME 用 `zwp_text_input_v3`；Sway 同 Fcitx5 嘅 `input-method-v2` 由外部處理。
Preedit 只係 overlay；commit 經 transaction 同 focus／Tab 檢查後先輸入 PTY。
毋須設定 `GTK_IM_MODULE`。Ptyra 唔會修改全域 IME environment。

## 安裝同移除

目前提供 Linux GNU library adapter，**未有 install／uninstall CLI 或持久 receipt**。
`InstallPlan::user_local(home).install(binary)` 寫入 `home/.local`；`InstallPlan::staged(root).install(binary)` 寫入 `root/usr/local`。
兩者只建立 `bin/ptyra`、`share/terminfo/p/ptyra` 同 `share/applications/ptyra.desktop`。
目的檔案已存在會拒絕；唔會覆寫舊版本。Prefix 要由 operator 控制，須為 UTF-8，唔可以包含 `=`、`%` 或 control characters。
Installed launcher 使用 absolute binary path，已測空格、引號、dollar、backtick 同 backslash；staged 目錄搬位後要重新建立 entry。

成功 install 會回傳 `InstallReceipt`。同一個 caller 保留 receipt，再用 `plan.uninstall(&receipt)` 移除。
Uninstall 只接受相同 plan 嘅完整 receipt；檔案被改動、替換或變成 symlink 時保留並回報 error。
已消失嘅檔案可重試；parent directories 同其他程式保留。Install 途中失敗亦只回滾本次建立嘅檔案。
錯誤包含 path 同 error kind；若檔案留喺 private recovery directory，先檢查回報位置。
Receipt 遺失後，現有 API 唔會憑三條 path 猜測 ownership，亦唔提供跨 process 自動 uninstall。
`system_local()` 只供檢視 `/usr/local` 路徑；install／uninstall 會拒絕執行，真正系統寫入或移除仍需另行批准。

可重跑嘅暫存 packaging 驗證：

```sh
cargo test --workspace --locked --offline packaged_capabilities_match_runtime_support
PTYRA_TEST_PACKAGING_NATIVE=1 cargo test --workspace --locked --offline packaged_capabilities_match_runtime_support -- --nocapture
```

第一行需要 ncurses `tic`；第二行另需 Sway、swaymsg、Fontconfig、Python 3、python3-gi／GIO 同 desktop-file-validate。
Native probe 只開 owned headless session，唔需要安裝到真正 user prefix。詳細結果見 [verification](VERIFICATION.md)。

## 能力同未驗收項目

| 區域 | 目前實作 | 驗收界線 |
|---|---|---|
| Terminal／TUI | ANSI core、alt screen、queries、navigation／keypad、mouse／focus reporting | Bundled terminfo 保守宣告 16 colors／256 pairs；完整 vim／htop／tmux／Codex 日常矩陣未完成 |
| Tabs／Clipboard | GUI Tabs、獨立 PTY／history／selection、copy／paste／Primary、pending target 隔離 | Firefox ↔ Ptyra ↔ Kitty desktop 全矩陣未完成 |
| Fcitx5 | Preedit／commit、focus／Tab epochs、candidate rectangle | 使用者已確認 TASK-129／176；完整指定平台、長組字、scale、restart gate 仍未完成 |
| Renderer | SHM software、CJK fallback、combining、styles／colors、cursor blink、integer scale | 複雜 shaping／fractional scale 未宣稱；AMD／Mesa desktop 校準未完成 |
| Kitty Keyboard | Opt-in flag 1、query／stack／screen ownership、Shift+Enter | Bits 2／4／8／16 未宣告；唔係 full Kitty |
| Kitty Graphics | Direct RGBA32／PNG100、有限 placement／delete、bounded decode／store | File／temp／SHM、animation、image history 等未支援；詳見 TASK-184–190 紀錄 |
| Packaging | 暫存 user-local／staged install、launcher／terminfo、receipt uninstall | Receipt 只存記憶體；真正 system install／remove 未執行 |
| Performance／stability | 固定資源量測、renderer replay、EGL/GLES PoC、多 Tabs soak runner、runtime boundary corpus | 完整 desktop backend 決定同長時間驗收未完成；固定 fixtures 唔係全面 fuzz |

單 Tab 80 MiB PSS、額外 Tab 15 MiB、idle 單核 CPU <1% 都係設計目標。
已有同機 offscreen backend probe；完整 desktop／其他 Terminal 比較未完成，唔宣稱效能優勢。
整體 gates 以 [TASKS.md](TASKS.md) 未勾選清單為準；headless／unit PASS 唔代表指定 desktop 驗收通過。
下面有日期嘅 verification sections 係當時紀錄，舊限制唔代表目前能力。

## TASK-191 Verification（2026-11-13）

`InstallPlan::user_local(home).install(binary)` 安裝到 `home/.local`。
`InstallPlan::staged(root).install(binary)` 安裝到 `root/usr/local`。
兩者都安裝 binary、bundled terminfo 同 desktop entry，成功後回傳 `InstallReceipt`。
`system_local()` 只提供可審核路徑，執行會回傳 PermissionDenied。

Adapter 逐層用 directory FD 同 `O_NOFOLLOW`，目的檔案用 `O_EXCL` 建立。
失敗會反向回滾本次檔案；回滾前核對 inode，唔刪 replacement。
回滾失敗會回報 path 同 error kind。既有及新建 directory 都保留。
Prefix 必須由 operator 控制；同 UID 惡意 concurrent writer 唔屬隔離範圍。
目前提供 library adapter，receipt 只存記憶體；TASK-192 已加入 receipt uninstall，見上方說明。

```text
TASK: TASK-191
Status: DONE
Dependencies: satisfied (TASK-122, TASK-124)
Changed files: src/install.rs, src/install/adapter.rs,
  src/install/adapter/filesystem.rs, src/install/adapter/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline install_adapter_rolls_back_only_created_files
Result: PASS; existing adapter retained, fake failures and real temporary prefixes verified
Workspace tests: PASS — 317 unit, 13 integration, 2 doc-tests; 1 optional fish test ignored
Format / Clippy: PASS
Manual / Integration checks: temporary user-local/staged prefixes, modes, existing files,
  source limits, symlinks, parent replacement and rollback PASS
Security / Privacy impact: exclusive creation; no new dependency, unsafe or payload logging
Known limitations: Linux GNU; directories remain; in-memory receipt; no system installation
Ready for [x]: YES
Approval: APPROVE FINISH TASK-191, 195
```

Sandbox 嘅 `/tmp` owner mapping 同 Unix socket 限制令第一次全套測試失敗。
同一組測試喺 sandbox 外重跑全部通過，冇放寬 installer 嘅 owner 檢查。

## TASK-196／197 Renderer Replay（2026-11-13）

```sh
cargo build --release --locked --offline --features gles-probe --example renderer_probe
python3 -B tools/renderer_benchmark.py target/release/examples/renderer_probe --backends shm gles --rounds 100 --output /tmp/ptyra-renderers.json
```

`renderer-probe` feature 可以只編譯 SHM；`gles-probe` 另需 system EGL 1.5／GLES 3 libraries。
預設 Ptyra build 同 renderer 不變。冇新增 Cargo dependencies；FFI 評審見 [DEPENDENCIES.md](DEPENDENCIES.md)。
每 round 固定八個 frames，涵蓋 CJK／combining／styles、partial/full damage、scale=1/2 同 resize。
兩條路徑共享 CPU glyph raster；GPU PoC 加 texture upload 同 framebuffer blit，唔係 GPU glyph renderer。
每 frame 都逐 byte 比較 full reference 同 backend readback。

JSON 記錄 fixture／font／binary hashes、driver、CPU/PSS/RSS/GPU 同 frame timings。
資源量測包含 verification；raster／submit timing 分開，GPU submit 包含 `glFinish`。
缺值係 `NOT AVAILABLE`，failure／timeout 回傳非零 exit code，report path 唔會覆寫。
範圍係 offscreen；不包括 Wayland presentation、vsync、Tab bar／selection／IME overlay。
AMD RX 6600／Mesa 26.2.4 各 800-frame replay 一致；正式 backend 選型仍然未完成。

## TASK-198 Soak Runner（2026-11-13）

```sh
cargo build --release --locked --offline
python3 -B tools/soak.py target/release/ptyra --seconds 3600 --seed 198 --tabs 5 --output /tmp/ptyra-soak.json
```

需要 Sway／swaymsg、Fontconfig、xkbcomp、wl-copy／wl-paste、D-Bus、Fcitx5 同 Cangjie3。
只開私有 headless Sway、D-Bus 同 Fcitx5；唔改或操作現有 desktop session。
同一個 Ptyra process 持續運行；固定 seed 排序 output、resize、switch、close/reopen、keyboard、Clipboard 同 IME。
每個 Tab 用固定 raw-PTY workload；input、paste、中文 commit 逐 byte 核對，額外或跨 Tab input 當 failure。

可選 2–10 Tabs，時長 1 秒至 7 日；預設 5 Tabs／1 小時。
報告分開記錄 FAILED／NOT RUN／EXERCISED、停止、timeout、cleanup、每個 process 嘅資源同 FD count。
只保留最近 4,096 個 samples，另列總 sample count，避免長測試令 harness 無界增長。
冇 payload logs；SIGINT／SIGTERM 會停止 runner、清理自己嘅 processes，再保存報告。
短測試只驗證 runner；完整日常操作、長時間 resource trend 同 desktop acceptance 仍要另外驗收。

120 秒、五 Tabs、seed=198 已完成 571 operations，七個場景各有 81–82 次。
106 個 samples 都有五個 children；Ptyra FD count 首／尾／peak 都係 11，最後 cleanup 通過。
PSS 由約 6.54 MiB 升至 21.61 MiB；包括首次 history/cache 分配，唔代表已排除長期 leak。
完整結果同限制見 [VERIFICATION.md](VERIFICATION.md)。

## TASK-199 Runtime Boundary Corpus（2026-11-13）

```sh
cargo test --workspace --locked --offline runtime_malformed_events_remain_bounded_and_isolated
```

固定 seed `0x50545952` 重播兩次，核對相同結果。
覆蓋 seat/device generation、serial 消耗／wrap、stale offers、FD cancellation／EINTR／UTF-8／限額、IME stale done 同 paste epoch。
真 `TabRegistry::drain_ready` 路徑加入 query flood、錯誤 source／長度、oversized OSC 同 random chunks。
核對每次 dispatch 上限、input queue／cell caps、closed Tab 不再 dispatch，同健康 Tab grid/input 保持獨立。
呢組係 deterministic adapter fixtures；唔包含惡意 Wayland wire decoder、kernel FD race 或無限輸入嘅完整 fuzz 證明。

## TASK-195 Resource Harness（2026-11-13）

```sh
cargo build --release --locked --offline
python3 tools/benchmark.py target/release/ptyra --output /tmp/ptyra-benchmark.json
```

需要 Python 3.11+、Sway、`swaymsg` 同 Fontconfig；harness 只開自己嘅 headless session。
報告用 exclusive creation，唔覆寫既有檔案。`--seconds 1 --warmup 0.2` 可做短 smoke check。
正式預設係每組預熱 5 秒，再量測 60 秒；固定 1／5／10 Tabs。
Window 係 1920×1080、scale=1、bundled synthetic fonts、font size=20、scrollback=1,000 行。
呢個 scrollback 設定令 10 Tabs 保持喺現有總 cell budget 內，唔代表預設 5,000 行配置。
每 Tab 輸出 200 行固定 ASCII/CJK/combining fixture，收到 terminal reply 後 idle，cursor 隱藏。

JSON schema v1 包含 UTC timestamps、OS/kernel/Sway、CPU affinity、source revision/dirty state、
binary version/hash、font/harness hashes、完整 benchmark config 同逐次 samples。
Ptyra 同每個直接 child 分開記錄；Python workload child 唔計入 Ptyra 數值。
CPU 係 `delta(utime + stime) / CLK_TCK / elapsed × 100`，唔除以 CPU 數目。
欄位依 [Linux proc stat](https://www.man7.org/linux/man-pages/man5/proc_pid_stat.5.html) 定義；唔加入 child CPU counters。
PSS/RSS 來自 `smaps_rollup`，保存 sample arithmetic mean 同 peak。
GPU 使用 [DRM fdinfo](https://www.kernel.org/doc/html/latest/gpu/drm-usage-stats.html) 嘅 resident memory，依 device/client ID 去重。
跨 client 共享 allocation 仍可能重複計入；compositor memory 唔會歸入 Ptyra。

`PTYRA_BENCHMARK_FRAMES=1` 只喺 runtime 關閉時輸出一次版本同 submitted buffer count。
Frame count 包含 startup、warmup 同量測期間，**唔係 idle 期間嘅 frame rate**。
缺值一律係 `NOT AVAILABLE`；process set 改變、退出或 setup 失敗會記錄 ERROR，同回傳非零 exit code。
報告嘅 assessment 一律係 `NOT EVALUATED`，冇自動判定性能達標。
Synthetic fonts + headless/pixman 結果唔代替指定 desktop、AMD/Mesa、renderer 比較或 soak 驗收。

```text
TASK: TASK-195
Status: DONE
Dependencies: satisfied (TASK-132, TASK-137, TASK-177)
Changed files: tools/benchmark.py, tests/benchmark.rs, tests/benchmark_samples.py,
  src/runtime.rs, src/runtime/benchmark.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline benchmark_samples_report_single_core_cpu_and_pss
Result: PASS — 4 deterministic Python fixture checks; test-first failed before harness existed
Workspace tests: PASS — 318 unit, 14 integration, 2 doc-tests; 1 optional fish test ignored
Format / Clippy: PASS
Manual / Integration checks: release build PASS; 1/5/10 Tabs, 5-second warmup,
  60-second samples recorded; final harness smoke and owned child cleanup PASS
Security / Privacy impact: metadata only, opt-in frame count; no new dependencies or unsafe
Known limitations: synthetic fonts, 1,000-line history, Sway 1.9 headless/pixman;
  GPU memory NOT AVAILABLE; frame count includes startup; no performance acceptance claim
Ready for [x]: YES
Approval: APPROVE FINISH TASK-191, 195
```

本機量測環境：Ubuntu 24.04.4、kernel 7.0.0-38、Sway 1.9 headless/pixman。
每組有 61 次 samples；下表只列 Ptyra，child 數值獨立保存在 JSON。

| Tabs | 平均 PSS | 平均 RSS | 單核 CPU | Lifetime frames | GPU memory |
|---|---:|---:|---:|---:|---|
| 1 | 14.66 MiB | 16.46 MiB | 0.0% | 3 | NOT AVAILABLE |
| 5 | 31.27 MiB | 33.07 MiB | 0.0% | 11 | NOT AVAILABLE |
| 10 | 51.86 MiB | 53.66 MiB | 0.0% | 21 | NOT AVAILABLE |

0.0% 表示量測期間冇觀察到新增 CPU ticks，唔係保證零 CPU。
完整報告喺 `target/benchmarks/task-195-2026-11-13.json`；最後版本嘅短測試喺 `target/benchmarks/task-195-final-smoke.json`。
兩份報告各自記錄執行時嘅 harness hash；`target/` 係本機產物，唔會提交入 Git。
Sway 1.10.1 真 desktop launch：PASS；完整 Firefox/Kitty/Fcitx5 acceptance 未完成。GPU memory unavailable；一小時 headless soak 喺 2,624 秒因 reopen timeout 終止，詳見 `VERIFICATION.md`。

## Build / Run

需要支援 Rust 2024 edition 嘅 toolchain 同 linker。Runtime 需要 Linux PTY、Wayland compositor（xdg-shell／wl_shm）、Fontconfig `fc-match`、可用本機字型同 Shell。
本機 GNU build 動態連接 glibc 同 libgcc_s；Fontconfig 工具有自己嘅 native libraries。Ptyra 唔係零 native dependencies。
中文輸入另需 compositor 配合 Fcitx5；明確開啟 hyperlink 時會呼叫 `xdg-open`。UI runtime 冇用 GTK、Qt、Electron、WebView 或 XWayland。

```sh
cargo build --release --locked
./target/release/ptyra
```

程式用 `SHELL` 指定嘅 absolute executable，未設定時用 `/bin/sh`。`exit` 或 compositor close 會關閉視窗及回收 child。
可用 `--config /absolute/path/config.toml`；冇 compositor、無法載入字型或啟動 Shell 時會顯示 metadata error 同非零 exit code。

原生整合測試（需要 Sway、grim、xkbcomp、tic；只建立獨立 headless session）：

```sh
python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-native.png
```

測試透過 virtual-keyboard protocol 輸入固定指令，核對真 Shell 執行、編輯鍵、Ctrl+C、repeat/focus、實際 compositor pixels 同退出。
呢個測試唔會修改使用者 desktop 設定，亦唔代替 Fcitx5/Clipboard/多 Tabs 或 AMD GPU 真機驗收。
字型 dependency 有已知維護風險，見 [dependency review](DEPENDENCIES.md)；只應使用可信本機字型。

### TASK-125–128 Verification（2026-11-05）

- Status：DONE；Dependencies satisfied；使用者已批准實作、`[x]` 同逐項 git commit。
- 每個指定 unit test 都先記錄失敗，再實作：`runtime_window_waits_for_configure`、`runtime_raster_draws_terminal_cells`、`runtime_pty_output_updates_grid`、`runtime_keyboard_routes_to_shell`。
- `cargo test --workspace <test_name>`：四項 PASS。最終 workspace：189 unit tests、13 integration tests、2 doctests PASS。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`git diff --check`、desktop-file-validate：PASS。
- `cargo build --release --locked --offline`：PASS。Debug 同 release executables 都通過 `tests/native_runtime.py` 原生互動測試。
- 額外 headless Sway checks：視窗 mapped、idle 阻塞、ASCII/CJK screenshot、PTY resize、close child cleanup：PASS。
- Security / Privacy：冇新增 app unsafe、Shell command 拼接或 payload logs；有界 buffer/font/keymap/cache，重用現有 PTY ownership。字型 parser 維護風險見 DEPENDENCIES.md。
- Manual / Acceptance：使用者實際 Sway/Fcitx5、Firefox Clipboard、多 Tabs、GPU/HiDPI、soak/benchmark：NOT RUN。Ready for `[x]`：YES，只限四項列明範圍。

## Runtime regression fixes（TASK-126 / TASK-128）

- Keymap FD 改用 positional read，由 offset 0 讀取而唔改共享 file offset；包含 dup FD、非零 offset、短檔案 regression tests。
- 已移除 held-key 去重。一次 press/release 只編碼一次；正常 hold repeat 仍由 RepeatScheduler 處理。
- Focus 中依 terminal cursor shape 畫 Block、Beam 或 Underline；失焦畫空心框；Hidden 保持隱藏。Focus events 會觸發 redraw。
- 192 unit tests、13 integration tests、2 doctests、Format/Clippy 同 release build PASS。Bash 同 fish（no-config）各自通過原生互動及同 compositor 連續三次啟動測試。
- Fish 原生測試：`PTYRA_TEST_FISH=1 python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-fish.png`。
- 另以 `PTYRA_TEST_FISH=configured` 跑現有 fish 設定（private mode）原生測試：指令內容、互動輸入及重新啟動均 PASS。呢個模式會載入使用者 fish startup files。
- 使用者診斷顯示每次 press 只寫入一個 byte，冇額外 repeat；cursor 卻隨 Shell output 向下移動。
- 已用 fish 固定兩行 prompt 重現：`TERM=dumb` 令重畫留喺新行，畫面出現多份同一個字。改用既有 Ptyra terminfo 後，cursor 留喺原行，grid 只保留一個字。
- `fish_multiline_prompt_redraw_keeps_one_character` 先 FAIL，再 PASS；屬需要 `/usr/bin/fish` 嘅 integration test，明確以 `cargo test --test pty_spawn fish_multiline_prompt_redraw_keeps_one_character -- --ignored` 執行。
- Terminfo compiled entry 隨 binary 內嵌，執行時只寫入每個 session 嘅 0700 暫存 directory；正常退出會清理。唔需要先安裝 terminfo，亦冇改全域設定。
- 診斷可用 `PTYRA_TRACE_INPUT=1 ./target/release/ptyra`；只印 press/release/repeat、queued/written byte 數量同 cursor 位置，預設關閉。唔記錄 keycodes、文字或 PTY payload。
- 本次檢查：193 unit tests、13 一般 integration tests、2 doctests PASS；fish regression 額外明確執行並 PASS。Format、Clippy、release build、Bash／configured fish 原生互動及連續三次啟動 PASS。
- 使用者重開新版後，實體 keyboard 輕按一次 `a`，確認「正常，只出一個 a」。Fcitx5／完整 TUI 驗收不在本次結果內。

## TASK-129 — Native Fcitx5 input（2026-11-05）

- 使用者已批准實作。Runtime 綁定 seat-scoped `text-input-v3`，focus 控制 enable/disable；content purpose 為 Terminal。
- Ctrl+Space 由 compositor/Fcitx5 處理；Ptyra 保留收到嘅普通 keyboard events。Preedit 只畫 overlay；`done` 後先將 UTF-8 commit 送入原本 PTY，一個 batch 只送一次。
- 失焦清除 pending/preedit；同一 serial 可以帶多個有效 batches。舊 serial 仍套用文字，但暫停 compositor state updates，直到收到 matching `done`。
- Cursor rectangle 跟 typing/resize 更新。Preedit 重用 glyph cache，支援 CJK width、combining marks、underline 同 byte-offset caret/selection；目前只畫一行，超出右邊會 clip。
- Native test 使用獨立 headless Sway、private D-Bus 同臨時 Fcitx5 倉頡設定。D-Bus 停用 service auto-start；唔連接現有 desktop，唔改使用者 IME 設定。Protocol trace 只包含測試產生嘅固定文字，測試結束會清理。

```sh
python3 tests/native_ime.py target/release/ptyra /tmp/ptyra-ime.png
python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-keyboard.png
```

```text
TASK: TASK-129
Status: TODO（實作及隔離 integration 完成；等實際 desktop 驗收同 checkbox 批准）
Dependencies: satisfied — TASK-069–082, TASK-126, TASK-128
Changed files: src/runtime.rs, src/runtime/ime.rs, src/runtime/keyboard.rs,
  src/runtime/raster.rs, src/policy_tests.rs, tests/native_ime.py,
  Cargo.toml, Cargo.lock, TASKS.md, README.md, DEPENDENCIES.md
Unit test: cargo test --workspace --locked --offline runtime_ime_commits_to_shell
Result: PASS（先有缺少實作嘅 FAIL；pending batch focus regression 亦先 FAIL 再 PASS）
Workspace tests: PASS — 195 unit tests, 13 integration tests, 2 doctests
  fish_multiline_prompt_redraw_keeps_one_character 額外用 --ignored 執行：PASS
Format / Clippy: PASS；git diff --check：PASS
Manual / Integration checks: release build PASS；隔離 Sway 1.9 + Fcitx5 5.1.7：PASS
  Ctrl+Space、Cangjie preedit、恰好一次「日」UTF-8 commit、cursor/resize、focus reset：PASS
  IME 停止時英文輸入、IME restart 後重新 focus 再輸入中文：PASS
  release native keyboard、repeat/focus、連續啟動三次、PTY output：PASS
  screenshot 已核對 preedit；候選 popup 未顯示，不能當作候選視窗驗收 PASS
Security / Privacy impact: 無新增 package/version、app unsafe、payload logs 或系統設定修改
Known limitations: 實際 Sway 1.10.1/Fcitx5、候選視窗位置、HiDPI、長組字 wrapping 未驗收
Ready for [x]: NO — 等實際 desktop 驗收同使用者批准狀態更新
```

Protocol 依據：[Wayland text-input-v3](https://github.com/wayland-mirror/wayland-protocols/blob/main/unstable/text-input/text-input-unstable-v3.xml)。
[Fcitx Sway 說明](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland#Sway) 列明 Sway 1.10 起支援 text-input-v3 candidate popup。

### TASK-129 完成核對（2026-11-13）

使用者批准「APPROVE FINISH TASK-129」，並確認第一個 Tab 嘅中文切換、候選位置、resize、單次 commit 同切返後輸入全部正常。新 Tab 中文輸入失效已確認係固定 `TabId(1)` 嘅限制，保留喺 TASK-175。整體多 Tabs / IME acceptance gate 未勾選。

本次將 preedit bounds 接入現有 damage tracking，令冇 PTY output 時組字仍會重畫，失焦後亦會還原舊 overlay。新增 `runtime_ime_preedit_damage_restores_on_focus_loss`，先記錄缺少 damage 方法嘅編譯 FAIL，再實作並 PASS。Native fixture 改為等齊 Fcitx table、classicui 同 dbusfrontend addons，避免初始化未完成就輸入。

```text
TASK: TASK-129
Status: DONE（只限單 Tab）
Dependencies: satisfied — TASK-069–082, TASK-126, TASK-128
Changed files: src/runtime.rs, src/runtime/ime.rs, src/runtime/keyboard.rs,
  src/runtime/raster.rs, tests/native_ime.py, src/policy_tests.rs,
  DEPENDENCIES.md, README.md, TASKS.md
  既有 src/install.rs / src/install/adapter*.rs / src/install/adapter/*.rs 只整理格式
Unit test: cargo test --workspace runtime_ime_commits_to_shell
Result: PASS；runtime_ime_preedit_damage_restores_on_focus_loss 亦 PASS
Workspace tests: PASS — 314 unit、13 integration、2 doctests
  fish_multiline_prompt_redraw_keeps_one_character 保持 ignored，本次 NOT RUN
Format / Clippy: PASS；git diff --check PASS
Manual / Integration checks:
  cargo build --locked --offline PASS
  Ubuntu 24.04.4 / owned headless Sway 1.9 / Fcitx5 5.1.7：native_ime.py PASS
  cursor/resize、倉頡 preedit、單次 UTF-8 commit、focus reset、IME restart PASS
  native_runtime.py：keyboard/repeat/focus、PTY output、重開及退出 PASS
  使用者 desktop 單 Tab 驗收：使用者回覆全部正常；desktop 版本未獨立核實
Security / Privacy impact: 冇新增 package/version、app unsafe、payload logs 或全域設定修改
  已核對既有 install adapter 嘅 nix/user 用途，補齊 dependency allowlist
Known limitations: 新 Tab 中文輸入仍未接通（TASK-175）；長 preedit／viewport 見 TASK-176
Ready for [x]: YES — 依使用者完成授權及單 Tab 驗收確認
```

首次完整 regression 因既有 install 改動新增 `nix/user` 而未更新 allowlist 失敗；第一次 format 亦因 install 格式失敗。補齊 invariant 同格式後，完整 workspace checks 全部通過。另以 `d749eb2` 加五個 TASK-129 runtime/native-test 檔案建立隔離 snapshot，313 unit、13 integration、2 doctests、Format 同 Clippy 亦 PASS；冇將隔離結果冒充當時 workspace PASS。

### TASK-175 完成核對（2026-11-13）

新 Tab 無法輸入中文嘅原因係 runtime 只對 `TabId(1)` 啟用 IME，preedit owner 亦固定喺第一個 Tab。現時按有效 keyboard focus 建立 overlay owner，沿用現有 switch/close focus hooks 同 enable/disable transactions。Focus 改變即取消 pending batch 同舊 overlay；`done` serial 必須屬於目前 focus transaction，避免切 Tab 後先到達嘅舊 batch 寫入新 Shell。同一 focus 內較舊嘅 cursor-update serial 仍可提交文字。

```text
TASK: TASK-175
Status: DONE
Dependencies: satisfied — TASK-093, TASK-129, TASK-133, TASK-134
Changed files: src/runtime/ime.rs, src/runtime.rs, tests/native_ime.py,
  README.md, TASKS.md
Unit test: cargo test --workspace runtime_ime_tab_switch_invalidates_old_batches
Result: PASS — 先因非 TabId(1) 無 Enable request 而 FAIL，再完成實作
  mock sequence 覆蓋新 Tab、切換、關閉／失焦、延遲 batch、preedit owner、單次 commit
  同一 focus 舊 cursor serial 仍有效；舊 focus serial 唔清除目前 overlay
Workspace tests: PASS — 315 unit、13 integration、2 doctests
  fish_multiline_prompt_redraw_keeps_one_character 保持 ignored，本次 NOT RUN
Format / Clippy: PASS；git diff --check PASS；debug build PASS
Manual / Integration checks:
  python3 -B tests/native_ime.py target/debug/ptyra /tmp/ptyra-task175-ime.png --tabs
  PASS — owned headless Sway 1.9 / Fcitx5 5.1.7 / Ubuntu 24.04.4
  保持中文組字新增 Tab、新舊 Tab commit、切返、關閉組字中 Tab、重開皆 PASS
  各 Shell 固定輸出檔案核對精確 UTF-8；舊組字取消，冇跨 Tab 輸入
  TASK-129 單 Tab／IME restart regression 亦 PASS，與以上多 Tab 結果分開報告
  native_runtime.py 普通 keyboard/repeat/focus、PTY output、重開及退出 PASS
  使用者 desktop 多 Tab 驗收：PASS（2026-11-13 使用者確認「已解決，新舊 Tab 都正常」）
  涵蓋保持中文模式新增 Tab、輸入、切返第一個 Tab、關閉組字中 Tab；desktop 版本未獨立核實
Security / Privacy impact: 無新增 dependencies、unsafe、payload logs 或全域設定修改
Known limitations: 長 preedit／viewport anchor 屬 TASK-176；Kitty/IME ownership 屬 TASK-183
Ready for [x]: YES — 使用者已批准 FINISH，指定 unit 同真多 Tab integration 已通過
```

首輪 native test 喺 compositor 啟動階段 timeout；fixture 現時先等 Sway IPC readiness，再連接 virtual keyboard，修正後完整 native test PASS。整體指定 desktop acceptance matrix 保持未勾選。

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

## TASK-048 Verification (2026-10-30)

```text
TASK: TASK-048
Status: DONE
Dependencies: satisfied (TASK-047 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline mime_prefers_utf8_text -- --test-threads=1
Result: PASS; test-first compilation failed before select_text_mime existed
Workspace tests: PASS — 109 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland MIME offer/Clipboard NOT RUN
Security / Privacy impact: metadata-only, bounded MIME choice; no payload, I/O, logging, unsafe or dependency
Known limitations: text/plain fallback still requires UTF-8 validation by the later reader; non-UTF-8 STRING is rejected
Ready for [x]: YES
Approval: user approved TASK-048 checkbox and scoped local commit; no push
```

`select_text_mime` returns the exact advertised MIME spelling for Wayland
receive. It prefers explicit UTF-8 text, then UTF8_STRING, then text/plain;
unsupported charset and non-text formats are rejected.

## TASK-049 Verification (2026-10-30)

```text
TASK: TASK-049
Status: DONE
Dependencies: satisfied (TASK-047, TASK-011 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_serial_requires_input_event -- --test-threads=1
Result: PASS; test-first compilation failed before SeatSerialState existed
Workspace tests: PASS — 110 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland seat/Clipboard set_selection NOT RUN
Security / Privacy impact: serial metadata only; no clipboard payload, logging, I/O, unsafe or dependency
Known limitations: trusted seat adapter must supply actual key/button event serial; no wl_data_device.set_selection execution or compositor acceptance yet
Ready for [x]: YES
Approval: user approved TASK-049 checkbox and scoped local commit; no push
```

`SeatSerialState` yields a selection serial only after a key press or pointer
button press for the focused seat, window and Tab. A mismatched event, focus
change or leave invalidates pending use; a serial is consumed once. Numeric
zero remains valid because Wayland serials may wrap.

## TASK-050 Verification (2026-10-30)

```text
TASK: TASK-050
Status: DONE
Dependencies: satisfied (TASK-048, TASK-049 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline data_source_advertises_text_mime -- --test-threads=1
Result: PASS; test-first compilation failed before DataSourceRequest existed
Workspace tests: PASS — 111 unit + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland data-source creation and Clipboard NOT RUN
Security / Privacy impact: fixed metadata-only request plan; no payload, I/O, logging, unsafe or dependency
Known limitations: adapter execution and compositor acceptance remain untested; local ownership is not asserted by planning
Ready for [x]: YES
Approval: user approved TASK-050 checkbox and scoped local commit; no push
```

`plan_data_source` emits Create, two text MIME advertisements and
SetSelection in order, only with a fresh matching seat serial. Planning
consumes that serial but does not claim compositor success.

## TASK-051 Verification (2026-10-30)

```text
TASK: TASK-051
Status: DONE
Dependencies: satisfied (TASK-050 已 [x])
Changed files: src/clipboard.rs, src/clipboard/write.rs, src/clipboard/write/tests.rs, tests/clipboard_fd.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_write_handles_short_write -- --test-threads=1
Result: PASS; test-first compilation failed before ClipboardSend existed
Workspace tests: PASS — 112 unit + 1 real Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real pipe FD bytes PASS; real Wayland Clipboard NOT RUN
Security / Privacy impact: owned payload has no Debug; 16 MiB input limit, bounded per-call writes, no payload logging or unsafe
Known limitations: event-loop writable readiness and Wayland send/FD lifecycle not wired; peer-close path verified with fake writer
Ready for [x]: YES
Approval: user approved TASK-051 checkbox and scoped local commit; no push
```

`ClipboardSend` retries EINTR within a bounded call, retains bytes on
WouldBlock or partial writes, and releases payload on completion or terminal
write error. A real Linux pipe test confirms Unicode bytes reach an FD.

## TASK-052 Verification (2026-10-30)

```text
TASK: TASK-052
Status: DONE
Dependencies: satisfied (TASK-047 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline data_offer_tracks_formats -- --test-threads=1
Result: PASS; test-first compilation failed before DataOffer existed
Workspace tests: PASS — 113 unit + 1 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real wl_data_offer event and Wayland Clipboard NOT RUN
Security / Privacy impact: bounded MIME metadata only, no payload logging, I/O, unsafe or dependency
Known limitations: Wayland adapter must map offer object IDs and coordinate DataOffer/ClipboardState cancellation; FD receive is later work
Ready for [x]: YES
Approval: user approved TASK-052 checkbox and scoped local commit; no push
```

`DataOffer` records up to 64 distinct advertised MIME strings, each at most
256 bytes, and exposes the preferred text MIME. Cancellation drops formats
and rejects later advertisements.

## TASK-053 Verification (2026-10-30)

```text
TASK: TASK-053
Status: DONE
Dependencies: satisfied (TASK-052, TASK-048 已 [x])
Changed files: src/clipboard.rs, src/clipboard/read.rs, src/clipboard/read/tests.rs, tests/clipboard_fd.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_read_handles_partial_utf8 -- --test-threads=1
Result: PASS; test-first compilation failed before ClipboardReceive existed
Workspace tests: PASS — 114 unit + 2 real Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real pipe FD UTF-8 PASS; real Wayland Clipboard NOT RUN
Security / Privacy impact: no payload Debug/logging; bounded per-call reads and 16 MiB hard safety cap, no unsafe or dependency
Known limitations: no Wayland offer.receive/request/FD lifecycle wiring; explicit Paste limit policy is TASK-054
Ready for [x]: YES
Approval: user approved TASK-053 checkbox and scoped local commit; no push
```

`ClipboardReceive` retains partial UTF-8 bytes through interrupted or
nonblocking reads, then validates and returns text only at EOF. Invalid UTF-8
or fatal I/O releases the buffer. A Linux pipe test exercises the real FD path.

## TASK-054 Verification (2026-10-30)

```text
TASK: TASK-054
Status: DONE
Dependencies: satisfied (TASK-053 已 [x])
Changed files: src/clipboard/read.rs, src/clipboard/read/tests.rs, tests/clipboard_fd.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_read_limit_is_explicit -- --test-threads=1
Result: PASS; test-first compilation failed before with_limit existed
Workspace tests: PASS — 115 unit + 3 real Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real pipe FD limit rejection PASS; real Wayland Clipboard/GUI NOT RUN
Security / Privacy impact: explicit 1..=16 MiB Paste byte limit; overflow returns FileTooLarge and drops payload without logging or truncation
Known limitations: application config and Wayland offer/FD lifecycle are not wired; 16 MiB is a hard ceiling, not a measured usage budget
Ready for [x]: YES
Approval: user approved TASK-054 checkbox and scoped local commit; no push
```

`ClipboardReceive::with_limit` rejects invalid settings. Reads that cross
the selected byte limit return a Clipboard `FileTooLarge` error and close
the receiver; a transfer exactly at the limit remains valid.

## TASK-055 Verification (2026-10-30)

```text
TASK: TASK-055
Status: DONE
Dependencies: satisfied (TASK-053 已 [x])
Changed files: src/clipboard/read.rs, src/clipboard/read/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline clipboard_cancel_drops_pending_transfer -- --test-threads=1
Result: PASS; test-first compilation failed before ClipboardTransfer existed
Workspace tests: PASS — 116 unit + 3 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland offer invalidation and GUI Paste NOT RUN
Security / Privacy impact: cancellation drops owned reader and buffered text; no payload logging or unsafe
Known limitations: adapter must call read_ready with current offer and owner before each read; runtime routing is pending
Ready for [x]: YES
Approval: user approved TASK-055 checkbox and scoped local commit; no push
```

`ClipboardTransfer` owns one offer's reader. A cancelled offer or changed
owner closes that reader and discards pending bytes before another read.

## TASK-056 Verification (2026-10-30)

```text
TASK: TASK-056
Status: DONE
Dependencies: satisfied (TASK-046, TASK-050 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline copy_shortcut_requires_selection -- --test-threads=1
Result: PASS; test-first compilation failed before copy_shortcut existed
Workspace tests: PASS — 117 unit + 3 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real keyboard shortcut and Wayland Clipboard NOT RUN
Security / Privacy impact: CopyIntent text has no Debug; invalid/missing selection creates no intent or ownership
Known limitations: caller must supply focused target, selection and Grid; config bindings and runtime data-source dispatch remain pending
Ready for [x]: YES
Approval: user approved TASK-056 checkbox and scoped local commit; no push
```

`copy_shortcut` creates a CopyIntent only for pressed Ctrl+Shift+C with a
valid nonempty selection. Release, repeat, other keys and invalid selection
produce no Copy request.

## TASK-057 Verification (2026-10-30)

```text
TASK: TASK-057
Status: DONE
Dependencies: satisfied (TASK-052, TASK-011 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline paste_shortcut_routes_to_focused_tab -- --test-threads=1
Result: PASS; test-first compilation failed before PasteIntent/PasteKeyAction existed
Workspace tests: PASS — 118 unit + 3 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real keyboard/Wayland Paste NOT RUN
Security / Privacy impact: matching shortcut is consumed; stale focus emits no PasteIntent or Shell text; no payload logging or I/O
Known limitations: caller supplies translated keysym/modifiers; config bindings and runtime keyboard routing remain pending
Ready for [x]: YES
Approval: user approved TASK-057 checkbox and scoped local commit; no push
```

`paste_shortcut` produces a PasteIntent for the current focused Tab only on
pressed Ctrl+Shift+V. A matched release, repeat or stale-focus event is
consumed without forwarding shortcut bytes to the Shell.

## TASK-058 Verification (2026-10-30)

```text
TASK: TASK-058
Status: DONE
Dependencies: satisfied (TASK-053, TASK-023 已 [x])
Changed files: src/clipboard.rs, src/clipboard/read/tests.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline paste_preserves_utf8_and_newlines -- --test-threads=1
Result: PASS; test-first compilation failed before PastePayload existed
Workspace tests: PASS — 119 unit + 3 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland Paste/PTY end-to-end NOT RUN
Security / Privacy impact: PastePayload has no Debug/logging and enforces 16 MiB maximum; bytes are unchanged
Known limitations: no runtime enqueue/chunk scheduler, target binding or bracketed-paste policy yet
Ready for [x]: YES
Approval: user approved TASK-058 checkbox and scoped local commit; no push
```

`PastePayload` takes completed Clipboard UTF-8 text and exposes the identical
byte sequence for later PTY routing. The test spans split multibyte characters,
tabs, spaces, LF and CRLF without normalization.

## TASK-059 Verification (2026-10-30)

```text
TASK: TASK-059
Status: DONE
Dependencies: satisfied (TASK-057, TASK-002 已 [x])
Changed files: src/clipboard.rs, src/clipboard/tests.rs, src/pty.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline paste_never_switches_target_mid_transfer -- --test-threads=1
Result: PASS; test-first compilation failed before PasteSink existed
Workspace tests: PASS — 120 unit + 3 Linux pipe FD + 9 Linux PTY integration + 2 doc-tests
Format / Clippy: PASS — cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked --offline -- -D warnings
Manual / Integration checks: real Wayland Paste/PTY end-to-end NOT RUN
Security / Privacy impact: pending payload has no Debug/logging; only its initiating Tab's PTY queue accepts chunks
Known limitations: no runtime Paste event-loop wiring or close/focus cancellation; bracketed-paste policy remains separate
Ready for [x]: YES
Approval: user approved TASK-059 checkbox and scoped local commit; no push
```

`PendingPaste` keeps the initiating TabId while queuing bounded chunks. Each
chunk checks the receiving Tab's identity; a queue error retains that chunk for
retry. Switching focus cannot retarget an existing pending Paste.

## TASK-176 — IME viewport / candidate anchor（2026-11-13）

Runtime 共用 Preedit layout、damage 同 candidate anchor。長 Preedit 使用單行水平移動，保留 caret 可見；需要時向左移動 overlay，唔切半 CJK glyph。Selection 仍用 UTF-8 byte offsets，combining marks 跟隨前一個字。Cursor 離開 scrollback viewport 時隱藏 overlay；返到可見範圍先恢復。Candidate rectangle 包含 Tab Bar origin，再轉成 surface-local units；origin/scale 改變會更新 cache。

```text
TASK: TASK-176
Status: DONE
Dependencies: satisfied (TASK-076, 077, 129, 137, 158, 167, 175)
Changed files: src/ime.rs, src/ime/cursor.rs, src/runtime/ime.rs,
  src/runtime/ime/layout.rs, src/runtime/raster.rs, tests/native_ime.py, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_ime_anchor_tracks_viewport_and_long_preedit
Result: PASS; first registered run failed before compose existed
Workspace tests: PASS — 319 unit, 14 integration, 2 doc-tests; 1 optional fish test ignored
Format / Clippy: PASS
Manual / Integration checks: release build PASS; Sway 1.9 headless/pixman + Fcitx5 5.1.7
  resize/scale, preedit/commit, focus, restart, new/switch/close/reopen Tabs PASS
  desktop acceptance: user confirmed TASK-176 OK on 2026-11-13
Security / Privacy impact: bounded 4 KiB preedit; no new dependencies, unsafe or payload logs
Known limitations: one horizontally panned row; desktop versions and scenarios not independently verified
Ready for [x]: YES
Approval: user confirmed "176係 OK" after the desktop acceptance request
```

Sandbox owner/socket restrictions caused three existing tests to fail; the full suite passed outside the sandbox. The first native attempt failed during Sway IPC startup; retry passed. These results do not replace desktop acceptance.

## TASK-183 — Kitty / IME key ownership（2026-11-13）

Compositor 送到 `wl_keyboard` 嘅 key 繼續由 terminal encoder 處理。IME 消耗嘅 key 唔會由 Preedit 重建成 keyboard event；commit 只經 `done` 寫一次 UTF-8。有效 IME batch、enter/leave 同移除 text-input 都會取消舊 local repeat。Keyboard focus/keymap epoch 檢查提前到 shortcut dispatch 前，避免 stale key 開新 Tab。Kitty 仍只宣告已完成嘅 flag 1；冇新增 event-reporting flags。

```sh
python3 tests/native_ime.py target/release/ptyra /tmp/ptyra-ime.png --tabs --kitty
PTYRA_TEST_KITTY=1 python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-keyboard.png
```

```text
TASK: TASK-183
Status: DONE
Dependencies: satisfied (TASK-114, 175, 182)
Changed files: src/runtime/ime.rs, src/runtime/keyboard.rs,
  tests/native_ime.py, tests/native_ime_kitty.py, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_kitty_ime_ownership_never_duplicates_text
Result: PASS — repeat-after-preedit and stale-shortcut regressions first failed, then passed
Workspace tests: PASS — 321 unit, 14 integration, 2 doc-tests; 1 optional fish test ignored
Format / Clippy: PASS; git diff --check PASS
Manual / Integration checks: release build PASS; Sway 1.9 headless/pixman + Fcitx5 5.1.7
  raw PTY probes passed separately for flags 0 and 1: exact Chinese commit, shortcut/release
  suppression, focus/Tab cancellation, IME restart; existing native Kitty negotiation,
  repeat, screen stacks, hidden Tab isolation and keyboard regression PASS
Security / Privacy impact: no new dependencies, unsafe, payload logs or system settings
Known limitations: only flag 1 advertised; headless evidence does not replace desktop acceptance
Ready for [x]: YES
Approval: APPROVE FINISH TASK-183 (2026-11-13)
```

使用者於 2026-11-13 確認「176係 OK, APPROVE FINISH TASK-183」，兩項 checkbox 已更新為 `[x]`。今次只更新文件狀態，冇重跑測試；上面保留實作時嘅實際測試結果。整體 desktop acceptance gates 維持未勾選。

## TASK-184–190 — Direct Kitty Graphics runtime（2026-11-13）

使用者於 2026-11-13 批准「APPROVE FINISH TASK-184 to TASK-190」，七項 checkbox 已更新為 `[x]`。所有 dependencies 已 `[x]`。今次只更新文件狀態，冇重跑測試；下方保留實作時嘅驗證結果。

每 Tab 嘅 APC parser、chunk assembler、image store 同 placements 已接入 PTY output。Malformed APC 會回覆有界 error，再恢復正常 text parsing。Query 只驗證 payload，唔留 image。回覆使用來源 Tab 嘅現有 PTY queue。

支援 direct `t/T/p/q/d`、RGBA32、PNG100、`i/p`、`m/q`、crop `x/y/w/h`、pixel offsets `X/Y`、cell extent `c/r`、cursor policy `C=0/1`。Crop 取 image 交集；cell box 保留 aspect ratio。`C=0` 移動 cursor，超出 grid 時 clamp；`C=1` 保持位置。Negative z 畫喺 glyph 後面，nonnegative z 畫喺 glyph 前面；cursor、selection 同 IME overlay 保持可見。低於 -1073741823 嘅 z 明確拒絕。對照來源：[Kitty Graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)。

Delete subset 係 `a/A/i/I`。Main/alternate placements 分開；進入 alternate screen 會清理舊 alternate placements。圖片只屬 viewport：scroll、line insertion/deletion、scroll-region change、REP/index commands、grid resize 同 clear 會保守淘汰受影響 screen placements。離開 alternate screen 唔會刪除 main placements。仍被另一 screen 引用嘅 image 會保留。RIS 同 Tab close 會釋放全部所屬 state。唔提供 image scrollback 或 resize reflow anchors。

`image_bytes` 同 `image_pixels` 於 decode 前檢查；每 Tab store 同 pending transfer 各受 `image_bytes` 限制。跨 Tab Graphics budget 為 128 MiB，包含 retained capacities、metadata reserve、pending buffer growth 同 decode peak reserve。每次最多 16 KiB PTY feed，只容許 32 MiB encoded-plus-RGBA decode work。每 Tab 最多 64 images、256 placements、8 Mi source-scale composition pixels。Budget 採用保守 peak reserve，可能提早拒絕。呢個 budget 唔係整個 process 嘅 RSS 上限，亦唔係 frame latency 保證。

未支援 file/temp/shared-memory、RGB24、compression、animation、virtual/relative placements 或 background 下方嘅 z layer。未支援 fields/transport 會明確拒絕。冇新增 dependencies、unsafe、payload logs、背景網絡或全域設定。

```sh
cargo test --workspace --locked --offline runtime_graphics_
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --release --locked --offline
PTYRA_TEST_GRAPHICS=1 python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-graphics.png
PTYRA_TEST_KITTY=1 python3 tests/native_runtime.py target/release/ptyra /tmp/ptyra-keyboard.png
```

共同驗證：8 個 runtime Graphics tests PASS；完整 workspace 為 329 unit、14 integration、2 doc-tests PASS，1 個 optional fish test ignored。Fmt、Clippy、release build 同 diff whitespace check PASS。隔離 Sway headless/pixman 通過 byte-split RGBA/PNG、alpha/crop compositor pixels、query/quiet/error replies、main/alternate、hidden Tab isolation、delete/reset/close。Kitty keyboard native regression 亦 PASS。

首個 native pixel probe 因 fixture 冇套用固定 cell font 而失敗；套用現有 font fixture 後通過。Resize lifecycle 補測先失敗，修正 placement 回收及 scroll-region reset 後通過。真 desktop、AMD/Mesa 表現、長時間 Graphics flood 同效能 benchmark 未驗證；唔聲稱完整 Kitty compatibility。

```text
TASK: TASK-184
Status: DONE
Dependencies: satisfied (TASK-115, 121, 132, 183); all [x]
Changed files: src/terminal.rs, src/terminal/tests.rs, src/terminal/graphics.rs, src/kitty/graphics.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_apc_handles_split_streams
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: split APC、ANSI 混合、error recovery；native byte-split probe PASS
Security / Privacy impact: bounded parser；禁止 file/temp/shared-memory
Known limitations: 只處理宣告 direct subset；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-185
Status: DONE
Dependencies: satisfied (TASK-116, 117, 118, 119, 177, 184); all [x]
Changed files: src/terminal/graphics.rs, src/terminal/graphics/tests.rs, src/kitty/graphics/transfer.rs, src/config.rs, src/tabs.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_transfer_decodes_once_with_limits
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: PNG/RGBA、取消、壞 PNG、尺寸 caps、Tab isolation PASS
Security / Privacy impact: decode 前檢查 config caps；冇半張 image 留低
Known limitations: 每 Tab store budget 使用 image_bytes；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-186
Status: DONE
Dependencies: satisfied (TASK-119, 185); all [x]
Changed files: src/terminal/graphics.rs, src/terminal/graphics/tests.rs, src/kitty/graphics.rs, src/kitty/graphics/store.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_placement_uses_current_screen_geometry
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: crop、offset、aspect、cell extent、cursor、alternate screen PASS
Security / Privacy impact: geometry 同 unsupported z layer 有明確拒絕
Known limitations: viewport placements；超出 grid 嘅 cursor movement clamp；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-187
Status: DONE
Dependencies: satisfied (TASK-126, 158, 167, 186); all [x]
Changed files: src/runtime/raster.rs, src/runtime/raster/graphics.rs, src/terminal.rs, tests/native_runtime.py, tests/native_graphics.py, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_pixels_respect_clipping_and_layers
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: alpha、scale、clip、partial rows、viewport offset、z、cursor；native pixels PASS
Security / Privacy impact: 只 compose active screen；placement work 有上限
Known limitations: nearest-neighbour sampling；冇 background 下方嘅 z layer；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-188
Status: DONE
Dependencies: satisfied (TASK-160, 184, 185, 186); all [x]
Changed files: src/terminal/graphics.rs, src/terminal/graphics/tests.rs, src/kitty/graphics.rs, tests/native_graphics.py, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_replies_follow_query_and_quiet_policy
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: query 無 store、quiet 0/1/2、unsupported transport；hidden PTY routing PASS
Security / Privacy impact: 回覆只含 ID 同固定 error metadata，冇 payload
Known limitations: 沿用 bounded core reply queue；唔支援未宣告 commands；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-189
Status: DONE
Dependencies: satisfied (TASK-120, 134, 185, 187); all [x]
Changed files: src/terminal.rs, src/terminal/graphics.rs, src/terminal/graphics/tests.rs, src/kitty/protocol.rs, src/kitty/graphics/store.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_lifecycle_reclaims_owned_resources
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: delete、reset、scroll、wide wrap、resize、drop budget；native screen/Tab cleanup PASS
Security / Privacy impact: 仍被引用嘅 image 保留；cancel/reset/close 回收 pending state
Known limitations: scroll/line operations 保守淘汰；唔保留 image history；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```

```text
TASK: TASK-190
Status: DONE
Dependencies: satisfied (TASK-118, 177, 185, 189); all [x]
Changed files: src/terminal/graphics.rs, src/terminal/graphics/tests.rs, src/tabs.rs, src/kitty/graphics/store.rs, src/kitty/graphics/transfer.rs, README.md, TASKS.md
Unit test: cargo test --workspace --locked --offline runtime_graphics_flood_stays_within_global_budget
Result: PASS
Workspace tests: PASS — 329 unit, 14 integration, 2 doc-tests; 1 optional fish ignored
Format / Clippy: PASS
Manual / Integration checks: shared-budget 拒絕/回收、decode work、64 images/256 placements 上限 PASS
Security / Privacy impact: 128 MiB Graphics peak reserve；decode work 每 feed 有界
Known limitations: 保守 reserve 可能提早拒絕；唔提供 hard latency 保證；共同未驗證項目見上文
Ready for [x]: YES
Approval: APPROVE FINISH TASK-184 to TASK-190 (2026-11-13)
```
