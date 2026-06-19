# Linux Ops Suite

A personal toolkit of focused, single-purpose Linux tools that work together through clean file-based contracts.

This repository is the **contract and index headquarters** for the suite. Each tool lives in its own repo. This repo defines how they should talk to each other.

## The Tools

| Tool | Role | Status |
|------|------|--------|
| **[Bulwark](https://github.com/tom2025b/bulwark)** | Read-only scanner + risk classifier | Active |
| **[ScriptVault](https://github.com/tom2025b/scriptvault)** | Fast TUI script launcher + favorites & recents | Active |
| **[Toolbox-Bridge](https://github.com/tom2025b/linux-ops-suite)** | Bridges Bulwark findings into ScriptVault sidecar metadata, via Workstate | Active |
| **[ToolFoundry](https://github.com/tom2025b/toolfoundry)** | Tool lifecycle, ownership, and health | Active |
| **[Workstate](https://github.com/tom2025b/workstate)** | Read-only state compiler — emits the v4 snapshot | Active |
| **[Proto](https://github.com/tom2025b/proto)** | Guided protocol / checklist runner — emits session records | Active |
| **[RexOps](https://github.com/tom2025b/rexops)** | Operations cockpit + suite launcher | Active |
| **[rex-doctor](crates/rex-doctor)** | Suite diagnostics — checks env/PATH, binaries & versions | Active |
| **[portman](crates/portman)** | Lists listening sockets + ownership chain, with baseline diff | Active |
| **[pulse](crates/pulse)** | Calm read-only TUI showing one suite-health verdict | Active |
| **[tripwire](crates/tripwire)** | File-integrity baseline + drift diff (SHA-256 + metadata) | Active |

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
- **Verifies the download against its published SHA256 checksum before extracting or installing it.** It looks for a sibling `<asset>.sha256` file (or a `SHA256SUMS` manifest) in the release and refuses to install on mismatch — or when no checksum is published at all (every suite release publishes one, so a missing checksum means a broken or tampered release). Requires `sha256sum` (coreutils).
- Extracts the archive, installs the binary into `~/.local/bin`, installs `rex`, writes `~/bin/r-<tool>`, and updates `~/.rust_aliases.sh`.
- Never edits your shell rc files. If `~/.local/bin` or `~/bin` is missing from `PATH`, it prints the exact line to add.

Integrity flags:

- `--allow-unverified` — downgrade a *missing* checksum from a hard failure to a loud warning and install anyway. (A checksum *mismatch* still fails regardless.)
- `--no-verify` — skip verification entirely (unsafe; local/offline testing only).

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

### Create the first releases

There is no release workflow in this repo yet; only CI is present in [.github/workflows/ci.yml](.github/workflows/ci.yml). The first release set is therefore a manual packaging step unless you add release automation.

For each standalone tool repo (`bulwark`, `scriptvault`, `toolfoundry`, `workstate`, `proto`, `rexops`):

1. Build the release binary in that repo.
2. Package the executable into a Linux archive, preferably `.tar.gz`.
3. Create a GitHub Release and upload the archive.

Example for a tool whose repo and binary are both `bulwark`:

```bash
cargo build --release
mkdir -p dist
tar -C target/release -czf dist/bulwark-x86_64-unknown-linux-gnu.tar.gz bulwark
gh release create v0.1.0 \
  dist/bulwark-x86_64-unknown-linux-gnu.tar.gz \
  --repo tom2025b/bulwark \
  --title "v0.1.0" \
  --notes "First Linux release"
```

For `toolbox-bridge`, build from this repo:

```bash
cargo build --release -p toolbox-bridge
mkdir -p dist
tar -C target/release -czf dist/linux-ops-suite-x86_64-unknown-linux-gnu.tar.gz toolbox-bridge
gh release create v0.1.0 \
  dist/linux-ops-suite-x86_64-unknown-linux-gnu.tar.gz \
  --repo tom2025b/linux-ops-suite \
  --title "v0.1.0" \
  --notes "First toolbox-bridge release"
```

Repeat with an `aarch64` build if you want ARM Linux installs to work without falling back to source builds.

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
- **Workstate** compiles the other tools' feeds into one versioned `snapshot.json` (schema v4) that **RexOps** consumes as its source of truth. The shape is pinned by `contracts/workstate.snapshot.schema.json` and validated in both repos' CI.
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
- A small status summary is printed from the resulting Workstate v4 snapshot when present.

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

---

Built for personal use. Keep it simple.
