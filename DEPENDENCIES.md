# Dependency review — TASK-012 (2026-10-29)

## TOML parser — TASK-094 (2026-11-05)

`toml = 1.1.6+spec-1.1.0` parses config syntax instead of implementing a partial TOML parser.
Only `std`, `parse`, and `serde` are enabled; no serialization/display feature or unbounded parser mode.
Config input is capped at 64 KiB. Parser errors are reduced to static reason/field and byte offset.
Raw values and parser diagnostic excerpts are not logged.

The [upstream release](https://github.com/toml-rs/toml/releases/tag/toml-v1.1.6) was tagged on 2026-09-10;
the upstream repository is active, not archived. This pinned version was already cached locally.
Cargo.lock adds these packages; manifest licenses were checked:

| Package | Version | License |
|---|---|---|
| toml | 1.1.6+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 |
| toml_writer (lockfile only) | 1.2.0+spec-1.1.0 | MIT OR Apache-2.0 |
| winnow | 1.0.4 | MIT |

The complete official [RustSec tree at 26fd2d4](https://github.com/RustSec/advisory-db/tree/26fd2d43f7280a1fb925a3743ec463017afe7119)
was checked for all six added package names. No matching advisory records were found.
This is a manual advisory check, not a cargo-audit run or a guarantee of vulnerability-free code.


`kbvm = 0.2.0`, with default features disabled, implements XKB parsing and client-side keysym lookup. A real parser is needed for compositor-supplied layouts; a hard-coded US key table cannot meet TASK-012. No GUI framework, X11 connection, Compose engine, or logging feature is enabled. `pkg-config` is a build helper; no new system package is installed.

## Maintenance and licensing

The [upstream repository](https://github.com/mahkoh/kbvm) was not archived when checked. The registry reports 0.2.0 released on 2026-09-24, and the repository last pushed on that date. The pinned crate checksum was verified against registry metadata. It requires Rust 1.95; this change was tested with Rust 1.97.1. Upstream documents [compatibility differences](https://github.com/mahkoh/kbvm/blob/master/docs/compatibility.md); this adapter uses client lookup, not the compositor state machine.

Cargo.lock records the following dependencies, including build/proc-macro and lockfile-only entries. Licenses come from their package manifests. The direct dependency's maintenance status was checked; this is not a source/security audit of every transitive dependency.

| Package | Locked version | Declared license |
|---|---|---|
| arrayvec | 0.7.8 | MIT OR Apache-2.0 |
| bstr | 1.13.1 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| debug-fn | 1.0.0 | MIT OR Apache-2.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| foldhash | 0.2.0 | Zlib |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| isnt | 0.2.0 | MIT OR Apache-2.0 |
| kbvm | 0.2.0 | MIT OR Apache-2.0 |
| kbvm-proc | 0.3.0 | MIT OR Apache-2.0 |
| linearize | 0.1.8 | MIT OR Apache-2.0 |
| linearize-derive | 0.2.2 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| secure-execution | 0.1.0 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| smallvec | 1.16.4 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |

## Advisory check

The complete, non-truncated official [RustSec tree at 40f5be6](https://github.com/RustSec/advisory-db/tree/40f5be6fb12ed93fc591bf4ab23b7d793b3f2239) was matched against every locked package name. Six records matched:

- hashbrown: RUSTSEC-2024-0402 is patched in >=0.15.1; the lockfile uses 0.17.1.
- smallvec: RUSTSEC-2018-0003, 2018-0018, 2019-0009, 2019-0012, and 2021-0003 are patched by the selected 1.16.4. The newest of these requires >=1.6.1 on the 1.x line.

No matching affected version was found. This was a manual registry/advisory review, **not a cargo-audit run**, and is not a guarantee that the dependencies have no vulnerabilities.

## Input and privacy boundaries

The adapter accepts self-contained text-v1 keymap bytes, with one optional trailing NUL. It rejects input above 256 KiB and embedded NULs. Context environment/default includes are disabled; include count/depth are zero, so the parser does not load files named by the payload. Warnings/errors fail closed and are converted to payload-free AppError metadata. Keymap and translated-key payloads have no Debug implementation.

KBVM's parser has an expression nesting limit; Ptyra additionally caps operator count per statement at 128 before parsing to bound left-associative AST depth. Its preflight follows KBVM 0.2.0's quoted string, key-name, `#`, and `//` comment boundaries. KBVM's work budget is 100,000 abstract steps (not a measured time limit). The parsed map is capped at 4,096 keys, 256 levels per group and 16 symbols per level before constructing the lookup table. KBVM stores large sparse key codes in maps; large level indexes are mappings rather than allocations sized by the index. Custom layouts beyond these limits are explicitly rejected.

No application unsafe or FFI was added. An isolated temporary compatibility check used the installed libxkbcommon 1.6.0 to generate a normal US/Greek serialized keymap from `/usr/share/X11/xkb`; ctypes signatures and object ownership were explicit, allocations freed in reverse order, and the helper had 256 MiB/10 CPU-second process limits. It did not connect to a display or modify configuration. The fixture was 70,067 bytes and the new adapter translated both layouts correctly. This is **offline library compatibility**, not Wayland/PTY/Fcitx5 integration acceptance.

The Wayland adapter still needs to bound/read the keymap FD, validate the protocol format, update modifier masks from compositor events, and replace the keymap atomically. Terminal byte encoding, Compose, IME ownership, and repeat timing are later tasks. Full Sway/Fcitx5/PTY checks remain NOT RUN.

# Dependency and unsafe-boundary review — TASK-021 (2026-10-29)

The Linux-only PTY spawn adapter pins `nix = 0.31.3`, with default features disabled and `fs`, `process`, `signal`, and `term` enabled. It uses Linux PTYs and libc; it does not add a GUI runtime or install a system package. The existing `std::process::Command` helpers remain responsible for literal argv and the conservative terminal environment.

## Backend choice and maintenance

[`portable-pty` 0.9.0](https://docs.rs/portable-pty/0.9.0/portable_pty/) (MIT, released 2025-02-11) was evaluated, including its Unix spawn and CommandBuilder source. Its builder resolves HOME/SHELL and silently substitutes HOME when an explicit cwd is not a directory. Its post-fork descriptor cleanup also enumerates descriptors with Rust library operations. That API does not preserve this task's strict explicit-cwd/error semantics without extra adaptation. It was not added to Cargo.lock.

[`nix` 0.31.3](https://docs.rs/nix/0.31.3/nix/pty/) provides the narrower PTY/FD/signal primitives needed with the existing Command. The [registry version record](https://crates.io/api/v1/crates/nix/0.31.3) reports release on 2026-05-11, MIT license, Rust 1.69 minimum, and not yanked. The [upstream repository](https://github.com/nix-rust/nix) was not archived and last pushed on 2026-10-18 when checked. Ptyra still requires Rust 1.95 through KBVM and was tested using Rust 1.97.1.

The following packages are newly locked; existing locked versions are unchanged. Their downloaded crate archives, lockfile checksums and official registry checksums match. `cfg-if 1.0.5` is reused from the prior dependency set.

| Package | Locked version | Declared license |
|---|---|---|
| nix | 0.31.3 | MIT |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| cfg_aliases | 0.2.2 | MIT |

The official, non-truncated RustSec tree was fetched again at [40f5be6](https://github.com/RustSec/advisory-db/tree/40f5be6fb12ed93fc591bf4ab23b7d793b3f2239). Matching the new package names and reused cfg-if found one advisory: [RUSTSEC-2021-0119](https://rustsec.org/advisories/RUSTSEC-2021-0119.html), a `getgrouplist` issue patched in nix >=0.23.0. The chosen 0.31.3 is outside the affected range, and the adapter does not use that API. No other matching advisory was present for these packages. This is a manual registry/advisory check, not cargo-audit or a full source audit; absence of a matching advisory does not guarantee safety.

## Native ownership and unsafe boundaries

- Master and slave are opened with atomic `O_CLOEXEC`. Slave duplicates use Rust's CLOEXEC duplication. Parent slave handles close on both success and failure. Only the master is nonblocking; the child's stdio stays blocking.
- `TIOCSWINSZ` receives a live owned slave FD and a pointer to a fully initialized, stack-owned `winsize` for the duration of that ioctl. It sets initial dimensions only, not TASK-024 runtime resize behavior.
- Rust's unsafe `CommandExt::pre_exec` runs after std has installed the slave on FD 0/1/2. The callback uses only setsid, the Linux TIOCSCTTY ioctl, sigaction and sigprocmask, with stack data prepared before fork. There is no allocation, logging, formatting, lock acquisition or directory enumeration after fork. TIOCSCTTY's force argument is zero; the new session does not steal another session's controlling terminal.
- Signal defaults are restored for HUP/INT/QUIT/TERM/PIPE/CHLD/TSTP/TTIN/TTOU, and the child signal mask is cleared. These changes affect only the new child. Existing unrelated inheritable FDs are the caller's responsibility; this adapter guarantees CLOEXEC for the descriptors it creates.
- A successful child is immediately wrapped in an owning guard. Failed exec/setup is reaped by std::Command. Explicit `terminate` closes the master and kills/reaps only the owned direct child; Drop is a best-effort fallback with no payload logging. The guard uses Child's reaped-status cache, avoiding signals to an already-reaped/reused PID. A kernel teardown wait may block; no reaper/event-loop thread is introduced. Detached descendants are not recursively killed, and this is not a shell sandbox.

## Input and test boundaries

Spawn requires an absolute executable path and absolute cwd. No PATH search, HOME fallback, shell-command concatenation or global environment mutation is added. Arguments retain the TASK-019 limits (256 arguments, 64 KiB including NUL separators); cwd is capped at 4,096 bytes and rejects NUL. Rows/columns are nonzero u16 values. `TERM=dumb` and removal of `COLORTERM` preserve TASK-020 until real terminal features exist. Other environment variables are inherited normally.

Typed spawn errors retain only TabId, the validation/open/spawn stage, and AppError metadata. Requests and sessions deliberately have no payload-bearing Debug implementation. Unit tests inject open/spawn failures and track independent resource ownership. Linux integration fixtures use fixed `/bin/sh -c` scripts with separate literal arguments, `/usr/bin/stty`, a five-second parent deadline, a 16 KiB output cap, and owned-child cleanup. They check stdio/control-terminal setup, size, argv bytes, environment, cwd failure, CLOEXEC, nonblocking master, repeated failure FD accounting, child reaping and session isolation. The test-only read loop does not implement TASK-022's production output stream.

Real Sway, Wayland, Fcitx5, application event-loop integration and interactive terminal acceptance remain NOT RUN. There is still no usable GUI terminal.

# Runtime PTY resize safety review — TASK-024 (2026-10-29)

No dependencies or lockfile entries change. The existing TIOCSWINSZ operation is
shared by initial slave sizing and runtime master sizing in `set_pty_size`.
Its unsafe call receives a BorrowedFd from the adapter's live OwnedFd/File and
a fully initialized stack `libc::winsize`. Row/column values are nonzero u16;
pixel fields remain zero. The ioctl reads the structure during the call and does
not retain its pointer or acquire FD ownership. No arbitrary integer FD, global
terminal, or user-selected external process is targeted.

Runtime resize makes at most one syscall per changed valid request. Invalid and
unchanged sizes do not call the adapter; an explicitly terminated session reports
NotConnected before duplicate-size handling. The cache starts from the successful
spawn size and updates only after successful ioctl. Errors, including EINTR,
return metadata and preserve the prior cache so the caller may retry later.
The cache is the last size applied by Ptyra, not a live query of changes made by
a child or direct FD user.

Linux sends SIGWINCH to the affected PTY's foreground process group when its size
changes ([Linux man-pages](https://man7.org/linux/man-pages/man2/TIOCSWINSZ.2const.html)).
The adapter does not separately signal a PID/process group. The fixed integration
fixture installs a WINCH trap before sending its readiness byte; the trap invokes
`stty size` and exits. Two owned sessions verify changed dimensions and signal
isolation with deadlines, bounded output, FD accounting and normal child reaping.
This is Linux PTY verification, not GUI resize, Wayland or Fcitx5 acceptance.


# Dependency review — TASK-026 (2026-10-29)

新增 `alacritty_terminal = 0.26.0`，停用預設 serde feature。用途係重用
`Term` grid 同 re-export 嘅 `vte::ansi::Processor`，唔自行重寫 ANSI parser。
Ptyra 只呼叫 core API，唔建立 upstream PTY、event loop 或 thread。
冇新增 GTK、Qt、X11 或其他 GUI runtime；upstream 嘅 Linux/native dependencies
仍會納入 build，唔代表零 native dependencies。

## 版本、維護同授權

官方 Cargo registry index 記錄 `alacritty_terminal 0.26.0` 於
2026-04-06 發佈，最低 Rust 1.85，Apache-2.0；`vte 0.15.0` 於
2025-02-02 發佈，最低 Rust 1.62.1，Apache-2.0 OR MIT。兩者均未 yanked。
專案既有 KBVM 要求 Rust 1.95；本次使用 Rust 1.97.1。

[Alacritty upstream](https://github.com/alacritty/alacritty) 未 archived，
查核時最後 push 為 2026-10-26；[VTE upstream](https://github.com/alacritty/vte)
未 archived，最後 push 為 2026-10-25。呢啲係維護活動證據，唔係安全保證。
API 同 feature 依據 [0.26.0 manifest](https://docs.rs/crate/alacritty_terminal/0.26.0/source/Cargo.toml)
及已下載、核對 checksum 嘅 source。

Cargo.lock 新增 42 個 entries，包括非 Linux target 同 lockfile-only entries。
所有新 entries 嘅 archive SHA-256、registry index checksum 同 lockfile checksum
一致，亦未 yanked。原有 package 版本同 checksum 全部保留。
以下 license 來自各 package manifest；並非對所有 transitive source 嘅完整審計。

| Package | Version | License |
|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| alacritty_terminal | 0.26.0 | Apache-2.0 |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| errno | 0.3.14 | MIT OR Apache-2.0 |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| futures-io | 0.3.34 | MIT OR Apache-2.0 |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 |
| home | 0.5.12 | MIT OR Apache-2.0 |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| miow | 0.6.1 | MIT OR Apache-2.0 |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| piper | 0.2.5 | MIT OR Apache-2.0 |
| polling | 3.11.0 | Apache-2.0 OR MIT |
| redox_syscall | 0.5.18 | MIT |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustix-openpty | 0.2.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| signal-hook | 0.4.5 | MIT OR Apache-2.0 |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 |
| vte | 0.15.0 | Apache-2.0 OR MIT |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |

## Advisory 查核

重新取得完整、未截斷嘅官方 [RustSec tree 40f5be6](https://github.com/RustSec/advisory-db/tree/40f5be6fb12ed93fc591bf4ab23b7d793b3f2239)，
比對全部 locked package names。新增套件匹配到以下 records：

| Package | Advisory | Patched | Locked |
|---|---|---|---|
| base64 | [RUSTSEC-2017-0004](https://rustsec.org/advisories/RUSTSEC-2017-0004.html) | >=0.5.2 | 0.22.1 |
| crossbeam-utils | [RUSTSEC-2022-0041](https://rustsec.org/advisories/RUSTSEC-2022-0041.html) | >=0.8.7 | 0.8.23 |
| lock_api | [RUSTSEC-2020-0070](https://rustsec.org/advisories/RUSTSEC-2020-0070.html) | >=0.4.2 | 0.4.14 |
| miow | [RUSTSEC-2020-0080](https://rustsec.org/advisories/RUSTSEC-2020-0080.html) | ^0.2.2 或 >=0.3.6 | 0.6.1 |

其餘 matches 係前述已審核、版本未變嘅 hashbrown、nix 同 smallvec。
冇匹配 alacritty_terminal 或 vte 嘅 advisory。呢次手動查核未發現受影響版本；
冇執行 cargo-audit，亦唔保證冇未知漏洞。

## Untrusted input 同資源邊界

Source review 發現 VTE `std` feature 會用可增長 Vec 保存未終止 OSC；
`Term::input` 亦會將 zero-width characters 加入 cell Vec。使用者另行明確批准
T026 加入最小防護，並維持 T099 未完成。

- 每次 feed 最多 16 KiB。錯 TabId／超大 call 會在修改 parser 前拒絕。
- 每個 screen 最多 32,768 cells，至少一 row、兩 columns。
  T026 完成時 history 設為 0；T029 嘅 bounded policy 見下文。
- OSC 最多 4,096 bytes，跨 calls 累積。計入 separators 同被 core 忽略嘅 controls，
  以保守方式限制長度。Guard 只追蹤 ESC／OSC 邊界；真正 ANSI 解讀由 VTE 負責。
  BEL、CAN、SUB 或 ESC 結束當前 OSC，匹配 pinned VTE source。
- 每個 cell 最多保留 64 個 zero-width characters。逐 parser step 檢查 core
  目前可能附加 combining marks 嘅 cell，涵蓋 wide spacer 同 pending wrap。
  VTE REP 一次最多輸入 65,535 個字元，因此偵測前可短暫多出至多呢個數量嘅
  marks；唔會累積至下一個 step。呢個 cap 係保留狀態上限，唔係零暫存 allocation。
- OSC／combining 超額會釋放該 adapter 嘅 core，回傳 InvalidData；後續 feed
  回傳 NotConnected。唔會靜默截斷後繼續解析，唔會將 bytes 轉送其他 Tab。
  同一 call 嘅 prefix 可能已解析；caller 必須停止使用該 stream，唔可以重試。
  呢個 library API 本身唔關閉 PTY、唔殺 child。
- Core 內建 title stack 上限為 4,096 entries。每個 title／hyperlink 嘅來源
  已受 OSC cap 限制；兩個 grids 同 cell metadata 亦受以上固定尺寸限制。
- OSC 52 明確設 Disabled，Kitty keyboard 明確設 false。`VoidListener`
  丟棄 Clipboard、title、query response 等外部 events；呢次唔提供 PTY reply 接線。
- Sync update sequences 即時套用，唔建立 wall-clock timeout 或等待 buffer。
  VTE 仍預先配置約 2 MiB sync buffer；未宣稱支援 synchronized rendering。
- 新 adapter 冇 unsafe、FFI、thread、network、payload log 或系統設定改動。
  Upstream 仍有 native／unsafe code，同可能包含 payload 嘅 debug/trace statements。
  目前程式冇安裝 logger；將來啟用 logger 前必須過濾呢啲 dependency logs。

Limits 係本次固定 guardrails，唔係已完成 config、performance budget 或 fuzz task。
Feed byte cap 唔係 CPU deadline：ANSI REP 等 sequence 會放大 parser 工作量。
未做 performance benchmark、完整 source audit 或 fuzz campaign。

## Scrollback allocation review — TASK-029 (2026-10-29)

重用現有 `alacritty_terminal 0.26.0`，冇新增 dependency 或 app unsafe。
`Config::scrolling_history` 設定 main grid 嘅 history 上限；alternate grid 維持零 history。
每個 `Terminal` 擁有獨立 core，唔另存文字或建立第二份 history buffer。

核對 pinned source 嘅 `grid::Storage::initialize`：history 按需配置，增長批次至少
1,000 rows，故 allocation 可以多過可讀嘅 history 行數。固定設定下（包括 reset），
spare rows 最多 1,000；本次將呢個上限計入預算。升級 core 時需要重新核對此常數。
無 history 時唔會進入呢條增長路徑。

Constructor 先驗證非零 rows、至少兩 columns、每 screen 最多 32,768 cells，
同 0–10,000 history lines，再檢查以下 cell slots 合計唔超過 2,000,000：

`2 × rows × columns + (history_lines + spare_rows) × columns`

兩個 screens 都計入；history 非零時 `spare_rows = 1,000`，否則為零。
行數限制在加法／乘法前檢查，`usize::MAX` 等設定直接回傳 InvalidInput。
已驗證嘅 rows／columns／history 範圍令計算在 32-bit 同 64-bit usize 都唔會 overflow。
超額設定唔會靜默縮減，亦唔會先配置 core 再報錯。

Core ring buffer 負責淘汰最舊可讀行，並重用已配置空間。Cached rows 可以暫時保留
已淘汰行嘅 metadata，仍然包含喺上述 spare-row 預算內；唔承諾每淘汰一行就即時
釋放 allocator memory。Cell slots cap 唔係 RSS／byte limit；row metadata、Vec capacity、
combining marks、hyperlinks、title stack 同 parser storage 仍有各自開銷及既有上限。
本次冇量度 PSS／RSS，亦未驗收整體 RAM budget。
