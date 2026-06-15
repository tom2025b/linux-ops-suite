# Linux Ops Suite

A personal toolkit of focused, single-purpose Linux tools that work together through clean file-based contracts.

This repository is the **contract and index headquarters** for the suite. Each tool lives in its own repo. This repo defines how they should talk to each other.

## The Tools

| Tool | Role | Status |
|------|------|--------|
| **[Bulwark](https://github.com/tom2025b/bulwark)** | Read-only scanner + risk classifier | Active |
| **[ScriptVault](https://github.com/tom2025b/scriptvault)** | Fast TUI script launcher + favorites & recents | Active |
| **[Toolbox-Bridge](crates/toolbox-bridge)** | Bridges Bulwark findings into ScriptVault sidecar metadata, via Workstate (an in-repo crate, not a standalone repo) | Active |
| **[ToolFoundry](https://github.com/tom2025b/toolfoundry)** | Tool lifecycle, ownership, and health | Active |
| **[Workstate](https://github.com/tom2025b/workstate)** | Read-only state compiler — emits the v3 snapshot | Active |
| **[Proto](https://github.com/tom2025b/proto)** | Guided protocol / checklist runner — emits session records | Active |
| **[RexOps](https://github.com/tom2025b/rexops)** | Operations cockpit + suite launcher | Active |

## Installation

There are two install paths:

- [`crates/linux-ops-install`](crates/linux-ops-install) downloads **prebuilt GitHub Release assets** for the suite tools, installs them to `~/.local/bin`, installs `rex`, writes `~/bin/r-<tool>` wrappers, and appends aliases to `~/.rust_aliases.sh`.
- [`install.sh`](install.sh) is the **build-from-source fallback**. It clones or updates each repo, runs `cargo build --release`, and installs the resulting binaries locally.

### Use `linux-ops-install`

From this repo:

```bash
git clone https://github.com/tom2025b/linux-ops-suite.git
cd linux-ops-suite

# Preview exactly what would be downloaded and installed.
cargo run -p linux-ops-install -- --dry-run

# Install or reinstall from the latest GitHub Releases.
cargo run -p linux-ops-install -- --force
```

What `linux-ops-install` does:

- Detects the current Linux architecture: `x86_64` or `aarch64`.
- Queries `https://api.github.com/repos/tom2025b/<repo>/releases/latest` for each tool.
- Downloads the matching Linux asset, preferring `.tar.gz` when available.
- Extracts the archive, installs the binary into `~/.local/bin`, installs `rex`, writes `~/bin/r-<tool>`, and updates `~/.rust_aliases.sh`.
- Never edits your shell rc files. If `~/.local/bin` or `~/bin` is missing from `PATH`, it prints the exact line to add.

Supported binaries:

- `bulwark`
- `scriptvault`
- `toolfoundry`
- `workstate`
- `proto`
- `rexops`
- `toolbox-bridge`

If a repo has no GitHub Release yet, `linux-ops-install` now says that explicitly and prints:

- the repo's Releases page
- the direct `releases/new` URL
- the asset shape it expects
- the fallback: use [`install.sh`](install.sh) right now

### Release asset format expected by `linux-ops-install`

For each tool repo, publish at least one Linux release asset that matches the binary name and target architecture:

- Preferred archive: `.tar.gz`
- Also accepted: `.tgz`, `.tar.xz`, `.zip`
- Expected executable name inside the archive: exactly the tool binary name, for example `bulwark` or `proto`
- Expected naming hints in the asset filename: Linux plus `x86_64` or `amd64`, or `aarch64` or `arm64`

Examples of good asset names:

```text
bulwark-x86_64-unknown-linux-gnu.tar.gz
proto-aarch64-unknown-linux-gnu.tar.gz
linux-ops-suite-x86_64-unknown-linux-gnu.tar.gz
```

The `linux-ops-suite` release is the one special case: it is where `toolbox-bridge` is expected to come from.

### Publish releases with `scripts/release.sh`

Releases are produced by [`scripts/release.sh`](scripts/release.sh) — it builds
every suite binary, packages each one as a `.tar.gz` named for the target triple
(the exact shape `linux-ops-install` expects), and creates **or updates** the
matching GitHub Release via the `gh` CLI. There is no CI release workflow; this
script is the release path. (CI itself — [.github/workflows/ci.yml](.github/workflows/ci.yml) —
only builds/tests the workspace and validates the contract schemas + examples.)

```bash
# Build + publish v0.1.0 across the whole suite:
./scripts/release.sh v0.1.0

# Preview every command without changing anything:
DRY_RUN=1 ./scripts/release.sh v0.1.0
```

It releases the six sibling tools (`bulwark`, `scriptvault`, `toolfoundry`,
`workstate`, `proto`, `rexops`) plus `toolbox-bridge`, which is published as the
`linux-ops-suite` release (the special case `linux-ops-install` downloads it
from). It is built to be robust:

- existing releases are **updated** (assets re-uploaded), not treated as fatal;
- if the release commit isn't on the remote yet, the branch is **pushed
  automatically** and the release retried;
- a failure in one repo does not abort the others; a success/failure summary is
  printed at the end.

Useful environment overrides:

```bash
GH_OWNER=tom2025b      # GitHub owner/org (default: tom2025b)
SUITE_SRC_DIR=...      # parent dir holding the sibling repos
DRY_RUN=1              # print commands without changing anything
ALLOW_DIRTY=1          # skip the clean-worktree guard
SKIP_EXISTING=1        # skip (don't update) releases that already exist
NO_PUSH=1              # never auto-push a missing commit; fail that repo instead
```

Releases default to the host architecture's target triple; run the script on an
`aarch64` host (or set up cross-compilation) to also publish ARM Linux assets.

### Use `install.sh` today

Until every repo has a GitHub Release, use the source-build installer:

```bash
./install.sh
```

Useful options:

```bash
./install.sh --dry-run
./install.sh --force
./install.sh --local
./install.sh --only a,b
./install.sh --skip-aliases
./install.sh --help
```

What `install.sh` does:

- clones or updates each tool repo
- runs `cargo build --release`
- copies `target/release/<binary>` into `~/.local/bin`
- installs `rex`
- writes `~/bin/r-<tool>` wrappers and aliases

### After either install path

If the installer reported that a directory is missing from `PATH`, add this to your shell rc:

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
- **RexOps** is the front door and top-level consumer — it reads the rolled-up Workstate snapshot and lets you launch the other tools. (ScriptVault is a secondary consumer: it reads the Toolbox-Bridge sidecar feed — see below.)
- **ToolFoundry** emits `toolfoundry workstate-feed`; the shape is pinned by `contracts/toolfoundry.workstate-feed.v1.schema.json`.
- **Workstate** compiles the other tools' feeds into one versioned `snapshot.json` (schema v3) that **RexOps** consumes as its source of truth. The shape is pinned by `contracts/workstate.snapshot.schema.json` and validated in both repos' CI.
- **Toolbox-Bridge** turns Bulwark findings into ScriptVault sidecar metadata *via
  Workstate only*: it reads the compiled snapshot (never Bulwark directly) and writes
  a versioned sidecar feed into Workstate's feeds directory for ScriptVault to
  consume. The shape is pinned by `contracts/toolbox-bridge.workstate-feed.v1.schema.json`.
- **Proto** reads human-authored protocols (YAML checklists) and emits one `session` JSON per run, pinned by `contracts/proto.session.schema.json`. It is read-only — it guides and records, it never acts on your behalf.

## Running a full suite refresh

```bash
rex run
```

- No arguments needed.
- Automatically detects the current project folder (git toplevel, falling back to pwd) and passes it to tools that scan or read manifests.
- First thing you see is the celebration banner ("Rex and Baby Grok built this. Enjoy.") with a cute detailed ASCII baby and fireworks.
- Then runs the producers and aggregators in contract order: ToolFoundry → Bulwark → Proto → Workstate → Toolbox-Bridge.
- Producer feeds are written to `$XDG_DATA_HOME/workstate/feeds`, and the compiled Workstate snapshot is written to `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`.
- Everything is optional and best-effort; missing tools are skipped (graceful degradation).
- A small status summary is printed from the resulting Workstate v3 snapshot when present.

`bin/rex` is the reference implementation (bash). The real RexOps TUI (in its own repo) will eventually provide the interactive cockpit and launcher on top of the same contracts.

## Design Principles

- One job per tool
- File-based contracts over shared code — for logic and data. The lone exception is
  the shared TUI *chrome* (`thomas-tui` + `suite-ui`); see below.
- Read-only by default
- Low-resource friendly (Linux Mint)
- Rust-first where it makes sense

## Repositories

- [Bulwark](https://github.com/tom2025b/bulwark) — Scanner & risk
- [ScriptVault](https://github.com/tom2025b/scriptvault) — Script launcher
- Toolbox-Bridge — lives in this repo: [`crates/toolbox-bridge`](crates/toolbox-bridge) (Bulwark → Workstate → ScriptVault adapter)
- [ToolFoundry](https://github.com/tom2025b/toolfoundry) — Lifecycle & ownership
- [Workstate](https://github.com/tom2025b/workstate) — State compiler
- [Proto](https://github.com/tom2025b/proto) — Guided protocol / checklist runner
- [RexOps](https://github.com/tom2025b/rexops) — Suite cockpit

## Shared UI (`thomas-tui` + `suite-ui`)

The one piece of shared *code* in the suite lives in this repo, split across two
crates:

- [`crates/thomas-tui`](crates/thomas-tui) — a general-purpose, project-agnostic
  terminal-UI toolkit (the `NO_COLOR`-aware theme, a panic-safe terminal guard,
  centering/truncation helpers, shared keymap constants, and the domain-free widgets
  and overlays). No suite or domain vocabulary.
- [`crates/suite-ui`](crates/suite-ui) — the suite's common TUI chrome layered on
  `thomas-tui`: it re-exports the whole toolkit and adds the few widgets tied to the
  suite's risk/health/job vocabulary (severity badge, attention flag, health strip,
  status bar, toast).

Both are **pure presentation** — no domain logic, no data flow — so they don't
reintroduce the coupling the file-contracts rule prevents. Bulwark, RexOps, and
ScriptVault consume `suite-ui` as a **git dependency** pinned to a commit of this repo
(no `path =` deps), pulling in `thomas-tui` transitively, so each builds from a fresh
clone without a sibling checkout. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#shared-ui-chrome-suite-ui) for the why.

```bash
# build + test the crates, and see every component rendered in each theme:
cargo test -p thomas-tui -p suite-ui
cargo run -p suite-ui --example gallery
```

## In-workspace crates

This repo is a small Cargo workspace with five members. None of them are sibling
tools — they're the umbrella's own code (the shared UI chrome, the bridge, and
the suite-level CLIs):

| Crate | Role |
|---|---|
| [`thomas-tui`](crates/thomas-tui) | General-purpose terminal-UI toolkit (theme, terminal guard, widgets). |
| [`suite-ui`](crates/suite-ui) | Suite TUI chrome layered on `thomas-tui` (severity/health/job widgets). |
| [`toolbox-bridge`](crates/toolbox-bridge) | Bulwark → Workstate → ScriptVault sidecar adapter (pure Rust, dry-run-capable). |
| [`linux-ops-install`](crates/linux-ops-install) | The prebuilt-release installer (see [Installation](#installation)). |
| [`rex-check`](crates/rex-check) | At-a-glance health of the suite repos: per-repo git status (branch, dirty, ahead/behind) and source line counts, with a totals table. |

```bash
# fast repo-health summary across the whole suite:
cargo run -p rex-check
```

---

Built for personal use. Keep it simple.
