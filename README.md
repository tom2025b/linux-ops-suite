# Linux Ops Suite

A personal toolkit of focused, single-purpose Linux tools that work together through clean file-based contracts.

This repository is the **contract and index headquarters** for the suite. Each tool lives in its own repo. This repo defines how they should talk to each other.

## The Tools

| Tool            | Role                                              | Status       |
|-----------------|---------------------------------------------------|--------------|
| **Bulwark**     | Read-only scanner + risk classifier               | Active       |
| **ScriptVault** | Fast TUI script launcher + favorites & recents    | Active       |
| **Toolbox Bridge** | Converts Bulwark risk data into ScriptVault sidecars | Active    |
| **ToolFoundry** | Tool lifecycle, ownership, and health             | Active       |
| **Workstate**   | Read-only state compiler — emits the v3 snapshot  | Active       |
| **Proto**       | Guided protocol / checklist runner — emits session records | Active       |
| **RexOps**      | Operations cockpit + suite launcher (`rex run` for full refresh) | Active |

## Installation

The whole suite installs with **one command**. Because each tool lives in its own
repo, [`install.sh`](install.sh) is an *orchestrator*: it clones (or updates) every
tool repo, builds it, and puts the binaries on your `PATH` — then installs the
`rex` launcher and the per-tool `r-<tool>` wrappers.

```bash
# Fresh machine: clone this repo, then run the installer.
git clone https://github.com/tom2025b/linux-ops-suite.git
cd linux-ops-suite
./install.sh
```

That's it. Re-run it any time to update — it's **idempotent** (skips what's already
built unless you pass `--force`).

### What it does

For each tool it: **clone or `git pull`** the repo → **`cargo build --release`** →
**copy `target/release/<binary>` into `~/.local/bin/`**. Specifically:

- **Rust tools** (Bulwark, ScriptVault, ToolFoundry, Workstate, Proto, RexOps) are
  built from source with `cargo build --release` and the resulting binary is copied
  to `~/.local/bin/`.
- **Toolbox Bridge** (Python) is installed via `pipx`, or a self-contained virtualenv
  with a launcher on your `PATH` if `pipx` isn't available.
- The **`rex`** launcher is installed to `~/.local/bin/rex`.
- A `r-<tool>` wrapper is written to `~/bin/` and an alias appended to
  `~/.rust_aliases.sh` for every tool.

The installer **never edits your shell config**. If `~/.local/bin` or `~/bin` isn't
on your `PATH`, it prints the exact line to add.

### Prerequisites

- **`git`** and a **Rust toolchain** (`cargo`) — required. Install Rust via
  [rustup](https://rustup.rs): `curl https://sh.rustup.rs -sSf | sh`.
- **`pipx`** or **`python3`** — optional, only for Toolbox Bridge (the Rust tools
  install fine without it).

### Options

```bash
./install.sh --dry-run        # show exactly what would happen; change nothing
./install.sh --force          # rebuild/reinstall even if already present
./install.sh --local          # use existing local clones; never clone/pull
./install.sh --only a,b       # operate on just these tools (comma-separated)
./install.sh --skip-aliases   # don't write r-<tool> wrappers or aliases
./install.sh --help
```

### After installing

If the installer reported that a directory isn't on your `PATH`, add it to your
shell rc (`~/.bashrc` or `~/.zshrc`) and source the aliases once:

```bash
export PATH="$HOME/.local/bin:$HOME/bin:$PATH"
[ -f "$HOME/.rust_aliases.sh" ] && source "$HOME/.rust_aliases.sh"
```

Then kick off a full suite refresh:

```bash
rex run
```

## How They Work Together

- Data flows **one way** through files (mostly JSON).
- No tool imports code from another tool.
- **RexOps** is the front door and only consumer — it reads summaries and lets you launch the other tools.
- **ToolFoundry** emits `toolfoundry workstate-feed`; the shape is pinned by `contracts/toolfoundry.workstate-feed.v1.schema.json`.
- **Workstate** compiles the other tools' feeds into one versioned `snapshot.json` (schema v3) that **RexOps** consumes as its source of truth. The shape is pinned by `contracts/workstate.snapshot.schema.json` and validated in both repos' CI.
- Also live: **Bulwark → Toolbox Bridge → ScriptVault** for risk sidecars.
- **Proto** reads human-authored protocols (YAML checklists) and emits one `session` JSON per run, pinned by `contracts/proto.session.schema.json`. It is read-only — it guides and records, it never acts on your behalf.

## Running a full suite refresh

```bash
rex run
```

- No arguments needed.
- Automatically detects the current project folder (git toplevel, falling back to pwd) and passes it to tools that scan or read manifests.
- First thing you see is the celebration banner ("Rex and Baby Grok built this. Enjoy.") with a cute detailed ASCII baby and fireworks.
- Then runs the producers and aggregator in the correct order: ToolFoundry → Bulwark → Proto → ScriptVault → Workstate.
- Everything is optional and best-effort; missing tools are skipped (graceful degradation).
- A small status summary is printed from the resulting Workstate v3 snapshot when present.

`bin/rex` is the reference implementation (bash). The real RexOps TUI (in its own repo) will eventually provide the interactive cockpit and launcher on top of the same contracts.

## Design Principles

- One job per tool
- File-based contracts over shared code — for logic and data. The lone exception is
  `suite-ui` (shared TUI *chrome* only); see below.
- Read-only by default
- Low-resource friendly (Linux Mint)
- Rust-first where it makes sense

## Repositories

- [Bulwark](https://github.com/tom2025b/bulwark) — Scanner & risk
- [ScriptVault](https://github.com/tom2025b/scriptvault) — Script launcher
- [Toolbox Bridge](https://github.com/tom2025b/toolbox-bridge) — Bulwark → ScriptVault connector
- [ToolFoundry](https://github.com/tom2025b/toolfoundry) — Lifecycle & ownership
- [Workstate](https://github.com/tom2025b/workstate) — State compiler
- [Proto](https://github.com/tom2025b/proto) — Guided protocol / checklist runner
- [RexOps](https://github.com/tom2025b/rexops) — Suite cockpit

## Shared UI (`suite-ui`)

The one piece of shared *code* in the suite lives in this repo:
[`crates/suite-ui`](crates/suite-ui) — the common TUI chrome (cyan/amber theme with
`NO_COLOR` support, rounded panes, health styles, and the help / confirm / toast /
command-palette overlays). It is **pure presentation** — no domain logic, no data flow
— so it doesn't reintroduce the coupling the file-contracts rule prevents. RexOps and
ScriptVault are the intended consumers. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#shared-ui-chrome-suite-ui) for the why.

```bash
# build + test the crate, and see every component rendered in each theme:
cargo test -p suite-ui
cargo run -p suite-ui --example gallery
```

---

Built for personal use. Keep it simple.
