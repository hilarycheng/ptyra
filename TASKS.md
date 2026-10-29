# Ptyra — TASKS.md

> **Draft v0.2 · 2026-10-27 · 未 APPROVE；只係規劃文件，未開始實作。**

## Rules / 使用方法

- 每個 TASK 用標準 Markdown checkbox：`- [ ]`；**只有完成測試同驗收後**先改成 `- [x]`。
- `TASK-XXX` 係永久識別碼，**唔係 Priority、Phase 或必須依次執行嘅次序**。
- 每個 TASK 只處理一個可獨立驗證嘅行為；Dependencies 完成後，單獨實作同測試。
- 每個 TASK 都列出一個確定性 **Unit Test** 名稱。平台 API 透過 fake/mock adapter 驗證。
- Sway、Fcitx5、Firefox 互通必須另外進行真機 Integration / Acceptance Tests。Unit Test 不能代替真機測試。
- 完成一項 TASK，必須提交 source、unit test、驗收證據；不得順手修改其他 TASK。
- 每次只把經使用者明確 **APPROVE** 嘅 TASK 交畀 Coding Agent 實作。
- 全部 `[ ]` 起始，表示尚未實作、未通過測試。

**Total:** 124 atomic TASKS。

## A. Foundation / 測試基礎

- [x] **TASK-001 — 建立最小 Rust executable + library**
  - **Dependencies:** —  |  **Unit Test:** `workspace_smoke`
  - **Done:** `cargo test` 可執行一個簡單 unit test；無 GTK/Qt dependency

- [x] **TASK-002 — 建立 `TabId` / `WindowId` 強型別**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `typed_ids_are_distinct`
  - **Done:** 不同 ID 類型不可混用；ID 比較結果正確

- [x] **TASK-003 — 定義 `AppEvent` 與事件來源**
  - **Dependencies:** `TASK-002`  |  **Unit Test:** `app_event_routes_by_source`
  - **Done:** 可區分 Wayland、PTY、IME、Clipboard events

- [x] **TASK-004 — 定義 pure event reducer 與 `AppEffect`**
  - **Dependencies:** `TASK-003`  |  **Unit Test:** `reducer_emits_expected_effect`
  - **Done:** 輸入 event 只產生可核對效果；無隱藏 I/O

- [x] **TASK-005 — 建立測試用 Wayland/PTY fake adapters**
  - **Dependencies:** `TASK-004`  |  **Unit Test:** `fake_adapter_records_calls`
  - **Done:** Unit test 不需要 Sway、Shell 或真 Clipboard

- [x] **TASK-006 — 建立可注入 clock / timer abstraction**
  - **Dependencies:** `TASK-005`  |  **Unit Test:** `fake_clock_advances_deterministically`
  - **Done:** Key Repeat、frame scheduling 可重現

- [x] **TASK-007 — 統一 error 類型與 context**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `error_preserves_context_without_secret`
  - **Done:** 錯誤包含 component 名稱但不包含敏感文字

## B. Native Wayland / Window / Input

- [x] **TASK-008 — 將 registry globals 轉為 `Capabilities`**
  - **Dependencies:** `TASK-005`  |  **Unit Test:** `capabilities_detect_protocols`
  - **Done:** 缺少 protocol 時回報功能不可用，不 crash

- [x] **TASK-009 — 產生 xdg-toplevel 建立 requests**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `toplevel_request_order`
  - **Done:** 可獨立建立 Window 命令序列

- [x] **TASK-010 — 處理 configure + ack state**
  - **Dependencies:** `TASK-009`  |  **Unit Test:** `configure_ack_matches_serial`
  - **Done:** 正確保存 width/height/serial

- [x] **TASK-011 — 處理 wl_keyboard enter/leave 狀態**
  - **Dependencies:** `TASK-002`, `TASK-005`  |  **Unit Test:** `keyboard_focus_transitions`
  - **Done:** 失焦後無 key event 送到 PTY

- [x] **TASK-012 — 轉換 keysym 與實體 key code**
  - **Dependencies:** `TASK-011`  |  **Unit Test:** `keymap_translates_modifiers`
  - **Done:** 保留 Ctrl/Shift/Alt 與 key repeat flag

- [x] **TASK-013 — 純邏輯 key-repeat scheduler**
  - **Dependencies:** `TASK-006`, `TASK-012`  |  **Unit Test:** `key_repeat_stops_on_release`
  - **Done:** 按鍵放開或失焦後停止 repeat

- [x] **TASK-014 — 轉換 pointer 座標及 click event**
  - **Dependencies:** `TASK-010`  |  **Unit Test:** `pointer_coordinates_are_scaled`
  - **Done:** click 路由結果具備 surface/local coordinates

- [x] **TASK-015 — 轉換 logical / physical scale**
  - **Dependencies:** `TASK-010`  |  **Unit Test:** `scale_roundtrip_pixel_bounds`
  - **Done:** scale 變更唔會造成負數尺寸或 overflow

- [x] **TASK-016 — 記錄 dirty rectangles**
  - **Dependencies:** `TASK-010`  |  **Unit Test:** `damage_is_clipped_to_surface`
  - **Done:** 輸出 dirty rect 不超出 Window

- [x] **TASK-017 — frame callback 狀態機**
  - **Dependencies:** `TASK-016`, `TASK-006`  |  **Unit Test:** `frame_callback_does_not_busy_loop`
  - **Done:** 無 dirty state 唔要求新 frame

- [x] **TASK-018 — Wayland 連線中斷事件映射**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `disconnect_reports_typed_error`
  - **Done:** 可清楚停止 render 同回報連線錯誤

## C. PTY / Terminal Core

- [x] **TASK-019 — 建立 Shell argv，不用字串拼 shell command**
  - **Dependencies:** `TASK-007`  |  **Unit Test:** `shell_argv_is_not_reinterpreted`
  - **Done:** 特殊字元維持 argv 元素

- [x] **TASK-020 — 建立 terminal environment map**
  - **Dependencies:** `TASK-019`  |  **Unit Test:** `term_env_matches_supported_features`
  - **Done:** `TERM` / `COLORTERM` 不宣稱未支援能力

- [x] **TASK-021 — 透過 PTY adapter 啟動 Shell**
  - **Dependencies:** `TASK-005`, `TASK-019`, `TASK-020`  |  **Unit Test:** `pty_spawn_failure_is_isolated`
  - **Done:** Spawn 成功/失敗都有 typed result

- [x] **TASK-022 — 讀取 PTY output chunk stream**
  - **Dependencies:** `TASK-021`  |  **Unit Test:** `pty_read_preserves_byte_order`
  - **Done:** 分段讀取唔會遺失或重排 bytes

- [x] **TASK-023 — 向 PTY 送出 byte queue**
  - **Dependencies:** `TASK-021`  |  **Unit Test:** `pty_write_handles_partial_writes`
  - **Done:** partial write 後保留未寫 bytes

- [x] **TASK-024 — 處理 row/column resize**
  - **Dependencies:** `TASK-021`  |  **Unit Test:** `pty_resize_sends_correct_dimensions`
  - **Done:** 只在尺寸有效或改變時送 resize request

- [x] **TASK-025 — 將 child exit 映射到 Tab event**
  - **Dependencies:** `TASK-021`, `TASK-003`  |  **Unit Test:** `pty_exit_does_not_exit_app`
  - **Done:** 一個 child exit 唔影響其他 child

- [x] **TASK-026 — PTY byte stream → terminal core adapter**
  - **Dependencies:** `TASK-022`  |  **Unit Test:** `term_feed_handles_split_utf8`
  - **Done:** 分段 ANSI/UTF-8 input 產生一致 terminal state

- [x] **TASK-027 — 讀取 alt-screen / bracketed-paste mode**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `terminal_modes_track_sequences`
  - **Done:** 可查詢 core 當前 mode，供 clipboard/render 使用

- [x] **TASK-028 — 驗證 CJK、combining 字元 cell width**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `cjk_combining_cells_align`
  - **Done:** 全形中文字/組合字不錯位

- [x] **TASK-029 — 限制每 Tab scrollback lines**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `scrollback_discards_oldest_lines`
  - **Done:** 超額只淘汰最舊行，Memory 有界

## D. Rendering / Font

- [x] **TASK-030 — 定義 renderer 輸入/輸出資料結構**
  - **Dependencies:** `TASK-010`, `TASK-026`  |  **Unit Test:** `renderer_accepts_terminal_snapshot`
  - **Done:** renderer 與 PTY/Wayland I/O 解耦

- [ ] **TASK-031 — cell coordinates 轉 pixel rectangles**
  - **Dependencies:** `TASK-030`, `TASK-015`  |  **Unit Test:** `cell_rect_rounding_is_stable`
  - **Done:** window size 改變後仍不重疊、不 overflow

- [ ] **TASK-032 — 計算 monospace cell metrics**
  - **Dependencies:** `TASK-031`  |  **Unit Test:** `font_metrics_validate_dimensions`
  - **Done:** 無效 font size 會回報錯誤

- [ ] **TASK-033 — 建立缺字 fallback 選擇邏輯**
  - **Dependencies:** `TASK-032`  |  **Unit Test:** `font_fallback_uses_cjk_candidate`
  - **Done:** 繁體中文字可選擇支援嘅 fallback font

- [ ] **TASK-034 — 實作可限定大小嘅 glyph cache**
  - **Dependencies:** `TASK-032`  |  **Unit Test:** `glyph_cache_evicts_by_limit`
  - **Done:** cache 超過 budget 會淘汰舊項

- [ ] **TASK-035 — Grid cell → draw command**
  - **Dependencies:** `TASK-030`, `TASK-034`  |  **Unit Test:** `draw_commands_preserve_colors`
  - **Done:** 文字、前景、背景不交換

- [ ] **TASK-036 — Cursor draw command**
  - **Dependencies:** `TASK-035`  |  **Unit Test:** `cursor_shape_matches_mode`
  - **Done:** Block / Beam / Underline 位置正確

- [ ] **TASK-037 — 獨立 selection/preedit overlay layer**
  - **Dependencies:** `TASK-035`  |  **Unit Test:** `overlay_does_not_change_grid`
  - **Done:** Preedit 顯示但不修改 PTY Grid

- [ ] **TASK-038 — 只繪製可見 dirty content**
  - **Dependencies:** `TASK-017`, `TASK-035`  |  **Unit Test:** `hidden_or_clean_view_skips_draw`
  - **Done:** Idle 不反覆 Render；隱藏 Tab 不 Render

## E. Selection / 文字抽取

- [ ] **TASK-039 — Mouse pixel → terminal cell 映射**
  - **Dependencies:** `TASK-031`, `TASK-014`  |  **Unit Test:** `hit_test_maps_cell_boundaries`
  - **Done:** Click 落在正確 row/col

- [ ] **TASK-040 — Mouse drag selection range reducer**
  - **Dependencies:** `TASK-039`  |  **Unit Test:** `drag_selection_normalizes_direction`
  - **Done:** 由左至右／反向拖曳結果一致

- [ ] **TASK-041 — Double-click word selection**
  - **Dependencies:** `TASK-040`  |  **Unit Test:** `double_click_selects_unicode_word`
  - **Done:** 中文及 ASCII 有清楚 word-boundary 策略

- [ ] **TASK-042 — Triple-click line selection**
  - **Dependencies:** `TASK-040`  |  **Unit Test:** `triple_click_selects_logical_line`
  - **Done:** 完整選取對應 terminal line

- [ ] **TASK-043 — Copy 時合併 soft-wrapped lines**
  - **Dependencies:** `TASK-040`, `TASK-026`  |  **Unit Test:** `copy_soft_wrap_does_not_add_newline`
  - **Done:** 軟折行唔額外加入 `\n`

- [ ] **TASK-044 — Copy 時保留 hard line breaks**
  - **Dependencies:** `TASK-040`, `TASK-026`  |  **Unit Test:** `copy_hard_break_preserves_newline`
  - **Done:** 真實換行保留 `\n`

- [ ] **TASK-045 — Selection 不切開 wide glyph**
  - **Dependencies:** `TASK-040`, `TASK-028`  |  **Unit Test:** `selection_preserves_fullwidth_glyph`
  - **Done:** 選中文字不產生半個 cell 或破字

- [ ] **TASK-046 — Selection 狀態與 Screen/Tab 隔離**
  - **Dependencies:** `TASK-040`, `TASK-002`  |  **Unit Test:** `selection_is_scoped_to_tab`
  - **Done:** 不同 Tab 選區互不影響

## F. Clipboard / Copy & Paste

- [ ] **TASK-047 — 定義 Clipboard state、owner、offer ID**
  - **Dependencies:** `TASK-002`, `TASK-005`  |  **Unit Test:** `clipboard_state_rejects_stale_offer`
  - **Done:** 舊 offer 唔覆蓋新 owner

- [ ] **TASK-048 — 建立 text MIME selection 規則**
  - **Dependencies:** `TASK-047`  |  **Unit Test:** `mime_prefers_utf8_text`
  - **Done:** 優先用 UTF-8 text，缺少時可 fallback

- [ ] **TASK-049 — 處理 Wayland seat serial validity**
  - **Dependencies:** `TASK-047`, `TASK-011`  |  **Unit Test:** `clipboard_serial_requires_input_event`
  - **Done:** 無有效 serial 時不可假裝成功 set_selection

- [ ] **TASK-050 — 建立 wl_data_source request sequence**
  - **Dependencies:** `TASK-048`, `TASK-049`  |  **Unit Test:** `data_source_advertises_text_mime`
  - **Done:** Copy 時對外提供正確 MIME

- [ ] **TASK-051 — 向 Clipboard FD 寫資料**
  - **Dependencies:** `TASK-050`  |  **Unit Test:** `clipboard_write_handles_short_write`
  - **Done:** partial write、EINTR 和 peer-close 安全處理

- [ ] **TASK-052 — 從 wl_data_offer event 建立可接收 offer**
  - **Dependencies:** `TASK-047`  |  **Unit Test:** `data_offer_tracks_formats`
  - **Done:** 能保存 clipboard format / cancellation

- [ ] **TASK-053 — 從 Clipboard FD 讀取文字**
  - **Dependencies:** `TASK-052`, `TASK-048`  |  **Unit Test:** `clipboard_read_handles_partial_utf8`
  - **Done:** 分段 UTF-8 唔會被錯誤切開

- [ ] **TASK-054 — 限制 Paste input size 並回報錯誤**
  - **Dependencies:** `TASK-053`  |  **Unit Test:** `clipboard_read_limit_is_explicit`
  - **Done:** 超額拒絕而非靜默截斷

- [ ] **TASK-055 — 取消或失效 offer 時停止讀取**
  - **Dependencies:** `TASK-053`  |  **Unit Test:** `clipboard_cancel_drops_pending_transfer`
  - **Done:** 舊 FD 不再寫入 active Tab

- [ ] **TASK-056 — Ctrl+Shift+C 只在有 selection 時複製**
  - **Dependencies:** `TASK-046`, `TASK-050`  |  **Unit Test:** `copy_shortcut_requires_selection`
  - **Done:** 冇 selection 時不產生錯誤 clipboard ownership

- [ ] **TASK-057 — Ctrl+Shift+V 產生 PasteIntent**
  - **Dependencies:** `TASK-052`, `TASK-011`  |  **Unit Test:** `paste_shortcut_routes_to_focused_tab`
  - **Done:** Shortcut 不會當作 Shell 普通文字

- [ ] **TASK-058 — Paste 保留 UTF-8、多行與空格**
  - **Dependencies:** `TASK-053`, `TASK-023`  |  **Unit Test:** `paste_preserves_utf8_and_newlines`
  - **Done:** 繁體中文、LF、tabs 不被無意改寫

- [ ] **TASK-059 — Paste target 綁定發起時嘅 TabId**
  - **Dependencies:** `TASK-057`, `TASK-002`  |  **Unit Test:** `paste_never_switches_target_mid_transfer`
  - **Done:** 切 Tab 時唔將 Paste 送錯 Shell

- [ ] **TASK-060 — Tab close / focus lost 後取消 pending Paste**
  - **Dependencies:** `TASK-059`  |  **Unit Test:** `paste_to_closed_tab_is_cancelled`
  - **Done:** 不向已關閉 Tab 或新 Tab 發送殘留輸入

- [ ] **TASK-061 — 只在 terminal mode 開啟時包 bracketed paste**
  - **Dependencies:** `TASK-027`, `TASK-058`  |  **Unit Test:** `bracketed_paste_wraps_exactly_once`
  - **Done:** 啟用輸出前後 delimiter；無啟用時不亂加

- [ ] **TASK-062 — 無 bracketed mode 時對多行 Paste 作安全提示**
  - **Dependencies:** `TASK-058`, `TASK-027`  |  **Unit Test:** `multiline_paste_requires_confirmation`
  - **Done:** 避免未經確認嘅多行 Paste；不聲稱能阻止所有 Shell 執行

- [ ] **TASK-063 — 偵測 primary-selection protocol 可用性**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `primary_selection_capability_is_optional`
  - **Done:** 缺少 protocol 時正常使用一般 Clipboard

- [ ] **TASK-064 — Primary Selection state 與 Clipboard 分離**
  - **Dependencies:** `TASK-063`, `TASK-047`  |  **Unit Test:** `primary_selection_does_not_replace_clipboard`
  - **Done:** Mouse selection / Copy 兩套 owner 狀態互不覆寫

- [ ] **TASK-065 — Middle-click 產生 Primary PasteIntent**
  - **Dependencies:** `TASK-064`, `TASK-014`  |  **Unit Test:** `middle_click_reads_primary_only`
  - **Done:** Middle-click 唔讀錯一般 Clipboard

- [ ] **TASK-066 — 拒絕預設 OSC 52 Clipboard Read**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `osc52_read_is_denied_by_default`
  - **Done:** PTY 輸出唔可偷偷讀系統 Clipboard

- [ ] **TASK-067 — OSC 52 Write 經 opt-in policy**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `osc52_write_needs_explicit_opt_in`
  - **Done:** 未 opt-in 時不覆寫 Clipboard

- [ ] **TASK-068 — OSC 52 Base64 payload 設上限**
  - **Dependencies:** `TASK-067`  |  **Unit Test:** `osc52_rejects_oversized_base64`
  - **Done:** 超額/壞編碼不造成無界 allocation

## G. Fcitx5 / Wayland `text-input-v3`

- [ ] **TASK-069 — 偵測 text-input-v3，建立 seat-scoped state**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `ime_capability_does_not_require_gtk`
  - **Done:** 冇 protocol 時仍可普通英文輸入

- [ ] **TASK-070 — IME enter/leave focus reducer**
  - **Dependencies:** `TASK-069`, `TASK-011`  |  **Unit Test:** `ime_focus_matches_surface`
  - **Done:** 只接受持有 focus 嘅 Surface IME events

- [ ] **TASK-071 — `enable/disable` 與 commit request 排序**
  - **Dependencies:** `TASK-070`  |  **Unit Test:** `ime_enable_disable_requests_ordered`
  - **Done:** 正確發出 Wayland text-input state changes

- [ ] **TASK-072 — 累積 Preedit/Commit，`done` 時原子套用**
  - **Dependencies:** `TASK-071`  |  **Unit Test:** `ime_done_applies_staged_events`
  - **Done:** `done` 前唔送半套更新入 PTY

- [ ] **TASK-073 — 保存 preedit text，不送 PTY**
  - **Dependencies:** `TASK-072`, `TASK-037`  |  **Unit Test:** `preedit_never_writes_to_pty`
  - **Done:** 候選字前文字只畫 overlay

- [ ] **TASK-074 — commit_string UTF-8 寫入 PTY 一次**
  - **Dependencies:** `TASK-072`, `TASK-023`  |  **Unit Test:** `ime_commit_is_written_once`
  - **Done:** 同一 commit 不重複、不漏寫

- [ ] **TASK-075 — 安全處理 delete_surrounding_text**
  - **Dependencies:** `TASK-072`  |  **Unit Test:** `ime_delete_does_not_inject_backspace`
  - **Done:** 未設定 surrounding text 時唔注入 Backspace

- [ ] **TASK-076 — 計算 cursor rectangle (surface-local)**
  - **Dependencies:** `TASK-031`, `TASK-073`  |  **Unit Test:** `ime_cursor_rect_tracks_text_cell`
  - **Done:** IME candidate anchors 對應目前 Cursor

- [ ] **TASK-077 — Scale/Resize 時更新 cursor rectangle**
  - **Dependencies:** `TASK-076`, `TASK-015`  |  **Unit Test:** `ime_cursor_rect_recomputes_after_resize`
  - **Done:** 視窗變更後 IME 座標正確

- [ ] **TASK-078 — focus epoch 擋掉舊 IME transaction**
  - **Dependencies:** `TASK-070`, `TASK-074`  |  **Unit Test:** `stale_ime_transaction_is_ignored`
  - **Done:** 焦點已轉移時不可誤 Commit 去其他 Tab

- [ ] **TASK-079 — 分離 IME 消費 key 與一般 PTY key**
  - **Dependencies:** `TASK-012`, `TASK-072`  |  **Unit Test:** `ime_key_is_not_sent_twice`
  - **Done:** 中文組字唔重複輸入 key/character

- [ ] **TASK-080 — Tab 切換時取消舊 preedit**
  - **Dependencies:** `TASK-078`, `TASK-002`  |  **Unit Test:** `ime_preedit_does_not_cross_tab`
  - **Done:** 轉 Tab 後唔保留舊 Tab 組字

- [ ] **TASK-081 — IME unavailable/restart 狀態復原**
  - **Dependencies:** `TASK-071`  |  **Unit Test:** `ime_restart_keeps_normal_keyboard`
  - **Done:** Fcitx5 重啟唔令一般 keyboard 失效

- [ ] **TASK-082 — 保留正常 IME switch shortcut 路徑**
  - **Dependencies:** `TASK-079`  |  **Unit Test:** `ime_toggle_key_is_not_swallowed`
  - **Done:** 按輸入法切換鍵唔被 Terminal 自行佔用

## H. Native GUI Tabs

- [ ] **TASK-083 — Tab registry 保存不同 `TabId` 狀態**
  - **Dependencies:** `TASK-002`  |  **Unit Test:** `tab_registry_keeps_unique_entries`
  - **Done:** 每 Tab 有獨立 Terminal/PTY/Selection state

- [ ] **TASK-084 — New Tab event 建立唯一 state + spawn effect**
  - **Dependencies:** `TASK-083`, `TASK-021`  |  **Unit Test:** `new_tab_emits_single_spawn`
  - **Done:** 按一次只開一個 Tab

- [ ] **TASK-085 — Active Tab 變更 reducer**
  - **Dependencies:** `TASK-083`  |  **Unit Test:** `tab_switch_updates_active_id`
  - **Done:** 只更改目標 Tab，其他 state 原樣保留

- [ ] **TASK-086 — Next/Previous Tab wrap-around**
  - **Dependencies:** `TASK-085`  |  **Unit Test:** `tab_cycle_wraps_at_ends`
  - **Done:** 切換順序有確定結果

- [ ] **TASK-087 — Close Tab 只清理所屬 PTY**
  - **Dependencies:** `TASK-083`, `TASK-025`  |  **Unit Test:** `close_tab_does_not_kill_other_pty`
  - **Done:** 退出一個 Tab 不影響其餘 Shell

- [ ] **TASK-088 — Tab Bar item rectangles + hit test**
  - **Dependencies:** `TASK-031`, `TASK-083`  |  **Unit Test:** `tab_bar_hit_test_selects_correct_tab`
  - **Done:** Mouse click 可正確識別 Tab

- [ ] **TASK-089 — Tab Title 顯示與安全長度截斷**
  - **Dependencies:** `TASK-083`  |  **Unit Test:** `tab_title_is_sanitized`
  - **Done:** 不可信 Escape/控制字元不直接顯示

- [ ] **TASK-090 — Inactive Tab 持續更新 PTY/Terminal state**
  - **Dependencies:** `TASK-085`, `TASK-026`  |  **Unit Test:** `inactive_tab_drains_output`
  - **Done:** 隱藏 Tab 不阻塞 child process

- [ ] **TASK-091 — 只 Render Active Tab**
  - **Dependencies:** `TASK-085`, `TASK-038`  |  **Unit Test:** `only_active_tab_is_rendered`
  - **Done:** 切 Tab 時刷新，Idle 隱藏 Tab 不 repaint

- [ ] **TASK-092 — Tab switch 時 Clipboard route 保持隔離**
  - **Dependencies:** `TASK-085`, `TASK-059`  |  **Unit Test:** `copy_paste_targets_original_tab`
  - **Done:** Pending Paste 唔混入另一個 Tab

- [ ] **TASK-093 — Tab switch 時 IME route 保持隔離**
  - **Dependencies:** `TASK-085`, `TASK-080`  |  **Unit Test:** `ime_commit_targets_correct_tab`
  - **Done:** Preedit/Commit 唔跨 Tab

## I. Configuration / Resource Policy

- [ ] **TASK-094 — TOML Config default values**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `config_defaults_are_valid`
  - **Done:** 無 config file 仍能啟動

- [ ] **TASK-095 — Font family/size config 驗證**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_rejects_invalid_font_size`
  - **Done:** 無效數值明確報錯

- [ ] **TASK-096 — Keyboard bindings conflict 驗證**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_detects_duplicate_shortcuts`
  - **Done:** Paste、IME、Tab shortcut 不意外重疊

- [ ] **TASK-097 — Scrollback / Clipboard / Glyph budgets**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_limits_are_bounded`
  - **Done:** 無效/極端值回報錯誤

- [ ] **TASK-098 — 損壞 Config 唔令 App 無提示退出**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `invalid_config_returns_diagnostic`
  - **Done:** 指出設定檔與錯誤，但不自動改檔

## J. Security / Reliability / Performance

- [ ] **TASK-099 — 限制不可信 OSC payload 長度**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `oversized_osc_is_rejected_safely`
  - **Done:** 過長 sequence 不引致無界 allocation

- [ ] **TASK-100 — Hyperlink 只經 user action 開啟**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `hyperlink_does_not_auto_execute`
  - **Done:** PTY 輸出唔可直接執行指令或開網址

- [ ] **TASK-101 — Logs 對 Keyboard/Clipboard/PTY text 進行 redaction**
  - **Dependencies:** `TASK-007`  |  **Unit Test:** `logs_do_not_include_sensitive_content`
  - **Done:** 預設日誌不洩漏輸入字串

- [ ] **TASK-102 — PTY flood 事件處理設 batching/budget**
  - **Dependencies:** `TASK-022`, `TASK-017`  |  **Unit Test:** `pty_flood_is_bounded_per_tick`
  - **Done:** 大量輸出不令 UI 永遠無機會處理輸入

- [ ] **TASK-103 — 非預期 child 退出只更新對應 Tab**
  - **Dependencies:** `TASK-025`, `TASK-083`  |  **Unit Test:** `unexpected_exit_is_per_tab`
  - **Done:** 單個 Shell crash 不會令 App crash

- [ ] **TASK-104 — 無 dirty output 時禁止固定頻率 redraw**
  - **Dependencies:** `TASK-017`, `TASK-038`  |  **Unit Test:** `idle_does_not_schedule_unneeded_frames`
  - **Done:** event-loop 無 busy spin

- [ ] **TASK-105 — 資源計算按 Tab 分配、有上限**
  - **Dependencies:** `TASK-083`, `TASK-097`  |  **Unit Test:** `tab_budget_rejects_overallocation`
  - **Done:** 新增 Tab 不造成無界 cache

- [ ] **TASK-106 — Terminal/Clipboard/IME parser fuzz fixtures**
  - **Dependencies:** `TASK-099`, `TASK-048`, `TASK-072`  |  **Unit Test:** `malformed_input_never_panics`
  - **Done:** 預設 fuzz seed + property unit tests 無 panic

- [ ] **TASK-107 — App side network/telemetry capability invariant**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `app_telemetry_is_disabled_by_design`
  - **Done:** Terminal 自身不主動上報資料；child process 不在此保證內

## K. Kitty Keyboard Protocol（基本功能完成後）

- [ ] **TASK-108 — 定義 Kitty keyboard enhancement flags**
  - **Dependencies:** `TASK-027`  |  **Unit Test:** `kitty_flags_roundtrip`
  - **Done:** flags 可解析與序列化

- [ ] **TASK-109 — 處理 Kitty keyboard query/response**
  - **Dependencies:** `TASK-108`  |  **Unit Test:** `kitty_query_reports_current_flags`
  - **Done:** query 只回報已支援 modes

- [ ] **TASK-110 — 編碼普通及 modified key events**
  - **Dependencies:** `TASK-108`, `TASK-012`  |  **Unit Test:** `kitty_encoder_handles_modified_enter`
  - **Done:** 不破壞 legacy keyboard 兼容

- [ ] **TASK-111 — 編碼 press/repeat/release**
  - **Dependencies:** `TASK-110`  |  **Unit Test:** `kitty_key_event_types_are_distinct`
  - **Done:** Mode 開啟時分清 press/repeat/release

- [ ] **TASK-112 — push/pop keyboard mode stack + cap**
  - **Dependencies:** `TASK-108`  |  **Unit Test:** `kitty_stack_is_bounded`
  - **Done:** 不能用 Escape 無限增長

- [ ] **TASK-113 — Main/Alternate Screen 各自 mode stack**
  - **Dependencies:** `TASK-112`, `TASK-027`  |  **Unit Test:** `kitty_mode_stack_is_screen_scoped`
  - **Done:** 兩個 Screen 切換時狀態不污染

- [ ] **TASK-114 — Kitty encoder 不重發 IME commit**
  - **Dependencies:** `TASK-110`, `TASK-079`  |  **Unit Test:** `kitty_encoder_skips_ime_consumed_text`
  - **Done:** 中文輸入不重複

## L. Kitty Graphics Protocol（Keyboard 之後）

- [ ] **TASK-115 — 有界 Kitty APC header parser**
  - **Dependencies:** `TASK-099`  |  **Unit Test:** `kitty_apc_parser_rejects_invalid_header`
  - **Done:** 壞 header / 超長序列無 panic

- [ ] **TASK-116 — 合併 direct transmission chunks**
  - **Dependencies:** `TASK-115`  |  **Unit Test:** `kitty_chunks_reassemble_in_order`
  - **Done:** 不完整 chunk 不 Render

- [ ] **TASK-117 — 解碼 Direct PNG / RGBA payload**
  - **Dependencies:** `TASK-116`  |  **Unit Test:** `kitty_direct_payload_checks_dimensions`
  - **Done:** 拒絕非法 dimensions / formats

- [ ] **TASK-118 — 限制 image bytes / pixel count**
  - **Dependencies:** `TASK-117`, `TASK-097`  |  **Unit Test:** `kitty_image_budget_limits_allocations`
  - **Done:** 超額拒絕而非 OOM

- [ ] **TASK-119 — Image ID 與 Placement state**
  - **Dependencies:** `TASK-118`  |  **Unit Test:** `kitty_image_ids_do_not_collide`
  - **Done:** 不同 images/placements 不互相覆蓋

- [ ] **TASK-120 — 刪除 image/placement 並釋放資源**
  - **Dependencies:** `TASK-119`  |  **Unit Test:** `kitty_delete_releases_memory`
  - **Done:** 刪除後 budget 可回收

- [ ] **TASK-121 — 預設拒絕 file/temp/shared-memory transport**
  - **Dependencies:** `TASK-115`  |  **Unit Test:** `kitty_file_transport_is_disabled`
  - **Done:** PTY output 不可令 Terminal 讀取本機檔案

## M. Packaging / Public Verification

- [ ] **TASK-122 — 產生可逆 user-local / `/usr/local` install paths**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `install_paths_do_not_overwrite_system_files`
  - **Done:** 安裝/移除範圍限 Ptyra 自己檔案

- [ ] **TASK-123 — 生成與已實作能力一致嘅 terminfo**
  - **Dependencies:** `TASK-027`  |  **Unit Test:** `terminfo_does_not_claim_unsupported_features`
  - **Done:** Kitty 未完成前不宣稱 Kitty 全功能

- [ ] **TASK-124 — Desktop entry schema/command 檢查**
  - **Dependencies:** `TASK-122`  |  **Unit Test:** `desktop_entry_points_to_ptyra`
  - **Done:** Desktop launcher 無任意 Shell evaluation

## Integration / Acceptance Matrix（唔當作 Unit TASK）

呢啲係真機 Gate，需人工或系統整合測試。每項對應上面一批已完成嘅 Atomic TASKS。

- [ ] Ubuntu 24.04 + Sway 1.10.1 原生 Wayland 可啟動；無 GTK / Qt UI runtime。
- [ ] 喺 Firefox 複製繁體中文 → Ptyra Paste，內容及換行完整。
- [ ] Ptyra Copy → Firefox、Kitty、其他 Wayland App，內容一致。
- [ ] 拖曳、Double-click、Triple-click、Soft Wrap Copy 正常。
- [ ] Large Paste、empty offer、Clipboard owner 消失、Paste 時切 Tab，無資料混 Tab。
- [ ] Primary Selection / Middle-click（當 compositor 支援時）正常。
- [ ] Fcitx5 繁體中文切換、Preedit、Candidate Position、Commit、Tab/Focus 切換正常。
- [ ] 開 5–10 Tabs 並同時輸出，PTY、Clipboard、IME 互不混淆。
- [ ] `htop`、`vim`、`tmux`、`Codex` 依其已實作能力正常操作。
- [ ] 同機量度一個 Tab 及多個 Tabs 嘅 PSS、Idle CPU、frame rate；記錄結果。
- [ ] 長時間 Soak Test 冇未解釋嘅視窗退出；如有 failure 保留可重現資料。
- [ ] Kitty Keyboard / Graphics 各自使用官方 protocol probes；未實作功能明確標示。

## Completion Record Template（填於 Pull Request，不塞入 Checkbox）

```text
TASK: TASK-XXX
Dependencies satisfied: YES / NO
Changed files: ...
Unit test command: cargo test <test_name>
Unit test result: PASS / FAIL
Integration test, if required: ...
Regression impact: Clipboard / IME / Tabs / Renderer / None
Unexpected behavior or security impact: ...
Approval: ...
```
