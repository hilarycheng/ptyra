# Dependency review — TASK-012 (2026-10-29)

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
