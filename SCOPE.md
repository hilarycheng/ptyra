# Ptyra — Project Scope

> **Draft v0.2 · 2026-10-27 · 未 APPROVE**  
> 本文件只定義軟件 Scope。未開始 Code、Build、Install 或修改任何 System Settings。  
> 目標平台：Ubuntu 24.04、Sway 1.10.1、Fcitx5、AMD GPU + Mesa。

## 1. Product Definition

**Ptyra** 係一個以 Rust 撰寫嘅原生 Wayland Terminal Emulator。

Ptyra 要解決四個實際問題：

1. **可靠 Copy & Paste**：Firefox、Terminal、Editor 之間複製繁體中文、多行文字唔出錯。
2. **Fcitx5 中文輸入**：正確切換、Preedit、Candidate Position、Commit，唔重複輸入。
3. **真正 Native GUI Tabs**：一個 Window，有 Tab Bar；每個 Tab 有自己嘅 PTY、Shell 同 Scrollback。
4. **低資源、長時間穩定**：Idle 唔忙等；唔依賴 GTK、Qt 或 XWayland GUI runtime。

**Kitty Keyboard** 同 **Kitty Graphics** 都保留喺產品 Scope，但在基本 Terminal、Clipboard、IME 同 Tabs 工作完成後先實作。兩套協定分別驗收，不以一個籠統「Kitty compatible」聲稱代替。

## 2. File Roles and TASK Model

| File | Responsibility |
|---|---|
| `SCOPE.md` | 定義產品邊界、架構、安全策略、驗收需求 |
| `TASKS.md` | 所有 Atomic TASK、Dependencies、Unit Test、Done Criteria、checkbox |
| `AGENTS.md` | Coding Agent 執行、測試、權限、Scope 管理規則 |

- 所有工作用 `TASK-XXX` 追蹤，格式固定為 `- [ ]` 或 `- [x]`。
- TASK ID 唔代表 Priority、Phase 或執行次序。
- 每個 TASK 只實作一個可測試行為，列出明確 Dependencies。
- 每個 TASK 可獨立透過 fake adapter / pure logic 做 Unit Test。
- Unit Test **唔代替** 真 Sway、Fcitx5、Wayland Clipboard Integration Tests。
- 本 Draft 係未批准規格。Coding Agent 未獲使用者 `APPROVE`，唔可以開始實作。

## 3. Scope Boundary

### Included: Foundation + Daily Terminal

- Rust application，Native Wayland connection、xdg-toplevel、seat、keyboard、mouse、frame callbacks。
- PTY、default Shell、terminal parser/core adapter、UTF-8、ANSI/VT、colors、alternate screen。
- Unicode cell width、繁體中文字型 fallback、cursor、selection overlay、bounded scrollback。
- GUI Tab Bar，新增、關閉、切換 Tabs；每 Tab PTY / Grid / Scrollback / Selection 獨立。
- Wayland Clipboard：Copy/Paste、text MIME、FD lifecycle、UTF-8、多行、soft wrap。
- Mouse selection、double/triple click、Primary Selection、middle-click（compositor 支援時）。
- Bracketed Paste、Paste target isolation、multiline paste safety policy。
- Fcitx5 Native Wayland text-input-v3：enter/leave、Preedit、Commit、cursor rectangle、focus/tab ownership。
- TOML config：fonts、shortcuts、scrollback、resource caps、basic colors。
- Runtime：bounded memory、idle redraw suppression、telemetry-free process、diagnostics。
- User-local 安裝及可回滾 `/usr/local` 安裝路徑，不覆寫系統程式。

### Included: Later TASK Groups

- **Kitty Keyboard Protocol**：enhancement flags、query、event encoding、stack、screen-scoped state、Shift+Enter。
- **Kitty Graphics Protocol**：安全、有界 direct transmission、PNG/RGBA、placement、delete、resource limits。
- Kitty Graphics `file`、`temporary file`、`shared memory` 途徑**不在目前交付範圍**；另需安全評審與明確批准。

### Not Included

- GTK / Qt / Electron / WebView / XWayland UI、完整 KDE libraries。
- 自行重寫完整 Shell 或 ANSI parser。
- 自帶 SSH client、Kitty kittens、Kitty remote control、Web browser、插件系統。
- 完整 Kitty private features 兼容聲明。
- 自動恢復已失去 PTY process 嘅 Shell session、跨 Window 移動 Tabs、Splits。
- 改動系統全域 Fcitx5、Sway、Mesa、Firefox 或其他 Terminal 設定。
- Shell child process sandbox；Terminal 不保證 child program 嘅行為或網絡活動。

## 4. Architecture

```text
Sway / Wayland compositor
  ├─ xdg-toplevel / wl_surface / wl_seat
  ├─ wl_data_device / primary-selection (optional)
  ├─ zwp_text_input_v3
  └─ frame callbacks / output scale
                  │
                  ▼
             Wayland Adapter
                  │
                  ▼
            Pure Event Router
    ├─ Clipboard Policy + Transfer State
    ├─ IME State Machine
    ├─ Tabs / Focus Manager
    ├─ Terminal Core Adapter
    ├─ PTY Process Controller
    └─ Renderer / Tab Bar / Overlay
                  │
                  ▼
          Active Tab PTY / Shell
```

平台 I/O 與 pure reducer 分開。平台 API 必須有 fake adapters。呢個設計令每項行為可以獨立 Unit Test。

### Candidate Libraries（待驗證）

| Component | Candidate | Notes |
|---|---|---|
| Wayland | `wayland-client`、`smithay-client-toolkit` | 唔依賴 GTK/Qt |
| Wayland IME | `wayland-protocols` `text_input::zv3` | 必須測試 Sway/Fcitx5 event ordering |
| Event loop | `calloop` 或功能相同方案 | Event-driven，無固定忙等 |
| Terminal Core | `alacritty_terminal` | 不承諾現成 Kitty Graphics support |
| Fonts | Fontconfig + 候選 glyph rasterizer | 需要 CJK fallback、monospace metrics |
| Renderer | `wl_shm` baseline，EGL/GLES 比較 | 由同機 CPU/PSS/穩定性決定 |
| Config | TOML | 不依賴 GUI framework |

Ptyra 主要用 Rust，但可依賴合理嘅 Linux native libraries。唔承諾零 C dependencies。

## 5. Clipboard Contract — Core Function

### 5.1 Copy

- `Ctrl+Shift+C` Copy selected text。冇 selection 時不得清空外部 Clipboard。
- Copy 要支援繁體中文、ASCII、Unicode combining、空格、Tab character。
- Soft-wrapped lines 不加入多餘 newline；Hard line breaks 要保留。
- Wayland `wl_data_source` 要宣告合適 text MIME。
- Clipboard owner 可能消失。Ptyra 唔承諾 owner 退出後 Clipboard 一定保留，因為需要 compositor/clipboard manager 協作。

### 5.2 Paste

- `Ctrl+Shift+V` 使用 Wayland `wl_data_offer` 讀取文字。
- MIME 選擇有固定順序；支援 `text/plain;charset=utf-8` 同可用 fallback。
- FD streaming 支援 partial read/write、EINTR、EOF、cancellation、可設定 byte limit。
- 不自動 trim、不自動補 newline、不靜默截斷。
- Paste 目標綁定發起時嘅 `TabId`。關閉/切換 Tabs 不可誤送其他 Shell。
- Terminal mode 開啟 Bracketed Paste 時按規格處理。
- Terminal mode 冇 Bracketed Paste 時，多行 Paste 需明確安全確認策略。
- Bracketed Paste 唔係 Shell command sandbox，不能保證所有指令都唔會被執行。

### 5.3 Primary Selection and OSC 52

- Primary Selection 同普通 Clipboard 係兩組獨立 state。
- Middle-click Paste 只用 Primary Selection；唔支援該 protocol 時 graceful degrade。
- `OSC 52` Clipboard Read 預設拒絕。
- `OSC 52` Clipboard Write 預設拒絕，明確 opt-in 後先容許。
- 限制 OSC 52 base64 payload 大小，避免惡意 output 消耗無界 RAM。

### 5.4 Clipboard Acceptance

- Firefox → Ptyra → Kitty → Firefox 可以交換繁體中文同多行文字。
- 同時開 5–10 Tabs，Clipboard/Paste 不會串到另一個 Tab。
- Selection、soft wrap、Primary Selection、empty Clipboard、large Paste 有 regression coverage。
- Clipboard owner 消失或 fd 讀寫錯誤後，Ptyra 不 crash。

## 6. Fcitx5 Contract — Native Wayland

- Ptyra 與 Sway 用 `zwp_text_input_v3`。
- Sway 與 Fcitx5 用 compositor/IME 一側嘅 `input-method-v2`。Ptyra 唔直接實作此 protocol。
- 處理 enter、leave、enable、disable、commit request 時序。
- Preedit 係 UI overlay。唔可以直接寫入 PTY。
- `commit_string` 只在相應 `done` transaction 完成後送入正確 Tab PTY。
- 使用 focus epoch / TabId 防止 stale event 導致字元跨 Tab。
- `set_cursor_rectangle` 追隨 Cursor、resize、scroll、scale、Tab 切換。
- 無可靠 editable surrounding text 時唔偽造內容；`delete_surrounding_text` 不可盲目送 Backspace。
- Fcitx5 restart / temporarily unavailable 時，普通英文 keyboard 應繼續正常。

**Release Gate:** 真 Sway + Fcitx5 必須通過中文切換、Preedit、Commit、候選字定位、focus/tab switching。只有 mock Unit Test 不足以宣稱成功。

## 7. Tabs Contract

- GUI 顯示可點擊 Tab Bar；唔用 Sway tabbed layout、tmux 或 Zellij 代替。
- 每個 Tab 擁有獨立 PTY、Shell PID、terminal grid、scrollback、selection、title。
- 一個 Tab 關閉唔可以殺死或污染其他 Tab。
- 隱藏 Tab 仍處理 PTY output，但只 Render active Tab。
- Clipboard, IME, keyboard focus target 都以 `TabId` 做 routing。
- Tab label 來自 untrusted title 時需要控制字元過濾與長度上限。

## 8. Rendering / Resource Budget

- Event-driven rendering；Window 冇變化唔應持續 redraw。
- 可共享 glyph cache；Scrollback、Clipboard、Graphics 都要有上限。
- 字型測試包括繁體中文、double-width、combining marks、cursor alignment。
- Renderer 選型先量度 `wl_shm` 同候選 GPU backend，再決定正式實作。

### Provisional Targets（未實測）

| Metric | Target | Benchmark Context |
|---|---:|---|
| 1 empty Tab PSS | ≤ 80 MiB | 1920×1080、scale=1、同一 font，穩定 60 秒 |
| Additional idle Tab PSS | ≤ 15 MiB | 同一 Window，新開空白 Shell |
| Idle CPU | < 1% of one core | 使用 htop 單核計，不用全 CPU 平均 |
| Scrollback | Default 5,000 lines / Tab | Configurable，具有限額 |
| Idle repaint | No continuous repaint | 除 cursor blink / surface updates |

以上係 Design Targets，唔係保證。必須以用戶實機測試調整並記錄差異。

## 9. Security / Privacy Boundary

| Risk | Mandatory Behavior |
|---|---|
| Telemetry / Analytics | 不加入、不上報 |
| Terminal auto-update network | 不自動連線 |
| Untrusted PTY output | Bounded ANSI/OSC/APC parser；不把 output 當指令 |
| Clipboard read via OSC 52 | 預設拒絕 |
| Clipboard write via OSC 52 | 預設拒絕，需要 opt-in |
| Hyperlinks | 只在用戶明確操作後開啟 |
| File/SHM Kitty Graphics | 當前不支援，不能讀任意本機檔案 |
| Image decode | Byte / Pixel / Cache limits |
| Logs | 預設不包含 Key、Clipboard、PTY 內容 |
| Child process | 不提權；PTY/FD 生命周期正確 |
| Dependencies | `Cargo.lock`、授權/安全性檢查 |

**Rust 不能單靠語言保證所有功能安全。** FFI、Font/GPU libraries、Terminal escape parsing 仍需安全測試。

## 10. Verification Strategy

### Unit Tests

- 每個 `TASK-XXX` 一個明確、確定性 Unit Test 名稱。
- Wayland / PTY / IME 協定事件用 fake adapters 驗證。
- Clipboard 測 MIME、UTF-8、FD partial reads、TabId target、limits、OSC 52 policy。
- IME 測 pending transaction、Preedit、Commit、done、focus epoch、tab reset。
- Tabs 測開/關、獨立 Scrollback、active-only Render、Clipboard/IME routing。
- Rendering 測 CJK width、cell layout、damage、cache eviction。

### Integration / Acceptance

- Ubuntu 24.04 + Sway 1.10.1 + Fcitx5 real sessions。
- Firefox / Kitty ↔ Ptyra Copy & Paste，含繁體中文及多行內容。
- 中文 IME 切換、Candidate placement、Fast key events、Tab switching。
- 多 Tabs + `vim`/`htop`/`tmux`/`Codex` 實際使用。
- Long-running soak test，記錄 Crash、CPU、PSS、RSS 與 GPU resource usage。
- 受信任與不受信任嘅 OSC/ANSI input fuzz tests。

Unit Test PASS 與 Integration Test PASS 要分開報告。未執行嘅測試必須標示 NOT RUN。

## 11. Delivery Gates（依 TASK Dependencies，非 Priority/Phase）

| Gate | Conditions |
|---|---|
| Basic Terminal | Native Wayland、PTY、Grid、Font、Keyboard、Renderer 正常 |
| Clipboard | Copy/Paste、Selection、UTF-8、多行、Tab target、security tests 通過 |
| Fcitx5 | Fake state-machine tests + 真 Sway/Fcitx5 acceptance 通過 |
| Tabs | GUI Tabs、PTY lifecycle、Clipboard/IME isolation 通過 |
| Daily-use | Performance、Security、Stability、Packaging checks 通過 |
| Kitty Keyboard | 相關獨立 TASK + protocol probes 通過 |
| Kitty Graphics | Direct transport、decode、limits、placement、delete probes 通過 |

Kitty 未完成唔應令基本 Terminal 使用受阻。不可喺未完成 Kitty Keyboard 時設定虛假 full compatibility capabilities。

## 12. Architecture Decisions Still Open

- Renderer：`wl_shm` vs EGL/GLES；由同機測量決定。
- Font backend：Rust rasterizer / Fontconfig-compatible 方案；由 CJK correctness、RAM 決定。
- `alacritty_terminal` 版本、公共 API 同 Kitty 擴展點，需 PoC 驗證。
- Clipboard text size default limit 同多行 Paste confirmation UX，需實測後確定。
- Kitty Keyboard / Graphics 嘅可支援子集，按 protocol tests 獨立宣告。

唔可以只因某方案「理論上快」就鎖定技術選型。

## 13. References

- [Kitty Keyboard Protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
- [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol.html)
- [Wayland text-input-unstable-v3](https://wayland.app/protocols/text-input-unstable-v3)
- [Rust wayland-protocols](https://docs.rs/wayland-protocols/latest/wayland_protocols/)
- [Smithay Client Toolkit](https://github.com/Smithay/client-toolkit)
- [alacritty_terminal](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/)
- [Fcitx5 Wayland](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland/en)

---

**Current status:** Planning Draft only。等明確 `APPROVE` 後，先執行指定 Coding TASK。
