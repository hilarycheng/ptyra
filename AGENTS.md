# Ptyra — AGENTS.md

> **Draft v0.2 · 2026-10-27 · 未 APPROVE。** 呢份文件約束將來所有 Coding Agents。現階段未開始寫應用程式。

## 1. Mission

Ptyra 係一個 **Rust + Native Wayland Terminal Emulator**。目標係低 RAM、低 Idle CPU、長時間穩定、正常 GUI Tabs、可靠 Copy & Paste、Fcitx5 中文輸入。Kitty Keyboard 同 Kitty Graphics 係後續 TASK，唔可以阻止基本 Terminal 交付。

禁止加入 GTK、Qt、Electron、WebView、XWayland 作為 UI runtime。Rust 程式可以使用必要嘅 Linux/Wayland、Font、OpenGL/EGL 等系統 libraries。不得聲稱「完全零 native dependencies」。

## 2. Read Before Work

1. 先讀 `SCOPE.md`，確認功能與安全邊界。
2. 再讀 `TASKS.md`，找出唯一指定嘅 `TASK-XXX`。
3. 檢查列出嘅 Dependencies 是否全部 `[x]`。
4. 如尚未收到使用者對實作嘅明確 `APPROVE`，只可以分析、討論或草擬文件。
5. 未經授權，不可以修改 Ubuntu、Sway、Fcitx5、其他 Terminal 或全域設定。

## 3. TASK Contract

- `TASKS.md` 係唯一 TASK 狀態來源。
- TASK 一律用 `- [ ]` / `- [x]` Markdown checkbox。
- `TASK-XXX` 係永久 ID。不可為咗排序而更改 ID。
- 不使用 Priority / Phase 作 TASK 排序。只依賴 Dependencies 與明確指定嘅 TASK。
- 每次只實作**一個**指定 TASK。唔好順手做下一個 TASK。
- 每個 TASK 要有一個主要行為、一套 deterministic Unit Test、一組 Done Criteria。
- TASK 需要用最少量 code 完成。Avoid unrelated refactoring。
- 無法在一個獨立變更內完成嘅 TASK，先提出拆分方案；未批准前唔擴大 Scope。
- 只在 Dependencies 已完成、Unit Test 通過、Acceptance Criteria 符合及使用者批准狀態更新後，改成 `[x]`。
- Test 失敗、無法執行或只做咗部分功能時，維持 `[ ]`。說明 Blocker。

## 4. Work Sequence for One TASK

1. 記錄 `TASK-XXX`、Dependencies、預期 source files 同 Unit Test name。
2. 用 fake adapter / pure logic 設計可重複嘅 Unit Test。
3. 先寫會失敗嘅測試，再寫最小實作。
4. 執行目標測試：`cargo test --workspace <test_name>`。
5. 執行 Regression：`cargo test --workspace`。
6. 執行 `cargo fmt --all -- --check` 同 `cargo clippy --workspace --all-targets -- -D warnings`（在工具鏈可用時）。
7. 列出改動檔案、實際測試結果、未解決問題。
8. 只在所有檢查符合要求後，申請將 Checkbox 改成 `[x]`。

如果 source 尚未存在，先完成對應 Foundation TASK。唔可以為咗一個高層 TASK，偷做幾十個 Dependencies。

## 5. Architecture / Dependency Boundaries

```text
Wayland compositor / Sway
    ├─ Wayland adapter   → platform events, clipboard, text-input-v3
    └─ Renderer adapter  → surface/frame/output
                         ↓
                 Pure App Event Router
                 ├─ Tab Manager
                 ├─ Clipboard Policy + Routing
                 ├─ IME State Machine
                 ├─ Terminal Core Adapter
                 └─ Render Model
                         ↓
                  PTY adapter / Shell
```

- Platform-specific APIs 必須留喺 adapters。Pure reducers 禁止直接做 Wayland、PTY、FD I/O。
- Unit Test 可以注入 mock Wayland events、PTY streams、fake clock、clipboard offers。
- Core 優先評估 `alacritty_terminal`。唔自行重寫完整 ANSI parser。
- Renderer backend 暫未鎖定。先用 interface 與同機 benchmark 評估 Software / EGL/GLES。
- 一個 Window 可以有多個 Tabs。每 Tab 必須有獨立 `TabId`、PTY、Terminal state、Scrollback 同 Selection。
- 隱藏 Tab 要繼續接收 PTY output，但不需要繪製 UI。
- 唔為單一 TASK 引入大型框架或額外 event-loop thread，除非先證明必要性。

## 6. Clipboard Rules — Mandatory

Clipboard 係核心功能。每次修改 Clipboard、Selection、Focus、IME 或 Tabs，都要重新跑相關回歸測試。

- `Ctrl+Shift+C` 執行 Copy，`Ctrl+Shift+V` 執行 Paste。Shortcut 要可設定。
- 正確處理 Wayland `wl_data_device`、`wl_data_source`、`wl_data_offer`、MIME types 同 seat serial。
- FD 讀寫必須處理 partial read/write、EINTR、EOF、cancellation 同 owner 消失。
- Copy/Paste 要保留 Unicode、繁體中文、空格及真實換行。
- Soft-wrapped line Copy 不可意外加入 newline。
- Clipboard 與 Primary Selection 分開；Middle-click 只喺能力可用時使用 Primary。
- Paste target 必須綁定原本 `TabId`。切 Tab/關 Tab 後，禁止將 pending Paste 寫到另一個 Shell。
- 有 bracketed-paste mode 時遵從 mode。冇 mode 時，多行 Paste 需要明確安全策略。
- Bracketed Paste 不能保證所有 Shell 都阻止危險指令。不得作過度安全承諾。
- OCR、Auto-correct、trim、newline normalize、silent truncation 唔可偷偷修改 Clipboard text。
- `OSC 52` Clipboard Read **預設拒絕**；Write **預設拒絕，只有明確 opt-in 才啟用**。
- 只可以顯示所需嘅 Clipboard error metadata；唔記錄 Clipboard 內容。

## 7. Fcitx5 / Wayland IME Rules — Mandatory

- Ptyra 使用 `zwp_text_input_v3` 與 Sway 互動。
- Sway 與 Fcitx5 嘅 `input-method-v2` 由外部處理。Ptyra 不直接控制該接口。
- 唔依賴 `GTK_IM_MODULE=fcitx`，亦唔修改全域 IME environment。
- `preedit_string` 只更新 UI overlay。唔可以直接寫入 PTY。
- `commit_string` 經 `done` transaction 後，只輸入一次 UTF-8。
- Focus serial / epoch 必須保護 pending events。切 Tab 或失焦不能誤送 Preedit/Commit。
- `delete_surrounding_text` 不可以憑空改 Terminal Grid，亦不可以盲目注入 Backspace。
- Key Release、Repeat、IME consumption 同 Kitty encoder 必須有明確 ownership。
- 每個 IME TASK 都要有 mock event sequence Unit Test。
- 最後仍要喺 **真 Sway + Fcitx5** 測試中文切換及候選字定位。

## 8. Security / Privacy Rules

- PTY output、ANSI/OSC/APC、Hyperlinks、OSC title、Graphics 都係 **untrusted input**。
- 不加入 Telemetry、Analytics、Auto-update checks 或背景對外連線。
- 呢個限制只適用 Ptyra process；Shell 內嘅程式有自己嘅網絡行為。
- 不可以直接將 untrusted output 當 Shell command、file path 或 URL 自動執行。
- 不得新增 `unsafe` 或 FFI，而唔同時記錄安全理由及邊界。
- Input、Output、Scrollback、Glyph Cache、Clipboard Payload、Image Decode 要有合理上限。
- 壞 input 不得造成 panic、OOM 或無界 busy loop。
- 新 Dependencies 要有用途、授權、維護狀態同漏洞檢查；提交 `Cargo.lock`。
- 不記錄 password、PTY text、Clipboard text、Key events 作預設 Log。
- 不使用 `sudo`、`rm -rf`、系統套件移除或修改 `/etc`，除非使用者對該操作明確批准。
- 本專案唔提供 Shell Sandbox，亦唔保證 child process 本身安全。

## 9. Performance / Stability Rules

- 優先 event-driven，禁止無理由固定高頻 redraw。
- 只 Render Active Tab。其他 Tabs 繼續處理 PTY output。
- 避免每 Tab 各自複製大份 Font/Glyph cache。
- 記錄單一核心 CPU usage、PSS、RSS、GPU Memory；不可只看總 CPU 平均。
- 原有 Budget 係設計目標，並非已完成 Benchmark。
- AMD + Mesa + Sway 問題要先重現、記錄 Log，再改 Renderer。
- 不得宣稱新 Terminal 比 Kitty、Ghostty 或 WezTerm 快，除非同機比較。

## 10. Tests: What Counts

| 類別 | 必要結果 |
|---|---|
| Unit Test | 不需要 Sway / Fcitx5；使用 fake adapters；可重複 |
| Integration Test | 需要 Wayland protocol / PTY / Clipboard 真實連接；可自動化時自動化 |
| Acceptance Test | 指定 Ubuntu + Sway + Fcitx5 + Firefox/Kitty 日常操作 |
| Benchmark | 固定 Window、font、tabs、workload；記錄 CPU/PSS/RSS |

**Unit Test 通過唔代表 Fcitx5 真機驗收通過。** 兩者必須分開報告。

### Regression Matrix

| 修改區域 | 最少加跑 |
|---|---|
| Clipboard | MIME、FD I/O、UTF-8、Bracketed Paste、Tab routing |
| Selection | Mouse selection、Soft/Hard wrap、Unicode Copy |
| IME | Preedit/Commit、focus epoch、Tab switching、key ownership |
| Tabs | PTY isolation、Clipboard target、IME target、hidden output |
| Wayland | Focus、serial、configure、scale、frame scheduling |
| Renderer | CJK width、Cursor、Preedit overlay、damage tracking |
| Kitty | Legacy keyboard、IME collision、untrusted graphics payload |

## 11. Required TASK Completion Report

```text
TASK: TASK-XXX
Status: TODO / BLOCKED / DONE
Dependencies: satisfied / not satisfied
Changed files: ...
Unit test: cargo test --workspace <test_name>
Result: PASS / FAIL / NOT RUN
Workspace tests: PASS / FAIL / NOT RUN
Format / Clippy: PASS / FAIL / NOT RUN
Manual / Integration checks: ...
Security / Privacy impact: ...
Known limitations: ...
Ready for [x]: YES / NO
```

唔好將 `NOT RUN` 寫成 `PASS`。未滿足條件時保持 `- [ ]`。
