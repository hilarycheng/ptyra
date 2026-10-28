# Ptyra

> **Planning Draft v0.2 · 2026-10-27 · 未 APPROVE**

Ptyra 係一個規劃中嘅 Rust Native Wayland Terminal。

目標：低 RAM、低 Idle CPU、可靠 Copy & Paste、Fcitx5 中文輸入、GUI Tabs。Kitty Keyboard / Graphics 放喺後續獨立 TASK。

## Planning Files

- [`SCOPE.md`](SCOPE.md) — 定義產品、架構、安全、性能與驗收邊界。
- [`TASKS.md`](TASKS.md) — **124 個 Atomic TASK**，全部用 `- [ ]` Checkbox，設有 Dependencies、Unit Test、Done Criteria。
- [`AGENTS.md`](AGENTS.md) — Coding Agent 必須遵守嘅執行、測試、安全同批准規則。

## Status

- 文件只係 Draft，未開始實作程式。
- 全部 TASK 狀態為 `[ ]`，Unit Test 名稱係設計規格，**未執行**。
- 使用者明確 `APPROVE` 指定 TASK 後，先開始 Code / Build / Install。
- 不會改動現有 Sway、Fcitx5、Mesa 或 Terminal 設定。

## How to Use

1. 讀 `SCOPE.md`。
2. 喺 `TASKS.md` 選一個 Dependencies 已完成嘅 `TASK-XXX`。
3. 按 `AGENTS.md` 建立 Unit Test 同最小實作。
4. Unit Test、Regression 同所需 Acceptance Checks 通過後，先把 `[ ]` 改為 `[x]`。
