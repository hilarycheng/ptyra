# Ptyra — TASKS.md

> **原始規劃 v0.2 · 2026-10-27。** Checkbox 只代表列出嘅 TASK 範圍；整體產品狀態見 README 同下方 Acceptance Matrix。

## Rules / 使用方法

- 每個 TASK 用標準 Markdown checkbox：`- [ ]`；**只有完成測試同驗收後**先改成 `- [x]`。
- `TASK-XXX` 係永久識別碼，**唔係 Priority、Phase 或必須依次執行嘅次序**。
- 每個 TASK 只處理一個可獨立驗證嘅行為；Dependencies 完成後，單獨實作同測試。
- 每個 TASK 都列出一個確定性 **Unit Test** 名稱。平台 API 透過 fake/mock adapter 驗證。
- Sway、Fcitx5、Firefox 互通必須另外進行真機 Integration / Acceptance Tests。Unit Test 不能代替真機測試。
- 完成一項 TASK，必須提交 source、unit test、驗收證據；不得順手修改其他 TASK。
- 每次只把經使用者明確 **APPROVE** 嘅 TASK 交畀 Coding Agent 實作。
- 全部 `[ ]` 起始，表示尚未實作、未通過測試。

**Total:** 199 atomic TASKS（160 已勾選；TASK-129、TASK-162–199 共 39 項未勾選）。

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

- [x] **TASK-031 — cell coordinates 轉 pixel rectangles**
  - **Dependencies:** `TASK-030`, `TASK-015`  |  **Unit Test:** `cell_rect_rounding_is_stable`
  - **Done:** window size 改變後仍不重疊、不 overflow

- [x] **TASK-032 — 計算 monospace cell metrics**
  - **Dependencies:** `TASK-031`  |  **Unit Test:** `font_metrics_validate_dimensions`
  - **Done:** 無效 font size 會回報錯誤

- [x] **TASK-033 — 建立缺字 fallback 選擇邏輯**
  - **Dependencies:** `TASK-032`  |  **Unit Test:** `font_fallback_uses_cjk_candidate`
  - **Done:** 繁體中文字可選擇支援嘅 fallback font

- [x] **TASK-034 — 實作可限定大小嘅 glyph cache**
  - **Dependencies:** `TASK-032`  |  **Unit Test:** `glyph_cache_evicts_by_limit`
  - **Done:** cache 超過 budget 會淘汰舊項

- [x] **TASK-035 — Grid cell → draw command**
  - **Dependencies:** `TASK-030`, `TASK-034`  |  **Unit Test:** `draw_commands_preserve_colors`
  - **Done:** 文字、前景、背景不交換

- [x] **TASK-036 — Cursor draw command**
  - **Dependencies:** `TASK-035`  |  **Unit Test:** `cursor_shape_matches_mode`
  - **Done:** Block / Beam / Underline 位置正確

- [x] **TASK-037 — 獨立 selection/preedit overlay layer**
  - **Dependencies:** `TASK-035`  |  **Unit Test:** `overlay_does_not_change_grid`
  - **Done:** Preedit 顯示但不修改 PTY Grid

- [x] **TASK-038 — 只繪製可見 dirty content**
  - **Dependencies:** `TASK-017`, `TASK-035`  |  **Unit Test:** `hidden_or_clean_view_skips_draw`
  - **Done:** Idle 不反覆 Render；隱藏 Tab 不 Render

## E. Selection / 文字抽取

- [x] **TASK-039 — Mouse pixel → terminal cell 映射**
  - **Dependencies:** `TASK-031`, `TASK-014`  |  **Unit Test:** `hit_test_maps_cell_boundaries`
  - **Done:** Click 落在正確 row/col

- [x] **TASK-040 — Mouse drag selection range reducer**
  - **Dependencies:** `TASK-039`  |  **Unit Test:** `drag_selection_normalizes_direction`
  - **Done:** 由左至右／反向拖曳結果一致

- [x] **TASK-041 — Double-click word selection**
  - **Dependencies:** `TASK-040`  |  **Unit Test:** `double_click_selects_unicode_word`
  - **Done:** 中文及 ASCII 有清楚 word-boundary 策略

- [x] **TASK-042 — Triple-click line selection**
  - **Dependencies:** `TASK-040`  |  **Unit Test:** `triple_click_selects_logical_line`
  - **Done:** 完整選取對應 terminal line

- [x] **TASK-043 — Copy 時合併 soft-wrapped lines**
  - **Dependencies:** `TASK-040`, `TASK-026`  |  **Unit Test:** `copy_soft_wrap_does_not_add_newline`
  - **Done:** 軟折行唔額外加入 `\n`

- [x] **TASK-044 — Copy 時保留 hard line breaks**
  - **Dependencies:** `TASK-040`, `TASK-026`  |  **Unit Test:** `copy_hard_break_preserves_newline`
  - **Done:** 真實換行保留 `\n`

- [x] **TASK-045 — Selection 不切開 wide glyph**
  - **Dependencies:** `TASK-040`, `TASK-028`  |  **Unit Test:** `selection_preserves_fullwidth_glyph`
  - **Done:** 選中文字不產生半個 cell 或破字

- [x] **TASK-046 — Selection 狀態與 Screen/Tab 隔離**
  - **Dependencies:** `TASK-040`, `TASK-002`  |  **Unit Test:** `selection_is_scoped_to_tab`
  - **Done:** 不同 Tab 選區互不影響

## F. Clipboard / Copy & Paste

- [x] **TASK-047 — 定義 Clipboard state、owner、offer ID**
  - **Dependencies:** `TASK-002`, `TASK-005`  |  **Unit Test:** `clipboard_state_rejects_stale_offer`
  - **Done:** 舊 offer 唔覆蓋新 owner

- [x] **TASK-048 — 建立 text MIME selection 規則**
  - **Dependencies:** `TASK-047`  |  **Unit Test:** `mime_prefers_utf8_text`
  - **Done:** 優先用 UTF-8 text，缺少時可 fallback

- [x] **TASK-049 — 處理 Wayland seat serial validity**
  - **Dependencies:** `TASK-047`, `TASK-011`  |  **Unit Test:** `clipboard_serial_requires_input_event`
  - **Done:** 無有效 serial 時不可假裝成功 set_selection

- [x] **TASK-050 — 建立 wl_data_source request sequence**
  - **Dependencies:** `TASK-048`, `TASK-049`  |  **Unit Test:** `data_source_advertises_text_mime`
  - **Done:** Copy 時對外提供正確 MIME

- [x] **TASK-051 — 向 Clipboard FD 寫資料**
  - **Dependencies:** `TASK-050`  |  **Unit Test:** `clipboard_write_handles_short_write`
  - **Done:** partial write、EINTR 和 peer-close 安全處理

- [x] **TASK-052 — 從 wl_data_offer event 建立可接收 offer**
  - **Dependencies:** `TASK-047`  |  **Unit Test:** `data_offer_tracks_formats`
  - **Done:** 能保存 clipboard format / cancellation

- [x] **TASK-053 — 從 Clipboard FD 讀取文字**
  - **Dependencies:** `TASK-052`, `TASK-048`  |  **Unit Test:** `clipboard_read_handles_partial_utf8`
  - **Done:** 分段 UTF-8 唔會被錯誤切開

- [x] **TASK-054 — 限制 Paste input size 並回報錯誤**
  - **Dependencies:** `TASK-053`  |  **Unit Test:** `clipboard_read_limit_is_explicit`
  - **Done:** 超額拒絕而非靜默截斷

- [x] **TASK-055 — 取消或失效 offer 時停止讀取**
  - **Dependencies:** `TASK-053`  |  **Unit Test:** `clipboard_cancel_drops_pending_transfer`
  - **Done:** 舊 FD 不再寫入 active Tab

- [x] **TASK-056 — Ctrl+Shift+C 只在有 selection 時複製**
  - **Dependencies:** `TASK-046`, `TASK-050`  |  **Unit Test:** `copy_shortcut_requires_selection`
  - **Done:** 冇 selection 時不產生錯誤 clipboard ownership

- [x] **TASK-057 — Ctrl+Shift+V 產生 PasteIntent**
  - **Dependencies:** `TASK-052`, `TASK-011`  |  **Unit Test:** `paste_shortcut_routes_to_focused_tab`
  - **Done:** Shortcut 不會當作 Shell 普通文字

- [x] **TASK-058 — Paste 保留 UTF-8、多行與空格**
  - **Dependencies:** `TASK-053`, `TASK-023`  |  **Unit Test:** `paste_preserves_utf8_and_newlines`
  - **Done:** 繁體中文、LF、tabs 不被無意改寫

- [x] **TASK-059 — Paste target 綁定發起時嘅 TabId**
  - **Dependencies:** `TASK-057`, `TASK-002`  |  **Unit Test:** `paste_never_switches_target_mid_transfer`
  - **Done:** 切 Tab 時唔將 Paste 送錯 Shell

- [x] **TASK-060 — Tab close / focus lost 後取消 pending Paste**
  - **Dependencies:** `TASK-059`  |  **Unit Test:** `paste_to_closed_tab_is_cancelled`
  - **Done:** 不向已關閉 Tab 或新 Tab 發送殘留輸入

- [x] **TASK-061 — 只在 terminal mode 開啟時包 bracketed paste**
  - **Dependencies:** `TASK-027`, `TASK-058`  |  **Unit Test:** `bracketed_paste_wraps_exactly_once`
  - **Done:** 啟用輸出前後 delimiter；無啟用時不亂加

- [x] **TASK-062 — 無 bracketed mode 時對多行 Paste 作安全提示**
  - **Dependencies:** `TASK-058`, `TASK-027`  |  **Unit Test:** `multiline_paste_requires_confirmation`
  - **Done:** 避免未經確認嘅多行 Paste；不聲稱能阻止所有 Shell 執行

- [x] **TASK-063 — 偵測 primary-selection protocol 可用性**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `primary_selection_capability_is_optional`
  - **Done:** 缺少 protocol 時正常使用一般 Clipboard

- [x] **TASK-064 — Primary Selection state 與 Clipboard 分離**
  - **Dependencies:** `TASK-063`, `TASK-047`  |  **Unit Test:** `primary_selection_does_not_replace_clipboard`
  - **Done:** Mouse selection / Copy 兩套 owner 狀態互不覆寫

- [x] **TASK-065 — Middle-click 產生 Primary PasteIntent**
  - **Dependencies:** `TASK-064`, `TASK-014`  |  **Unit Test:** `middle_click_reads_primary_only`
  - **Done:** Middle-click 唔讀錯一般 Clipboard

- [x] **TASK-066 — 拒絕預設 OSC 52 Clipboard Read**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `osc52_read_is_denied_by_default`
  - **Done:** PTY 輸出唔可偷偷讀系統 Clipboard

- [x] **TASK-067 — OSC 52 Write 經 opt-in policy**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `osc52_write_needs_explicit_opt_in`
  - **Done:** 未 opt-in 時不覆寫 Clipboard

- [x] **TASK-068 — OSC 52 Base64 payload 設上限**
  - **Dependencies:** `TASK-067`  |  **Unit Test:** `osc52_rejects_oversized_base64`
  - **Done:** 超額/壞編碼不造成無界 allocation

## G. Fcitx5 / Wayland `text-input-v3`

- [x] **TASK-069 — 偵測 text-input-v3，建立 seat-scoped state**
  - **Dependencies:** `TASK-008`  |  **Unit Test:** `ime_capability_does_not_require_gtk`
  - **Done:** 冇 protocol 時仍可普通英文輸入

- [x] **TASK-070 — IME enter/leave focus reducer**
  - **Dependencies:** `TASK-069`, `TASK-011`  |  **Unit Test:** `ime_focus_matches_surface`
  - **Done:** 只接受持有 focus 嘅 Surface IME events

- [x] **TASK-071 — `enable/disable` 與 commit request 排序**
  - **Dependencies:** `TASK-070`  |  **Unit Test:** `ime_enable_disable_requests_ordered`
  - **Done:** 正確發出 Wayland text-input state changes

- [x] **TASK-072 — 累積 Preedit/Commit，`done` 時原子套用**
  - **Dependencies:** `TASK-071`  |  **Unit Test:** `ime_done_applies_staged_events`
  - **Done:** `done` 前唔送半套更新入 PTY

- [x] **TASK-073 — 保存 preedit text，不送 PTY**
  - **Dependencies:** `TASK-072`, `TASK-037`  |  **Unit Test:** `preedit_never_writes_to_pty`
  - **Done:** 候選字前文字只畫 overlay

- [x] **TASK-074 — commit_string UTF-8 寫入 PTY 一次**
  - **Dependencies:** `TASK-072`, `TASK-023`  |  **Unit Test:** `ime_commit_is_written_once`
  - **Done:** 同一 commit 不重複、不漏寫

- [x] **TASK-075 — 安全處理 delete_surrounding_text**
  - **Dependencies:** `TASK-072`  |  **Unit Test:** `ime_delete_does_not_inject_backspace`
  - **Done:** 未設定 surrounding text 時唔注入 Backspace

- [x] **TASK-076 — 計算 cursor rectangle (surface-local)**
  - **Dependencies:** `TASK-031`, `TASK-073`  |  **Unit Test:** `ime_cursor_rect_tracks_text_cell`
  - **Done:** IME candidate anchors 對應目前 Cursor

- [x] **TASK-077 — Scale/Resize 時更新 cursor rectangle**
  - **Dependencies:** `TASK-076`, `TASK-015`  |  **Unit Test:** `ime_cursor_rect_recomputes_after_resize`
  - **Done:** 視窗變更後 IME 座標正確

- [x] **TASK-078 — focus epoch 擋掉舊 IME transaction**
  - **Dependencies:** `TASK-070`, `TASK-074`  |  **Unit Test:** `stale_ime_transaction_is_ignored`
  - **Done:** 焦點已轉移時不可誤 Commit 去其他 Tab

- [x] **TASK-079 — 分離 IME 消費 key 與一般 PTY key**
  - **Dependencies:** `TASK-012`, `TASK-072`  |  **Unit Test:** `ime_key_is_not_sent_twice`
  - **Done:** 中文組字唔重複輸入 key/character

- [x] **TASK-080 — Tab 切換時取消舊 preedit**
  - **Dependencies:** `TASK-078`, `TASK-002`  |  **Unit Test:** `ime_preedit_does_not_cross_tab`
  - **Done:** 轉 Tab 後唔保留舊 Tab 組字

- [x] **TASK-081 — IME unavailable/restart 狀態復原**
  - **Dependencies:** `TASK-071`  |  **Unit Test:** `ime_restart_keeps_normal_keyboard`
  - **Done:** Fcitx5 重啟唔令一般 keyboard 失效

- [x] **TASK-082 — 保留正常 IME switch shortcut 路徑**
  - **Dependencies:** `TASK-079`  |  **Unit Test:** `ime_toggle_key_is_not_swallowed`
  - **Done:** 按輸入法切換鍵唔被 Terminal 自行佔用

## H. Native GUI Tabs

- [x] **TASK-083 — Tab registry 保存不同 `TabId` 狀態**
  - **Dependencies:** `TASK-002`  |  **Unit Test:** `tab_registry_keeps_unique_entries`
  - **Done:** 每 Tab 有獨立 Terminal/PTY/Selection state

- [x] **TASK-084 — New Tab event 建立唯一 state + spawn effect**
  - **Dependencies:** `TASK-083`, `TASK-021`  |  **Unit Test:** `new_tab_emits_single_spawn`
  - **Done:** 按一次只開一個 Tab

- [x] **TASK-085 — Active Tab 變更 reducer**
  - **Dependencies:** `TASK-083`  |  **Unit Test:** `tab_switch_updates_active_id`
  - **Done:** 只更改目標 Tab，其他 state 原樣保留

- [x] **TASK-086 — Next/Previous Tab wrap-around**
  - **Dependencies:** `TASK-085`  |  **Unit Test:** `tab_cycle_wraps_at_ends`
  - **Done:** 切換順序有確定結果

- [x] **TASK-087 — Close Tab 只清理所屬 PTY**
  - **Dependencies:** `TASK-083`, `TASK-025`  |  **Unit Test:** `close_tab_does_not_kill_other_pty`
  - **Done:** 退出一個 Tab 不影響其餘 Shell

- [x] **TASK-088 — Tab Bar item rectangles + hit test**
  - **Dependencies:** `TASK-031`, `TASK-083`  |  **Unit Test:** `tab_bar_hit_test_selects_correct_tab`
  - **Done:** Mouse click 可正確識別 Tab

- [x] **TASK-089 — Tab Title 顯示與安全長度截斷**
  - **Dependencies:** `TASK-083`  |  **Unit Test:** `tab_title_is_sanitized`
  - **Done:** 不可信 Escape/控制字元不直接顯示

- [x] **TASK-090 — Inactive Tab 持續更新 PTY/Terminal state**
  - **Dependencies:** `TASK-085`, `TASK-026`  |  **Unit Test:** `inactive_tab_drains_output`
  - **Done:** 隱藏 Tab 不阻塞 child process

- [x] **TASK-091 — 只 Render Active Tab**
  - **Dependencies:** `TASK-085`, `TASK-038`  |  **Unit Test:** `only_active_tab_is_rendered`
  - **Done:** 切 Tab 時刷新，Idle 隱藏 Tab 不 repaint

- [x] **TASK-092 — Tab switch 時 Clipboard route 保持隔離**
  - **Dependencies:** `TASK-085`, `TASK-059`  |  **Unit Test:** `copy_paste_targets_original_tab`
  - **Done:** Pending Paste 唔混入另一個 Tab

- [x] **TASK-093 — Tab switch 時 IME route 保持隔離**
  - **Dependencies:** `TASK-085`, `TASK-080`  |  **Unit Test:** `ime_commit_targets_correct_tab`
  - **Done:** Preedit/Commit 唔跨 Tab

## I. Configuration / Resource Policy

- [x] **TASK-094 — TOML Config default values**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `config_defaults_are_valid`
  - **Done:** 無 config file 仍能啟動

- [x] **TASK-095 — Font family/size config 驗證**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_rejects_invalid_font_size`
  - **Done:** 無效數值明確報錯

- [x] **TASK-096 — Keyboard bindings conflict 驗證**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_detects_duplicate_shortcuts`
  - **Done:** Paste、IME、Tab shortcut 不意外重疊

- [x] **TASK-097 — Scrollback / Clipboard / Glyph budgets**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `config_limits_are_bounded`
  - **Done:** 無效/極端值回報錯誤

- [x] **TASK-098 — 損壞 Config 唔令 App 無提示退出**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `invalid_config_returns_diagnostic`
  - **Done:** 指出設定檔與錯誤，但不自動改檔

## J. Security / Reliability / Performance

- [x] **TASK-099 — 限制不可信 OSC payload 長度**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `oversized_osc_is_rejected_safely`
  - **Done:** 過長 sequence 不引致無界 allocation

- [x] **TASK-100 — Hyperlink 只經 user action 開啟**
  - **Dependencies:** `TASK-026`  |  **Unit Test:** `hyperlink_does_not_auto_execute`
  - **Done:** PTY 輸出唔可直接執行指令或開網址

- [x] **TASK-101 — Logs 對 Keyboard/Clipboard/PTY text 進行 redaction**
  - **Dependencies:** `TASK-007`  |  **Unit Test:** `logs_do_not_include_sensitive_content`
  - **Done:** 預設日誌不洩漏輸入字串

- [x] **TASK-102 — PTY flood 事件處理設 batching/budget**
  - **Dependencies:** `TASK-022`, `TASK-017`  |  **Unit Test:** `pty_flood_is_bounded_per_tick`
  - **Done:** 大量輸出不令 UI 永遠無機會處理輸入

- [x] **TASK-103 — 非預期 child 退出只更新對應 Tab**
  - **Dependencies:** `TASK-025`, `TASK-083`  |  **Unit Test:** `unexpected_exit_is_per_tab`
  - **Done:** 單個 Shell crash 不會令 App crash

- [x] **TASK-104 — 無 dirty output 時禁止固定頻率 redraw**
  - **Dependencies:** `TASK-017`, `TASK-038`  |  **Unit Test:** `idle_does_not_schedule_unneeded_frames`
  - **Done:** event-loop 無 busy spin

- [x] **TASK-105 — 資源計算按 Tab 分配、有上限**
  - **Dependencies:** `TASK-083`, `TASK-097`  |  **Unit Test:** `tab_budget_rejects_overallocation`
  - **Done:** 新增 Tab 不造成無界 cache

- [x] **TASK-106 — Terminal/Clipboard/IME parser fuzz fixtures**
  - **Dependencies:** `TASK-099`, `TASK-048`, `TASK-072`  |  **Unit Test:** `malformed_input_never_panics`
  - **Done:** 預設 fuzz seed + property unit tests 無 panic

- [x] **TASK-107 — App side network/telemetry capability invariant**
  - **Dependencies:** `TASK-001`  |  **Unit Test:** `app_telemetry_is_disabled_by_design`
  - **Done:** Terminal 自身不主動上報資料；child process 不在此保證內

## K. Kitty Keyboard Protocol（基本功能完成後）

- [x] **TASK-108 — 定義 Kitty keyboard enhancement flags**
  - **Dependencies:** `TASK-027`  |  **Unit Test:** `kitty_flags_roundtrip`
  - **Done:** flags 可解析與序列化

- [x] **TASK-109 — 處理 Kitty keyboard query/response**
  - **Dependencies:** `TASK-108`  |  **Unit Test:** `kitty_query_reports_current_flags`
  - **Done:** query 只回報已支援 modes

- [x] **TASK-110 — 編碼普通及 modified key events**
  - **Dependencies:** `TASK-108`, `TASK-012`  |  **Unit Test:** `kitty_encoder_handles_modified_enter`
  - **Done:** 不破壞 legacy keyboard 兼容

- [x] **TASK-111 — 編碼 press/repeat/release**
  - **Dependencies:** `TASK-110`  |  **Unit Test:** `kitty_key_event_types_are_distinct`
  - **Done:** Mode 開啟時分清 press/repeat/release

- [x] **TASK-112 — push/pop keyboard mode stack + cap**
  - **Dependencies:** `TASK-108`  |  **Unit Test:** `kitty_stack_is_bounded`
  - **Done:** 不能用 Escape 無限增長

- [x] **TASK-113 — Main/Alternate Screen 各自 mode stack**
  - **Dependencies:** `TASK-112`, `TASK-027`  |  **Unit Test:** `kitty_mode_stack_is_screen_scoped`
  - **Done:** 兩個 Screen 切換時狀態不污染

- [x] **TASK-114 — Kitty encoder 不重發 IME commit**
  - **Dependencies:** `TASK-110`, `TASK-079`  |  **Unit Test:** `kitty_encoder_skips_ime_consumed_text`
  - **Done:** 中文輸入不重複

## L. Kitty Graphics Protocol（Keyboard 之後）

- [x] **TASK-115 — 有界 Kitty APC header parser**
  - **Dependencies:** `TASK-099`  |  **Unit Test:** `kitty_apc_parser_rejects_invalid_header`
  - **Done:** 壞 header / 超長序列無 panic

- [x] **TASK-116 — 合併 direct transmission chunks**
  - **Dependencies:** `TASK-115`  |  **Unit Test:** `kitty_chunks_reassemble_in_order`
  - **Done:** 不完整 chunk 不 Render

- [x] **TASK-117 — 解碼 Direct PNG / RGBA payload**
  - **Dependencies:** `TASK-116`  |  **Unit Test:** `kitty_direct_payload_checks_dimensions`
  - **Done:** 拒絕非法 dimensions / formats

- [x] **TASK-118 — 限制 image bytes / pixel count**
  - **Dependencies:** `TASK-117`, `TASK-097`  |  **Unit Test:** `kitty_image_budget_limits_allocations`
  - **Done:** 超額拒絕而非 OOM

- [x] **TASK-119 — Image ID 與 Placement state**
  - **Dependencies:** `TASK-118`  |  **Unit Test:** `kitty_image_ids_do_not_collide`
  - **Done:** 不同 images/placements 不互相覆蓋

- [x] **TASK-120 — 刪除 image/placement 並釋放資源**
  - **Dependencies:** `TASK-119`  |  **Unit Test:** `kitty_delete_releases_memory`
  - **Done:** 刪除後 budget 可回收

- [x] **TASK-121 — 預設拒絕 file/temp/shared-memory transport**
  - **Dependencies:** `TASK-115`  |  **Unit Test:** `kitty_file_transport_is_disabled`
  - **Done:** PTY output 不可令 Terminal 讀取本機檔案

## M. Packaging / Public Verification

- [x] **TASK-122 — 產生可逆 user-local / `/usr/local` install paths**
  - **Dependencies:** `TASK-094`  |  **Unit Test:** `install_paths_do_not_overwrite_system_files`
  - **Done:** 安裝/移除範圍限 Ptyra 自己檔案

- [x] **TASK-123 — 生成與已實作能力一致嘅 terminfo**
  - **Dependencies:** `TASK-027`  |  **Unit Test:** `terminfo_does_not_claim_unsupported_features`
  - **Done:** Kitty 未完成前不宣稱 Kitty 全功能

- [x] **TASK-124 — Desktop entry schema/command 檢查**
  - **Dependencies:** `TASK-122`  |  **Unit Test:** `desktop_entry_points_to_ptyra`
  - **Done:** Desktop launcher 無任意 Shell evaluation

## N. Native Runtime Integration（2026-11-05 已批准）

- [x] **TASK-125 — 啟動真正 Wayland 視窗並正常關閉**
  - **Dependencies:** `TASK-009`, `TASK-010`, `TASK-017`, `TASK-018`, `TASK-094` | **Unit Test:** `runtime_window_waits_for_configure`
  - **Done:** main 連接 compositor；configure/ack 後提交有界 SHM buffer；無 dirty state 時阻塞等待；close/disconnect 正常退出；隔離 compositor 驗證真正視窗。

- [x] **TASK-126 — 將字形與 terminal grid 畫到視窗**
  - **Dependencies:** `TASK-125`, `TASK-028`, `TASK-032`, `TASK-033` | **Unit Test:** `runtime_raster_draws_terminal_cells`
  - **Done:** 載入實際字型，bounded rasterization 呈現 grid、顏色及 cursor；真 compositor screenshot 驗證可見文字。

- [x] **TASK-127 — Shell PTY output 接入 runtime 畫面**
  - **Dependencies:** `TASK-126`, `TASK-021`, `TASK-022`, `TASK-024`, `TASK-025`, `TASK-026` | **Unit Test:** `runtime_pty_output_updates_grid`
  - **Done:** 啟動 Shell；event-driven PTY read 更新畫面；resize 同步 grid/PTY；退出時回收 child；真 PTY 與 compositor 驗證。

- [x] **TASK-128 — Keyboard 輸入接入互動 Shell**
  - **Dependencies:** `TASK-127`, `TASK-011`, `TASK-012`, `TASK-013`, `TASK-023`, `TASK-110` | **Unit Test:** `runtime_keyboard_routes_to_shell`
  - **Done:** 真 Wayland keymap/focus/modifiers/repeat 接駁現有 encoder 同 PTY queue；失焦停止輸入；Enter、Backspace、arrows、Ctrl+C 可用；真 compositor/PTY 驗證互動指令。

呢四項只交付單視窗、單 Shell 嘅 runtime；Clipboard、IME、Tabs、Kitty graphics runtime 接線仍須另外驗收。

- [ ] **TASK-129 — 接通單視窗 Fcitx5 中文輸入（2026-11-05 已批准實作）**
  - **Dependencies:** `TASK-069`–`TASK-082`, `TASK-126`, `TASK-128` | **Unit Test:** `runtime_ime_commits_to_shell`
  - **Done:** 綁定 seat-scoped text-input-v3；focus 控制 enable/disable；done 後只送一次 UTF-8 commit 到原本 PTY；preedit 只畫 overlay；候選字 anchor 跟 cursor/resize；保留 compositor/IME shortcut ownership；失焦清除 pending/preedit；無 IME 時英文輸入正常。Mock sequence、workspace regression 同隔離 Sway/Fcitx5 integration 分開報告；實際 desktop 候選視窗位置仍須人工驗收。
  - **Expected files:** `src/runtime.rs`, `src/runtime/ime.rs`, `src/runtime/keyboard.rs`, `src/runtime/raster.rs`, `Cargo.toml`, `Cargo.lock`, dependency invariant test、native IME test 同 verification docs。重用現有 IME/PTY/overlay 模組；不修改系統設定。

## Missing Feature / Runtime Backlog（2026-11-06 核對）

以下係按 `SCOPE.md` 同目前 source 補回嘅缺漏，**只批准記錄，唔代表已批准實作或完成驗收**。`TASK-001`–`TASK-128` 嘅 `[x]` 保留原本列明嘅 library / runtime 範圍；唔可以將 pure model、fake test 或獨立 adapter 當成整個產品已接通。`TASK-129` 已有未提交實作同隔離測試紀錄，但實際 desktop 候選視窗未驗收，繼續 `[ ]`；本次唔替使用者驗收。

**核對依據 / Coverage：**

- `src/runtime.rs` 仍持有單一 `Terminal` / `PtySession`，固定 `TabId(1)`；未接 `TabRegistry`、pointer、Clipboard 或 output scale。對應 O、P、Q、T。
- `src/runtime/keyboard.rs` 未使用 config bindings，encoder flags 固定為 empty；`src/terminal.rs` 未接 terminal reply events，runtime 未取走 `take_keyboard_reply()`。對應 O、S、W。
- `src/selection.rs` / `src/clipboard*.rs` / `src/tabs*.rs` 已有 reducers、FD helpers 同 routing policy，但未連到真 GUI events；`PendingPaste::cancel_for_tab` 有 partial framing 未解決註記。對應 P、Q。
- `src/terminal.rs` 有 history cap，未提供 runtime scroll 操作；`src/runtime/raster.rs` 未按 display offset 畫 history。對應 R。
- `src/runtime/raster.rs` 使用 scale=1、regular font、固定 palette，同整幅 buffer redraw；`src/renderer/overlay.rs` / runtime preedit 只畫一行。對應 T、U。
- `src/config.rs` 已載入 fonts / bindings / caps，但未有 basic colors 或 OSC 52 write opt-in；部分 caps 未有對應 runtime consumer。對應 Q、T、V。
- `src/kitty/protocol.rs` 支援旗標仍為空，reply queue 有 32 項上限；`src/kitty/graphics/*` 未連到 PTY stream、command dispatcher 或 renderer。對應 W、X。
- `src/install.rs` 只產生 paths / preflight / uninstall plan；assets 已存在，未有真正安裝及回滾 adapter。對應 Y。
- `README.md` 嘅 headless Sway/Fcitx5 證據有平台限制；實際 Ubuntu/Sway/Fcitx5、Firefox/Kitty、多 Tabs、AMD/Mesa、效能及 soak gates 未完成。對應 Z 同下面 Acceptance Matrix，唔另造「unit test 已驗收」狀態。

每個新增 TASK 只交付列明行為，重用現有模組；Unit Test 名稱係待實作測試，**唔係已存在或已通過**。Dependencies 係必要條件，唔係批量實作授權。涉及 native 路徑嘅 Done 同時需要對應隔離 integration evidence；最終 desktop gates 仍獨立保留。Kitty Keyboard / Graphics 依 `SCOPE.md` 喺基本 Terminal、Clipboard、IME 同 Tabs 完成後先實作，唔阻擋基本功能交付。

TASK-130–140 其後獲使用者明確 APPROVE 實作、更新 checkbox 同逐項 commit；驗證紀錄見 `VERIFICATION.md`。TASK-129 保持未完成。TASK-141–199 其後亦獲使用者明確 APPROVE 實作、更新 checkbox 同逐項 commit；只在各自依賴同驗證完成後勾選，真 desktop acceptance 仍獨立保留。

## O. Runtime Shortcuts / Native GUI Tabs

- [x] **TASK-130 — 將已設定 shortcuts 接入 runtime action router**
  - **Dependencies:** `TASK-096`, `TASK-128` | **Unit Test:** `runtime_bindings_route_actions_once`
  - **Done:** 使用 config bindings 產生 Copy/Paste/New/Close/Next/Previous action；press 只觸發一次，相關 release/repeat 唔漏入 Shell；未匹配按鍵保留原路徑；IME toggle 仍由 compositor/IME 擁有。未接通 action 要明確不可用，唔假裝成功。

- [x] **TASK-131 — Runtime 新 Tab 建立獨立 Shell session**
  - **Dependencies:** `TASK-084`, `TASK-105`, `TASK-127`, `TASK-130` | **Unit Test:** `runtime_new_tab_spawns_one_session`
  - **Done:** 由 `TabRegistry` 管理 runtime，包括第一個 Tab；一次 NewTab 只 spawn 一個 PTY，獨立 grid/history/selection/title；套用 scrollback/tab caps，spawn 失敗只回滾新 Tab，唔影響原 Shell。

- [x] **TASK-132 — 將多個 PTY 接入同一 poll loop**
  - **Dependencies:** `TASK-090`, `TASK-102`, `TASK-131` | **Unit Test:** `runtime_multi_pty_poll_is_fair`
  - **Done:** 依 ready FD 同 TabId drain/flush 對應 session；inactive Tab 繼續收 output；設每輪總 budget 同公平輪轉，flood 唔餓死 keyboard/Wayland/其他 PTY，idle 唔忙等。

- [x] **TASK-133 — Runtime 切換 active Tab 同 keyboard target**
  - **Dependencies:** `TASK-085`, `TASK-086`, `TASK-091`, `TASK-130`, `TASK-131` | **Unit Test:** `runtime_tab_switch_retargets_keyboard`
  - **Done:** Next/Previous/direct activation 更新唯一 active Tab；取消舊 repeat，重新綁定 focus epoch；keyboard 只寫新 target，renderer 只畫 active grid；保留其他 Tab state，切換唔新開 Shell。

- [x] **TASK-134 — Runtime 關 Tab 只清理所屬 session**
  - **Dependencies:** `TASK-087`, `TASK-131`, `TASK-133` | **Unit Test:** `runtime_close_tab_preserves_other_sessions`
  - **Done:** CloseTab 移除對應 FD、回收 child 同該 Tab resources；只選一個有效 successor；last Tab 關閉有明確退出行為；重複/stale close 唔誤殺其他 Shell，ID 不重用。

- [x] **TASK-135 — Child exit / EOF 只更新對應 runtime Tab**
  - **Dependencies:** `TASK-103`, `TASK-132`, `TASK-134` | **Unit Test:** `runtime_child_exit_is_per_tab`
  - **Done:** drain 最後 output 後按已記錄 exit policy 處理該 Tab，保留 exit metadata；唔再以任何一個 EOF 停止整個 app；其餘 Shell、pending output 同 UI 繼續運作。

- [x] **TASK-136 — Resize 同步每 Tab grid / PTY / reservation**
  - **Dependencies:** `TASK-024`, `TASK-105`, `TASK-131` | **Unit Test:** `runtime_resize_updates_tab_budgets`
  - **Done:** 以實際 terminal content area 計算 rows/columns，扣除 Tab Bar；active/inactive Tab 尺寸及 cell reservation 一致；超額 resize 不造成無界 allocation 或污染其他 Tab，保留有效狀態並回報 metadata。

- [x] **TASK-137 — 畫出真正 GUI Tab Bar**
  - **Dependencies:** `TASK-088`, `TASK-089`, `TASK-126`, `TASK-131`, `TASK-136` | **Unit Test:** `runtime_tab_bar_layout_reserves_content_area`
  - **Done:** 一個 native Window 畫出 labels 同 active 狀態，唔用 Sway tabbed/tmux 代替；Tab Bar 唔覆蓋 terminal rows；窄視窗/多 Tabs 有有界顯示策略，全部 Tabs 仍可透過切換操作到達。

- [x] **TASK-138 — Tab Bar pointer click 切換目標 Tab**
  - **Dependencies:** `TASK-133`, `TASK-137`, `TASK-140` | **Unit Test:** `runtime_tab_bar_click_selects_exact_tab`
  - **Done:** 實際 pointer press 經同一 layout hit-test 只選中一個 Tab；Tab Bar click 唔變成 terminal selection/mouse report；resize 或 stale target 唔切錯 Tab。

- [x] **TASK-139 — OSC title 接入 Tab label 同 window title**
  - **Dependencies:** `TASK-089`, `TASK-131`, `TASK-137` | **Unit Test:** `runtime_titles_are_sanitized_and_tab_scoped`
  - **Done:** drain 各 terminal title/reset events，過濾控制字元同長度後更新所屬 label；active Tab title 更新 toplevel；hidden Tab 唔改 active title，output 永不成為可執行指令。

## P. Pointer / Selection Runtime

- [x] **TASK-140 — 綁定 Wayland pointer 並保留事件 serial**
  - **Dependencies:** `TASK-014`, `TASK-049`, `TASK-125` | **Unit Test:** `runtime_pointer_lifecycle_preserves_serials`
  - **Done:** seat pointer capability 加入/移除、enter/leave、motion/button/axis 轉成有 target 嘅 events；保留有效 input serial，正確轉換 surface coordinates；移除/失焦取消 drag，stale surface event 不操作新 target。

- [x] **TASK-141 — 拖曳選取接入目前 Tab / Screen**
  - **Dependencies:** `TASK-039`, `TASK-040`, `TASK-045`, `TASK-046`, `TASK-140` | **Unit Test:** `runtime_drag_selects_whole_glyphs`
  - **Done:** 真 press/motion/release 更新原 target selection，正反方向同 CJK/combining 結果一致；拖出邊界有明確 clip/cancel 行為，唔產生 PTY text 或跨 Tab 選區。

- [x] **TASK-142 — Double / Triple click 接入選字選行**
  - **Dependencies:** `TASK-006`, `TASK-041`, `TASK-042`, `TASK-141` | **Unit Test:** `runtime_click_count_selects_word_or_line`
  - **Done:** 可注入 clock 嘅 click-count 判斷驅動現有 word/logical-line selection；超時、位移、Tab/Screen 改變重設 count；真操作測中文、ASCII 同 soft-wrapped line。

- [x] **TASK-143 — 將 runtime selection 畫成獨立 overlay**
  - **Dependencies:** `TASK-037`, `TASK-126`, `TASK-141` | **Unit Test:** `runtime_selection_overlay_matches_range`
  - **Done:** 真選區可見，wide glyph highlight 完整；選區改變/清除只更新畫面，唔改 terminal text；選區 state 同 renderer snapshot 接通，唔只測獨立 reducer。

- [x] **TASK-144 — Selection 跟隨 screen / resize / output lifecycle**
  - **Dependencies:** `TASK-046`, `TASK-131`, `TASK-136`, `TASK-141` | **Unit Test:** `runtime_selection_survives_only_valid_anchors`
  - **Done:** main/alt screen 各自隔離；resize/reflow、內容淘汰、Tab close 按明確 anchor policy 更新或失效選區；唔將舊 row/column 選區套到無關文字，stale drag 不可重建已清除選區。

## Q. Wayland Clipboard / Primary Selection Runtime

- [x] **TASK-145 — 綁定 seat-scoped Wayland data device**
  - **Dependencies:** `TASK-047`, `TASK-049`, `TASK-125`, `TASK-140` | **Unit Test:** `runtime_data_device_tracks_seat_lifecycle`
  - **Done:** 建立/銷毀 `wl_data_device_manager` / device，同 protocol objects 對應 library IDs；保留 keyboard/pointer 有效 serial；seat/global 移除清理 pending state，缺 protocol 時唔 crash 或宣稱 Clipboard 可用。

- [x] **TASK-146 — Copy shortcut 發佈真 Wayland selection owner**
  - **Dependencies:** `TASK-043`, `TASK-044`, `TASK-050`, `TASK-056`, `TASK-130`, `TASK-141`, `TASK-145` | **Unit Test:** `runtime_copy_publishes_selected_text_only`
  - **Done:** 有有效選區/serial 先建立 `wl_data_source`、宣告 text MIME 同 set_selection；UTF-8、tabs、空格、hard/soft wrap 照原文抽取；冇選區唔清空外部 Clipboard，source cancellation 撤銷相應 ownership。

- [x] **TASK-147 — Clipboard send FD 接入非阻塞 event loop**
  - **Dependencies:** `TASK-051`, `TASK-146` | **Unit Test:** `runtime_clipboard_send_resumes_partial_writes`
  - **Done:** source send request 用現有 bounded writer，poll writable FD；處理 short write/EINTR/EAGAIN/EPIPE/cancel；限制同時 transfer 數量同 work budget，結束時關 FD，Clipboard client 唔可卡住 UI。

- [x] **TASK-148 — 接收真 data offer / selection lifecycle**
  - **Dependencies:** `TASK-048`, `TASK-052`, `TASK-055`, `TASK-145` | **Unit Test:** `runtime_clipboard_offers_ignore_stale_objects`
  - **Done:** MIME events、selection replacement/None、owner 消失同 object destroy 更新正確 offer ID；用固定 text MIME 次序；未知、非文字或 stale offer 不誤當新 Clipboard。

- [x] **TASK-149 — Paste receive FD 接入有界非阻塞讀取**
  - **Dependencies:** `TASK-053`, `TASK-054`, `TASK-055`, `TASK-148` | **Unit Test:** `runtime_clipboard_receive_is_bounded_and_cancellable`
  - **Done:** 發出正確 offer.receive，FD 由 poll 驅動；partial UTF-8/EOF/EINTR/EAGAIN、失效 offer 同 owner 消失有確定結果；使用 config clipboard cap，超額拒絕唔靜默截斷，取消後釋放 FD/payload。

- [x] **TASK-150 — Paste action 寫入原 Tab PTY queue**
  - **Dependencies:** `TASK-057`, `TASK-058`, `TASK-059`, `TASK-061`, `TASK-130`, `TASK-131`, `TASK-149` | **Unit Test:** `runtime_paste_preserves_payload_and_target`
  - **Done:** Paste 開始時捕捉 TabId/focus token，完成後只向原有效 target enqueue；不 trim/normalize/補 newline；按該 terminal mode 加一次 bracketed framing，queue backpressure 唔遺失、重複或重排 payload。

- [x] **TASK-151 — 非 bracketed 多行 Paste 顯示安全確認 UI**
  - **Dependencies:** `TASK-062`, `TASK-150` | **Unit Test:** `runtime_multiline_paste_waits_for_user_decision`
  - **Done:** 無 bracketed mode 嘅多行 payload 先保留，任何 bytes 都唔送 Shell；native prompt 只顯示必要 metadata，明確接受先繼續；取消/Escape/focus 或 target 失效即丟棄，舊決定唔批准新 paste；唔承諾 bracketed paste 係 sandbox。

- [x] **TASK-152 — 取消 partial Paste 時維持 queue / framing 一致**
  - **Dependencies:** `TASK-060`, `TASK-092`, `TASK-134`, `TASK-150`, `TASK-151` | **Unit Test:** `runtime_partial_paste_cancel_never_crosses_tabs`
  - **Done:** 區分未排隊、已排隊、已寫入同 Tab 已關閉；focus/Tab change 取消未送內容，必要嘅結束 framing 只可送原仍存在 PTY，唔轉送新 Shell；已寫 bytes 不聲稱可撤回；測所有 partial-write 邊界同 pending confirmation。

- [x] **TASK-153 — 接通獨立 Primary Selection / Middle-click**
  - **Dependencies:** `TASK-063`, `TASK-064`, `TASK-065`, `TASK-140`, `TASK-141`, `TASK-147`, `TASK-149`, `TASK-150`, `TASK-152` | **Unit Test:** `runtime_primary_selection_never_uses_clipboard_offer`
  - **Done:** 有 protocol 時用獨立 source/offer/state 發佈選區同 middle-click paste，重用 bounded FD pump 同 paste safety；唔覆寫或 fallback 到一般 Clipboard；缺 protocol 時一般 Copy/Paste 繼續正常。

- [x] **TASK-154 — OSC 52 write opt-in 接入 config 同 Clipboard adapter**
  - **Dependencies:** `TASK-066`, `TASK-067`, `TASK-068`, `TASK-094`, `TASK-145`, `TASK-147`, `TASK-153` | **Unit Test:** `runtime_osc52_respects_explicit_write_policy`
  - **Done:** 明確 config opt-in 才 drain/發佈 bounded OSC 52 writes，仍需有效 seat serial 同正確 selection kind；缺 serial/能力有明確拒絕策略；read 永遠預設拒絕，PTY output 不能自授權；撤銷 opt-in 清理 pending writes。

- [x] **TASK-155 — Clipboard 可恢復錯誤顯示 metadata 提示**
  - **Dependencies:** `TASK-101`, `TASK-147`, `TASK-149`, `TASK-151` | **Unit Test:** `runtime_clipboard_errors_do_not_leak_or_exit`
  - **Done:** empty/non-text offer、size limit、owner 消失、FD failure 同取消各有明確結果；需要提醒嘅錯誤可見，但無 Clipboard text/key/PTY payload；只取消該 transfer，其他 Shell 同下一次 Copy/Paste 仍可用。

## R. Scrollback View / History Interaction

- [x] **TASK-156 — Terminal adapter 暴露有界 viewport scroll**
  - **Dependencies:** `TASK-029`, `TASK-030` | **Unit Test:** `terminal_viewport_scroll_clamps_to_history`
  - **Done:** 以 core API 移動 display offset，page/top/bottom 操作有確定邊界；history=0、alt screen 同淘汰舊行唔越界；操作只移 viewport，唔改 Shell cursor 或 PTY bytes。

- [x] **TASK-157 — Wheel / scroll shortcuts 驅動 viewport**
  - **Dependencies:** `TASK-130`, `TASK-133`, `TASK-140`, `TASK-156` | **Unit Test:** `runtime_scroll_input_targets_active_viewport`
  - **Done:** 無 application mouse ownership 時 wheel 同明確可設定 scroll bindings 操作 active history；新增 bindings 參與 conflict 驗證；按鍵輸入/新 output 時 follow-bottom policy 有記錄，其他 Tab offset 不變。

- [x] **TASK-158 — Raster 正確呈現歷史 viewport**
  - **Dependencies:** `TASK-126`, `TASK-156` | **Unit Test:** `runtime_history_rows_use_display_offset`
  - **Done:** 負 history row 依 display offset 映射可見 row，唔全部畫到第零行；cursor 離開 viewport 時不誤畫；resize/reflow、CJK 同 soft wrap 在真視窗可見且無殘影。

- [x] **TASK-159 — 從 scrollback 選取同 Copy 正確文字**
  - **Dependencies:** `TASK-144`, `TASK-146`, `TASK-156`, `TASK-158` | **Unit Test:** `runtime_history_selection_copies_original_lines`
  - **Done:** pointer hit-test/selection anchor 同 history offset 一致，跨可見頁選區按已定義策略保留或明確失效；保留 hard/soft wrap、CJK/combining 同空格；history eviction 唔複製成另一段文字。

## S. Terminal Protocol / Daily TUI Runtime

- [x] **TASK-160 — 將 terminal core replies 送回來源 PTY**
  - **Dependencies:** `TASK-023`, `TASK-026`, `TASK-132` | **Unit Test:** `runtime_terminal_replies_return_to_source_pty`
  - **Done:** 接住目前忽略嘅 `Event::PtyWrite`，建立有界、按 Tab 排序嘅 reply queue；DA/DSR/CPR 等 core 回覆透過該 PTY write path 發送一次；處理 partial writes/overflow，唔經 active focus routing 或顯示成文字。

- [x] **TASK-161 — 回覆實際 palette / terminal geometry queries**
  - **Dependencies:** `TASK-136`, `TASK-160` | **Unit Test:** `runtime_queries_report_actual_colors_and_geometry`
  - **Done:** 處理 core `ColorRequest` 同 `TextAreaSizeRequest`，由實際 palette、content rows/columns/cell pixels 格式化回覆來源 Tab；未知 query 不虛報能力，queue 有上限，唔將 ClipboardLoad 夾帶成可讀 Clipboard 授權。

- [x] **TASK-162 — 接通 application mouse reporting**
  - **Dependencies:** `TASK-027`, `TASK-140`, `TASK-141`, `TASK-157`, `TASK-160` | **Unit Test:** `runtime_mouse_reports_only_enabled_modes`
  - **Done:** 按 terminal 已啟用嘅 button/motion/SGR 等已支援 modes 編碼 click/release/wheel/cell coordinates；明確 selection override 同 Tab Bar/Primary ownership，避免同一事件既 paste 又 report；未支援模式唔宣稱可用。

- [x] **TASK-163 — 接通 terminal focus-in / focus-out reports**
  - **Dependencies:** `TASK-027`, `TASK-133`, `TASK-160` | **Unit Test:** `runtime_focus_reports_follow_tab_focus`
  - **Done:** 只喺 application 啟用 focus-report mode 時對應來源 Tab 發送正確 focus transition；切 Tab、window leave、seat 移除無重複或 stale report；disabled mode 完全唔發送。

- [x] **TASK-164 — 補齊日常 legacy keypad / key mode coverage**
  - **Dependencies:** `TASK-012`, `TASK-027`, `TASK-128`, `TASK-130` | **Unit Test:** `legacy_keypad_obeys_terminal_modes`
  - **Done:** 針對現有 encoder 未處理嘅 keypad keys/APP_KEYPAD、NumLock 同 modified navigation 建立明確 mapping；不改已驗證 Enter/arrows/Ctrl+C/F-keys；unsupported keys 明確記錄，普通 legacy Shell 唔意外收到 Kitty-only sequences。

- [x] **TASK-165 — 明確使用者 click 才執行 hyperlink opener**
  - **Dependencies:** `TASK-100`, `TASK-140`, `TASK-158` | **Unit Test:** `runtime_hyperlink_open_requires_explicit_click`
  - **Done:** 將既有 Ctrl+click HTTP(S) gate 接到 adapter，以 literal argv 開啟連結，唔經 shell 字串；scrollback hit-test 正確，普通 output/hover 唔執行；失敗只回報 metadata，無 URI payload logs。

- [x] **TASK-166 — Synchronized update 使用有界 runtime timeout**
  - **Dependencies:** `TASK-006`, `TASK-017`, `TASK-026`, `TASK-127` | **Unit Test:** `runtime_sync_update_timeout_releases_frame`
  - **Done:** 解決 `NoSync` 註記：支援 core 同步更新時延後 presentation 至結束或有界 deadline，缺結束 sequence 亦唔永久凍結畫面；不停止 keyboard/PTY drain，唔用固定 busy polling；未接通前不宣稱支援。

## T. HiDPI / Rendering / Basic Colors

- [x] **TASK-167 — Runtime 套用 output scale 同 buffer geometry**
  - **Dependencies:** `TASK-015`, `TASK-031`, `TASK-125`, `TASK-126`, `TASK-140` | **Unit Test:** `runtime_scale_keeps_logical_and_buffer_coordinates_consistent`
  - **Done:** 綁定 output/surface scale lifecycle，移動視窗或 scale=1↔2 後更新 buffer scale、pixel size、字型 metrics 同 hit-test；logical/physical 唔混用；較新 scale protocol 缺席時有一致 fallback，分配先檢查上限。

- [x] **TASK-168 — 真字型 fallback 依 glyph coverage 選擇**
  - **Dependencies:** `TASK-028`, `TASK-033`, `TASK-034`, `TASK-126` | **Unit Test:** `runtime_font_fallback_covers_cjk_and_combining`
  - **Done:** 唔只用單一「繁」字揀 fallback；以 bounded 候選/實際 coverage 處理繁體中文、combining、wide glyph；缺字有明確 fallback，baseline/cell width 對齊；用可信 font fixtures 做 pixels/metrics 回歸，唔擴張成完整 shaping 承諾。

- [x] **TASK-169 — Raster 呈現已支援 SGR 文字樣式**
  - **Dependencies:** `TASK-035`, `TASK-126`, `TASK-168` | **Unit Test:** `runtime_sgr_styles_match_terminal_cells`
  - **Done:** 明確支援並測試 bold/dim/italic、underline variants、strikeout 同 inverse/hidden 嘅畫面效果；cache key 分辨需要嘅 face/style/size，唔錯用 regular mask；不支援樣式列明，terminfo 唔虛報。

- [x] **TASK-170 — Cursor blink 跟 mode / focus 事件驅動**
  - **Dependencies:** `TASK-006`, `TASK-036`, `TASK-104`, `TASK-126` | **Unit Test:** `runtime_cursor_blink_has_no_idle_busy_loop`
  - **Done:** 消費 core cursor-blink state，fake clock 驗證有限 deadline；Hidden/unfocused/非 active view 不持續 blink，Block/Beam/Underline 正確恢復；只有 cursor dirty 時才排 frame，唔靠高頻輪詢。

- [x] **TASK-171 — Runtime 採用正確 dirty-region presentation**
  - **Dependencies:** `TASK-016`, `TASK-038`, `TASK-091`, `TASK-126`, `TASK-143`, `TASK-158` | **Unit Test:** `runtime_partial_damage_preserves_other_pixels`
  - **Done:** terminal/selection/preedit/cursor damage 接到 raster 同 surface damage；double-buffer reuse 保留未更新 pixels，resize/Tab switch 需要時完整重畫；一格更新唔無條件重畫整窗，idle/hidden view 不新增 frame。

- [ ] **TASK-172 — TOML basic color palette schema 同驗證**
  - **Dependencies:** `TASK-094`, `TASK-098` | **Unit Test:** `config_palette_validates_and_defaults`
  - **Done:** 提供 scope 內 basic foreground/background/cursor/ANSI colors，缺省維持有效 palette；無效格式/範圍明確報錯，唔自動改設定檔；測部分 override 同 malformed input。

- [ ] **TASK-173 — Runtime 套用設定 palette 同 core color overrides**
  - **Dependencies:** `TASK-161`, `TASK-172`, `TASK-126` | **Unit Test:** `runtime_palette_changes_pixels_and_query_replies`
  - **Done:** raster、cursor/overlay 同 color query 共用已解析 palette；設定、SGR index、OSC color/reset precedence 有一致結果；唔只 parse 但仍使用硬編碼 colors。

- [ ] **TASK-174 — 超額 configure / buffer request 安全退讓**
  - **Dependencies:** `TASK-010`, `TASK-136`, `TASK-167` | **Unit Test:** `runtime_oversized_configure_keeps_valid_sessions`
  - **Done:** 當視窗/HiDPI 尺寸超過目前 surface 或 terminal cap，使用符合 xdg configure 規則嘅有界尺寸/錯誤策略；唔 overflow、OOM 或因普通 resize 無提示殺掉所有 Shell；清楚記錄可支援上限同實際退讓行為。

## U. IME Remaining Runtime Integration

- [ ] **TASK-175 — IME focus / commit 接入真多 Tabs**
  - **Dependencies:** `TASK-093`, `TASK-129`, `TASK-133`, `TASK-134` | **Unit Test:** `runtime_ime_tab_switch_invalidates_old_batches`
  - **Done:** 移除 IME 固定 `TabId(1)`，switch/close 時取消舊 preedit/pending batch、更新 enable/disable transaction；commit 只寫原有效 Tab，延遲 done 不跨 Tab；真多 Tabs integration 同單窗 TASK-129 驗收分開記錄。

- [ ] **TASK-176 — IME overlay / candidate anchor 跟實際 viewport**
  - **Dependencies:** `TASK-076`, `TASK-077`, `TASK-129`, `TASK-137`, `TASK-158`, `TASK-167`, `TASK-175` | **Unit Test:** `runtime_ime_anchor_tracks_viewport_and_long_preedit`
  - **Done:** 用同一 content origin/scale 計算 overlay 同 surface-local cursor rectangle；長 preedit 喺右/下邊界有明確有界換行或可視 caret 策略，唔再無說明只 clip 一行；scroll、resize、Tab Bar、CJK 同 byte-offset selection 唔錯位，grid/PTY 不受 preedit 改寫。真候選視窗仍須 desktop 驗收。

## V. Runtime Resource / Failure Boundaries

- [ ] **TASK-177 — Runtime budgets 接入所有資源 owner**
  - **Dependencies:** `TASK-097`, `TASK-105`, `TASK-131`, `TASK-149`, `TASK-168` | **Unit Test:** `runtime_resource_caps_apply_to_actual_owners`
  - **Done:** 核對並接通 config caps 同跨 Tab reservation，glyph/font cache 按 Window 共用；clipboard payload/transfer count、PTY queues、grid/history 同 frame buffers 有明確總上限；新增/關閉/resize 會正確保留或回收 budget，不只測孤立 counter。

- [ ] **TASK-178 — PTY input backpressure 唔令整個 app 退出**
  - **Dependencies:** `TASK-023`, `TASK-132`, `TASK-150`, `TASK-160` | **Unit Test:** `runtime_input_backpressure_preserves_order_without_global_exit`
  - **Done:** keyboard、IME commit、paste 同 protocol reply 共用來源 Tab 嘅有界 ordered write policy；queue full/partial write 有明確 retry 或可見拒絕結果，不靜默遺失或重複 bytes；一個慢 child 唔令其他 Tabs/Wayland 停止。

- [ ] **TASK-179 — 不可信 stream failure 隔離到來源 Tab**
  - **Dependencies:** `TASK-099`, `TASK-101`, `TASK-106`, `TASK-132`, `TASK-135` | **Unit Test:** `runtime_bad_stream_does_not_stop_other_tabs`
  - **Done:** parser resource violation、oversized OSC 同 PTY I/O failure 只停止/標記所屬 session，保留可讀 metadata 同其他 Tabs；唔 retry 已部分消費嘅 bytes、唔轉送新 Tab；close/cleanup 後 budgets/FD/child 正確釋放。

## W. Kitty Keyboard Runtime（獨立交付）

- [ ] **TASK-180 — Drain Kitty query replies 避免 queue 飽和**
  - **Dependencies:** `TASK-109`, `TASK-127`, `TASK-160` | **Unit Test:** `runtime_kitty_queries_drain_before_reply_queue_fills`
  - **Done:** `take_keyboard_reply()` 接入來源 PTY，包括同一 read chunk 多次 query；即使 enhancement 尚未啟用亦回覆真實 0 flags；重複 query 不因 runtime 漏 drain 而令第 33 次關閉 terminal；queue/backpressure 有界、錯誤不跨 Tab。

- [ ] **TASK-181 — 定義可宣告 Kitty flags 嘅完整 key coverage**
  - **Dependencies:** `TASK-108`, `TASK-110`, `TASK-111`, `TASK-164` | **Unit Test:** `kitty_supported_flags_match_complete_encoder_coverage`
  - **Done:** 對擬宣告 subset 建立 printable/control/function/keypad/modified press-repeat-release byte fixtures，包括 Shift+Enter；補齊該 flags 要求嘅 encoding，未完成旗標仍拒絕/不宣告；唔因已有 bit definition 就開啟全 flags。

- [ ] **TASK-182 — Negotiated Kitty mode 驅動 active Tab encoder**
  - **Dependencies:** `TASK-112`, `TASK-113`, `TASK-130`, `TASK-133`, `TASK-180`, `TASK-181` | **Unit Test:** `runtime_kitty_negotiation_controls_active_encoder`
  - **Done:** 將每 Tab main/alt stack current flags 提供 runtime key encoder；set/push/pop/query/reset 都一致；只宣告已測 subset，legacy apps 維持原輸入；Tab/screen switch 不混入另一個 mode stack。

- [ ] **TASK-183 — Enhanced keyboard 保持 IME / shortcut ownership**
  - **Dependencies:** `TASK-114`, `TASK-175`, `TASK-182` | **Unit Test:** `runtime_kitty_ime_ownership_never_duplicates_text`
  - **Done:** 真 runtime 下 shortcut/IME-consumed key、press/repeat/release 同 commit 有單一 owner；中文 commit 唔再被 Kitty encoder 重送，focus/tab/IME restart 不洩漏舊事件；legacy 同 enhanced native probes 分開通過。

## X. Kitty Graphics Runtime（Direct Only，Keyboard 之後）

- [ ] **TASK-184 — PTY APC stream 接入 bounded Graphics parser**
  - **Dependencies:** `TASK-115`, `TASK-121`, `TASK-132`, `TASK-183` | **Unit Test:** `runtime_graphics_apc_handles_split_streams`
  - **Done:** 每 Tab 有持續 parser state；ESC/APC/header/payload 任意 split 同混合正常 ANSI output 都正確路由；malformed/超長序列有界；file/temp/shared-memory transport 明確拒絕，唔讀取任何指定本機檔案。

- [ ] **TASK-185 — Direct chunks 解碼後存入來源 Tab image store**
  - **Dependencies:** `TASK-116`, `TASK-117`, `TASK-118`, `TASK-119`, `TASK-177`, `TASK-184` | **Unit Test:** `runtime_graphics_transfer_decodes_once_with_limits`
  - **Done:** dispatcher 將 direct PNG/RGBA chunks 經 assembler/decode/store 串起；完整 payload 才存入一次，config byte/pixel caps 於 allocation 前生效；取消/壞資料不留下半張 image，Tab 之間 image ID 隔離。

- [ ] **TASK-186 — Graphics commands 產生正確 placement**
  - **Dependencies:** `TASK-119`, `TASK-185` | **Unit Test:** `runtime_graphics_placement_uses_current_screen_geometry`
  - **Done:** 對宣告子集處理 transmit-and-display/place，以 cursor/content geometry 建立 screen-scoped placements；支援字段嘅 cell/pixel/crop/cursor 行為有 fixtures，未支援組合明確拒絕；唔只建立 image 而冇位置。

- [ ] **TASK-187 — Raster 畫出可見 image placements**
  - **Dependencies:** `TASK-126`, `TASK-158`, `TASK-167`, `TASK-186` | **Unit Test:** `runtime_graphics_pixels_respect_clipping_and_layers`
  - **Done:** active screen placements 經有界 alpha composition 呈現；scale、clip、viewport offset 同宣告 layering 一致，唔畫 hidden Tab；固定 PNG/RGBA fixture 以 compositor pixels 驗證，文字/cursor/overlay 不意外消失。

- [ ] **TASK-188 — Graphics query / success / error 回覆來源 PTY**
  - **Dependencies:** `TASK-160`, `TASK-184`, `TASK-185`, `TASK-186` | **Unit Test:** `runtime_graphics_replies_follow_query_and_quiet_policy`
  - **Done:** 為宣告嘅 direct subset 產生正確 ID、success/error/query replies，遵從 quiet policy；query 不意外留下 image/placement；malformed/unsupported/limit cases 有 bounded 回覆，唔包含 payload 或跨 Tab。

- [ ] **TASK-189 — Graphics delete / reset / Tab close 回收資源**
  - **Dependencies:** `TASK-120`, `TASK-134`, `TASK-185`, `TASK-187` | **Unit Test:** `runtime_graphics_lifecycle_reclaims_owned_resources`
  - **Done:** 真 delete dispatcher 移除正確 image/placement 並 repaint；screen/reset/scroll 淘汰/Tab close 清理符合已記錄 subset 嘅 ownership，保留仍被引用資源；pending chunks、cache 同 budget 可回收，ID 唔影響別 Tab。

- [ ] **TASK-190 — Graphics 跨 Tab 總 budget 同 decode work 限制**
  - **Dependencies:** `TASK-118`, `TASK-177`, `TASK-185`, `TASK-189` | **Unit Test:** `runtime_graphics_flood_stays_within_global_budget`
  - **Done:** image cache、placements、pending/decoded buffers 同時存在時計入跨 Tab 總額；reject/replace/delete 正確回收；大量 chunks/decode 唔無界佔用 RAM 或令 UI 永遠收唔到 input；malicious PNG/尺寸/數量 fixtures 驗證。

## Y. Packaging / Operator Documentation

- [ ] **TASK-191 — 執行可回滾 user-local / staged install plan**
  - **Dependencies:** `TASK-122`, `TASK-124` | **Unit Test:** `install_adapter_rolls_back_only_created_files`
  - **Done:** filesystem adapter 安裝 binary/terminfo/desktop entry，用 exclusive creation 同 symlink-race 防護；記錄本次建立檔案，途中失敗只回滾自己產物；測 fake FS 同臨時 prefix。`/usr/local` 只提供可審核路徑，實際系統寫入仍需另行授權。

- [ ] **TASK-192 — 安全執行 uninstall / rollback receipt**
  - **Dependencies:** `TASK-191` | **Unit Test:** `uninstall_preserves_unowned_or_changed_files`
  - **Done:** 只接受成功 install receipt 內 Ptyra-owned paths；核對檔案身份，遇到替換/外部修改/符號連結唔誤刪，唔刪 parent 或其他程式；重複 uninstall 可安全處理，真正系統移除仍需授權。

- [ ] **TASK-193 — 驗證 packaged launcher / terminfo 同 binary 一致**
  - **Dependencies:** `TASK-020`, `TASK-123`, `TASK-124`, `TASK-191` | **Unit Test:** `packaged_capabilities_match_runtime_support`
  - **Done:** source terminfo、compiled asset、private runtime TERMINFO 同 install entry 有可重現檢查；desktop launcher 可找到正確 binary；僅按實際已通過功能增補 TUI capabilities，未完成 Kitty/mouse/render 特性唔虛報；臨時 prefix 啟動不依賴 repo cwd。

- [ ] **TASK-194 — 提供同實際功能一致嘅 config / 使用文件**
  - **Dependencies:** `TASK-130`, `TASK-154`, `TASK-172`, `TASK-193` | **Unit Test:** `documented_config_examples_parse_with_defaults`
  - **Done:** 提供可解析 default/example config、shortcut、paste 安全策略、caps、安裝/移除同必要 native dependencies 說明；README/task count/能力表以 TASKS 狀態更新，明列未驗收項；不聲稱零 native dependencies、full Kitty 或已比其他 terminal 快。

## Z. Repeatable Verification / Performance Tooling

- [ ] **TASK-195 — 建立固定 workload 嘅資源量測 harness**
  - **Dependencies:** `TASK-132`, `TASK-137`, `TASK-177` | **Unit Test:** `benchmark_samples_report_single_core_cpu_and_pss`
  - **Done:** 固定 window/font/scale/workload，量測 1/5/10 Tabs 嘅 CPU（單核基準）、PSS/RSS、frame count 同可用 GPU memory；metadata 包含版本/時間/平台，區分 Ptyra 同 child；缺值標 NOT AVAILABLE，結果唔偽裝 PASS，schema/計算用 fixture 驗證。

- [ ] **TASK-196 — 建立可比較 renderer 嘅同輸入 benchmark driver**
  - **Dependencies:** `TASK-030`, `TASK-195` | **Unit Test:** `renderer_benchmark_replays_identical_frames`
  - **Done:** 現有 SHM backend 同候選 backend 接收相同 grid/dirty/scale/workload 同 correctness fixtures；記錄 CPU/PSS/RSS/GPU、frame timings 同 failure，結果可重跑；不以不同字型/視窗/工作量冒充比較。

- [ ] **TASK-197 — 建立最小 EGL/GLES renderer 比較 PoC**
  - **Dependencies:** `TASK-126`, `TASK-167`, `TASK-196` | **Unit Test:** `gpu_renderer_probe_preserves_frame_and_resource_bounds`
  - **Done:** 只為 `SCOPE.md` 尚未決定嘅 backend 比較提供最小候選 adapter，使用相同 frame fixtures 同資源上限；fake context 驗證 lifecycle，真 AMD/Mesa 測 correctness/穩定性/量測；唔未經結果就替換預設 renderer。新增 native dependency/unsafe 須依 AGENTS 評審。

- [ ] **TASK-198 — 建立多 Tabs 長時間 soak runner**
  - **Dependencies:** `TASK-135`, `TASK-152`, `TASK-175`, `TASK-179`, `TASK-195` | **Unit Test:** `soak_runner_records_failures_and_cleanup`
  - **Done:** 可設定時長/固定 seed，重複 output、resize、switch、close/reopen 同已接通 input/clipboard/IME scenarios；fake process 測 runner 停止/timeout/紀錄，真執行追蹤 crash/FD/child/resource 趨勢；只存測試 metadata，完成報告區分未跑場景同失敗。

- [ ] **TASK-199 — 將 malformed fixtures 擴展到 runtime boundaries**
  - **Dependencies:** `TASK-106`, `TASK-149`, `TASK-160`, `TASK-175`, `TASK-179` | **Unit Test:** `runtime_malformed_events_remain_bounded_and_isolated`
  - **Done:** 固定 seed/property tests 覆蓋 Wayland object/serial lifecycle、Clipboard FD cancellation、IME stale done、PTY query flood 同 parser 限額；驗證不 panic/無界 allocation/busy loop/跨 Tab 輸入；Kitty query flood 另見 TASK-180，Graphics APC/decode/store 限額另見 TASK-184/TASK-190，唔令基本安全 gate 依賴後續 Kitty 交付；未接通部分明確標示，唔以幾個 fixture 宣稱全面 fuzz 完成。

## Integration / Acceptance Matrix（唔當作 Unit TASK）

呢啲係真機 Gate，需人工或系統整合測試。對應 TASK 完成及 unit test 通過只係前提，唔代表 Gate 已通過。原有 gates 保留未勾選；使用者目前未能驗收，唔可因新增 backlog、headless PASS 或 git commit 而勾選。每次記錄實際 OS/compositor/IME/GPU 版本、commit、測試步驟同 PASS / FAIL / NOT RUN；Sway 1.9 headless 結果唔代替指定 Sway 1.10.1 desktop。

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

### Gate Evidence / 尚欠驗收範圍（保留原有 gates，唔係額外已完成 TASK）

- **Basic Terminal / Daily TUI:** `TASK-160`–`TASK-166`、`TASK-193` 後，真 Shell/`vim`/`htop`/`tmux`/`Codex` 測 DA/DSR、color/size queries、navigation/keypad、mouse/focus、alt screen、resize；保存支援/不支援矩陣。
- **Clipboard / Selection:** `TASK-140`–`TASK-159` 後，Firefox ↔ Ptyra ↔ Kitty 逐方向測繁體中文、combining、spaces/tabs、hard/soft wrap、empty/large payload；有/無 bracketed mode、多行接受/取消、owner 消失、partial transfer 中切/關 Tab 全部獨立記錄。
- **Tabs:** `TASK-131`–`TASK-139`、`TASK-152`、`TASK-175` 後，5–10 個真 Shell 同時輸出，確認 click/shortcut、新增/關閉、title、history/selection、child exit、背景 output 同 keyboard/Clipboard/IME target 隔離。
- **Fcitx5:** `TASK-129` 仍等實際 Sway 1.10.1 desktop 候選 popup；`TASK-175`/`TASK-176` 另驗多 Tabs、長組字、視窗邊界、scale/resize/scroll、快速切 focus 同 restart。無 popup screenshot/操作證據不可寫 candidate-position PASS。
- **Rendering / HiDPI:** `TASK-167`–`TASK-176` 後，AMD + Mesa + Sway 真畫面測 CJK/combining、styles/colors、cursor、history/selection/preedit、scale=1/2 同移動 output；記錄不支援 scale/字型行為，唔用 headless SHM 結果冒充 GPU 驗收。
- **Performance / Backend Decision:** `TASK-195`–`TASK-197` 後，同機同 workload 比較 SHM 同 EGL/GLES，按 correctness/CPU/PSS/RSS/GPU/穩定性記錄正式 backend 決定；對照 `SCOPE.md` 80 MiB / +15 MiB / <1% 單核 provisional targets，未達標列偏差，唔靜默改目標。比較失敗或 NOT RUN 不能當成已選定 backend。
- **Stability / Security:** `TASK-177`–`TASK-179`、`TASK-198`/`TASK-199` 後，記錄實際 soak 時長同 resource 趨勢；檢查無殘留 FD/child、無未解釋退出、無 payload logs/telemetry，以及 malformed input / config / protocol 缺席後嘅可恢復性。
- **Packaging:** `TASK-191`–`TASK-194` 後，在暫存 user-local prefix 驗證安裝、啟動、terminfo、失敗回滾同 uninstall；真 `/usr/local` 安裝唔係測試前提，未經授權不執行。
- **Kitty Keyboard:** `TASK-180`–`TASK-183` 後，官方 protocol probes 驗 query/negotiation/stack、Shift+Enter、press/repeat/release、screen/tab isolation 同 IME collision；只宣告有 byte-level 同 native 證據嘅 flags subset。
- **Kitty Graphics:** `TASK-184`–`TASK-190` 後，官方 direct PNG/RGBA probes 驗 chunks、query/quiet replies、placement/clip/delete、screen/tab/resize lifecycle 同限額；file/temp/shared-memory 一律保持不支援。Keyboard PASS 唔代表 Graphics PASS。

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
