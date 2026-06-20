# suite-core — shared non-UI foundation for the Linux Ops Suite

Date: 2026-06-20
Status: approved (Core-set scope; PATH exec-bit fix; staged delivery)

## Problem

The workspace has grown to 11 crates / ~28.7k lines. A full review flagged
the biggest issue as **duplicated helper code with no shared non-UI
foundation**. An audit of the 9 tool crates (conductor, tripwire, rewind,
portman, rex-check, rex-doctor, pulse, toolbox-bridge, linux-ops-install)
found the same small helpers copy-pasted — mostly byte-identical:

| helper | copies | status |
|---|---|---|
| `is_executable` / exec-bit | 6 | byte-identical |
| `stdout_is_tty` (isatty) | 7 | near-identical |
| `is_root` (geteuid) | 3 | byte-identical |
| `home_dir` ($HOME) | 4 | byte-identical |
| XDG `data_dir`/`config_dir` | 4 | identical base logic, only `<tool>` suffix differs |
| `expand_tilde` | 2 | byte-identical |
| `human_size` (bytes→string) | 3 | 2 identical, 1 trivial unit diff |
| `which` / resolve-on-PATH | 5 | **one diverges (latent bug)** |

The UI chrome is already shared (`thomas-tui`, `suite-ui`); **none** of the
tool crates depend on those. So `suite-core` is a clean new **non-UI**
library — the missing peer to the UI crates.

## Scope (Core set)

**IN**

- `env` : `stdout_is_tty`, `is_root`, `home_dir`
- `path`: `is_executable_file`, `resolve_on_path`, `which`
- `xdg` : `data_dir(tool)`, `config_dir(tool)`, `expand_tilde`
- `fmt` : `human_size`

**OUT** (deliberately left where they are)

- The 251-line hand-rolled `Sha256` (identical in rewind + tripwire).
  Integrity-critical; revisit as its own task.
- Each tool's domain-specific `error.rs` enum. Too tool-specific to share.
- All TUI chrome — already in `thomas-tui` / `suite-ui`.

## Design

New crate `crates/suite-core/`. **Zero third-party dependencies** (std +
two libc externs only — matches the suite's lean discipline). No features.

```
crates/suite-core/
  Cargo.toml          # all fields .workspace = true; no deps
  src/lib.rs          # module decls + crate doc
  src/env.rs          # stdout_is_tty, is_root, home_dir
  src/path.rs         # is_executable_file, resolve_on_path, which
  src/xdg.rs          # data_dir(tool), config_dir(tool), expand_tilde
  src/fmt.rs          # human_size
```

### API

```rust
// env.rs
pub fn stdout_is_tty() -> bool;          // isatty(1) == 1
pub fn is_root() -> bool;                // geteuid() == 0
pub fn home_dir() -> Option<PathBuf>;    // $HOME, empty rejected

// path.rs
pub fn is_executable_file(p: &Path) -> bool;        // is_file && mode & 0o111
pub fn resolve_on_path(name: &str) -> Option<PathBuf>;
pub fn which(name: &str) -> bool;                   // resolve_on_path(name).is_some()

// xdg.rs
pub fn data_dir(tool: &str) -> Option<PathBuf>;     // $XDG_DATA_HOME|~/.local/share / linux-ops-suite / tool
pub fn config_dir(tool: &str) -> Option<PathBuf>;   // $XDG_CONFIG_HOME|~/.config / linux-ops-suite / tool
pub fn expand_tilde(raw: &str) -> PathBuf;

// fmt.rs
pub fn human_size(bytes: u64) -> String;            // 1024-based, ["B","KB","MB","GB","TB"], "{v:.1} {unit}"
```

`resolve_on_path` adopts rex-doctor's most-complete variant: a name
containing `/` is treated as a literal path; otherwise scan `$PATH` and
return the first **executable** match. It **always** checks the exec bit.

### The PATH bug being fixed

`pulse/src/cockpit.rs::resolve_on_path` checks only `candidate.is_file()` —
no exec-bit check — so a non-executable file shadowing a binary name would
be "found". 4 of the 5 copies already check the bit. suite-core's single
impl always checks it, silently correcting pulse/cockpit. Called out in the
commit/LAST_WORK.

### XDG parametrization

Every caller passes a literal tool name today ("rewind", "tripwire",
"portman"). `data_dir(tool)` / `config_dir(tool)` keep one impl while
preserving each tool's distinct path. Each crate keeps a one-line wrapper
(`fn data_dir() { suite_core::xdg::data_dir("rewind") }`) so its internal
call sites are untouched.

### `human_size` units

suite-core uses `["B","KB","MB","GB","TB"]` (rewind/tripwire's exact form).
rex-check currently prints `B/K/M/G/T`; it will be standardized onto
suite-core's form (cosmetic; noted as a behavior change).

## Migration

Each tool keeps its `util.rs` as a **thin re-export shim** over suite-core,
so call sites across each crate don't change. Delete the duplicated bodies
and their now-redundant unit tests (the canonical tests live in suite-core).
Net per crate: large deletions, tiny additions, identical public behavior.

Order (one small commit each):
1. rewind  2. tripwire  3. portman  4. rex-doctor  5. rex-check
6. conductor  7. pulse (the cockpit bug-fix site)

`toolbox-bridge` / `linux-ops-install` only have small exec-bit/path-dir
fragments; fold in only if a clean drop-in, else leave (installer is
release-critical) and note it.

## Wiring

- root `Cargo.toml`: add `crates/suite-core` to `members`; add
  `suite-core = { path = "crates/suite-core" }` to `[workspace.dependencies]`.
- each consumer `Cargo.toml`: `suite-core = { workspace = true }`.

## Verification

After each crate, and once at the end:

```
cargo build  --workspace
cargo test   --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt    --all --check
```

Then update `LAST_WORK.md` before declaring done.

## Delivery

Staged. Build suite-core (crate + wiring) and stop for review. Then migrate
one crate per checkpoint, pausing after each. All work in an isolated git
worktree. Nothing pushed / no PR / no merge without explicit approval.

## Non-goals / risks

- Not extracting Sha256, error enums, or JSON envelopes (this pass).
- No new dependencies.
- Behavior changes are limited to: (1) pulse/cockpit PATH exec-bit fix,
  (2) rex-check size-unit labels. Both intentional and documented.
