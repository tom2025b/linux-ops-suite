# Linux Ops Suite

A personal toolkit of focused, single-purpose Linux tools that work together through clean file-based contracts.

This repository is the **contract and index headquarters** for the suite. Each tool lives in its own repo. This repo defines how they should talk to each other.

## The Tools

| Tool | Role | Status |
|------|------|--------|
| **[Bulwark](https://github.com/tom2025b/bulwark)** | Read-only scanner + risk classifier | Active |
| **[ScriptVault](https://github.com/tom2025b/scriptvault)** | Fast TUI script launcher + favorites & recents | Active |
| **[Toolbox-Bridge](https://github.com/tom2025b/linux-ops-suite)** | Bridges Bulwark findings into ScriptVault sidecar metadata, via Workstate | Active |
| **[ToolFoundry](crates/toolfoundry)** | Tool lifecycle, ownership, and health | Active |
| **[Workstate](https://github.com/tom2025b/workstate)** | Read-only state compiler — compiles the one canonical snapshot (shape/version/path defined by `workstate-schema`) | Active |
| **[Proto](crates/proto)** | Guided protocol / checklist runner — emits session records | Active |
| **[RexOps](https://github.com/tom2025b/rexops)** | Operations cockpit + suite launcher | Active |
| **[rex-doctor](crates/rex-doctor)** | Suite diagnostics — checks env/PATH, binaries & versions | Active |
| **[portman](crates/portman)** | Lists listening sockets + ownership chain, with baseline diff | Active |
| **[pulse](crates/pulse)** | Calm read-only TUI showing one suite-health verdict | Active |
| **[tripwire](crates/tripwire)** | File-integrity baseline + drift diff (SHA-256 + metadata) | Active |
| **[rewind](crates/rewind)** | Suite history + safe rollback — content-addressed captures of suite state, with guarded restore | Active |
| **[conductor](crates/conductor)** | Guided operator — reads the canonical snapshot and walks you through an ordered runbook | Active |
| **[rex-forge](crates/rex-forge)** | TUI-first project scaffolder for Rust and Go | Active |

### Capability matrix

How each tool relates to the suite's data flow — who you invoke directly, who
emits a `workstate-feed` for Workstate to compile, and who reads the resulting
snapshot:

| Tool | User runs it? | Produces feed? | Reads snapshot? | Description |
|------|:---:|:---:|:---:|------|
| **Bulwark** | ✓ | ✓ | – | Read-only scanner + risk classifier |
| **ScriptVault** | ✓ | ✓ | – | Fast TUI script launcher (favorites & recents) |
| **Toolbox-Bridge** | – | ✓ | ✓ | Bridges Bulwark findings → ScriptVault sidecar metadata |
| **ToolFoundry** | ✓ | ✓ | – | Tool lifecycle, ownership, and health |
| **Workstate** | ✓ | compiles | writes | Compiles the one canonical snapshot |
| **Proto** | ✓ | ✓ | – | Guided protocol / checklist runner |
| **RexOps** | ✓ | – | ✓ | Operations cockpit + suite launcher |
| **rex-doctor** | ✓ | – | – | Suite diagnostics (env/PATH, binaries, versions) |
| **portman** | ✓ | – | – | Listening sockets + ownership chain, baseline diff |
| **pulse** | ✓ | – | ✓ | Calm one-verdict suite-health TUI |
| **tripwire** | ✓ | – | – | File-integrity baseline + drift diff |
| **rewind** | ✓ | – | – | Suite history + guarded rollback |
| **conductor** | ✓ | – | ✓ | Guided operator — ordered runbook from the snapshot |
| **rex-forge** | ✓ | – | – | TUI-first project scaffolder (Rust/Go) |

Legend: ✓ yes · – no. "Produces feed?" means it emits a `workstate-feed` that
Workstate ingests; Workstate itself compiles the snapshot, and the consumers read
it back through `workstate-schema`.

## Installation

There are two install paths:

- [`crates/linux-ops-install`](crates/linux-ops-install) downloads **prebuilt GitHub Release assets** for the suite tools, installs them to `~/.local/bin`, writes `~/bin/r-<tool>` wrappers, and appends aliases to `~/.rust_aliases.sh`.
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
- Extracts the archive, installs the binary into `~/.local/bin`, writes `~/bin/r-<tool>`, and updates `~/.rust_aliases.sh`.
- Never edits your shell rc files. If `~/.local/bin` or `~/bin` is missing from `PATH`, it prints the exact line to add.

Integrity flags:

- `--allow-unverified` — downgrade a *missing* checksum from a hard failure to a loud warning and install anyway. (A checksum *mismatch* still fails regardless.)
- `--no-verify` — skip verification entirely (unsafe; local/offline testing only).

Supported binaries:

From standalone tool repos (each publishes its own GitHub Release):

- `bulwark`
- `scriptvault`
- `workstate`
- `rexops`

From this umbrella repo (all shipped together in the `linux-ops-suite` release archive):

- `toolbox-bridge`
- `rex-doctor`
- `portman`
- `pulse`
- `tripwire`
- `rewind`
- `conductor`
- `rex-forge`
- `proto`
- `toolfoundry`

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

The `linux-ops-suite` release is the special case: a single `linux-ops-suite-<target>` archive carries **all** the in-workspace tools — `toolbox-bridge`, `rex-doctor`, `portman`, `pulse`, `tripwire`, `rewind`, `conductor`, and `rex-forge` — and the installer extracts each binary by name.

### Cutting releases

This repo has a release workflow: [.github/workflows/release.yml](.github/workflows/release.yml) builds, packages, checksums, and publishes the in-workspace tool binaries on every `v*` tag — `v0.3.0` was published this way (continuous integration lives in [.github/workflows/ci.yml](.github/workflows/ci.yml)). So cutting this repo's release is just a tag:

```bash
git tag v0.3.1
git push origin v0.3.1   # release.yml builds the x86_64 + aarch64 archives and uploads them
```

The standalone tool repos (`bulwark`, `scriptvault`, `workstate`, `rexops`) each publish their own release. For one of them:

1. Build the release binary in that repo.
2. Package the executable into a Linux archive, preferably `.tar.gz`.
3. Create a GitHub Release and upload the archive.

Example for a tool whose repo and binary are both `bulwark`:

```bash
cargo build --release
mkdir -p dist
asset="bulwark-x86_64-unknown-linux-gnu.tar.gz"
tar -C target/release -czf "dist/$asset" bulwark
( cd dist && sha256sum "$asset" > "$asset.sha256" )   # the installer verifies this, fail-closed
gh release create vX.Y.Z dist/"$asset" dist/"$asset.sha256" \
  --repo tom2025b/bulwark --title "vX.Y.Z" --notes "Linux release"
```

To package this repo's in-workspace tools by hand (the `release.yml` workflow does exactly this on a tag):

```bash
cargo build --release -p toolbox-bridge -p rex-doctor -p portman -p pulse \
  -p tripwire -p rewind -p conductor -p rex-forge
mkdir -p dist
asset="linux-ops-suite-x86_64-unknown-linux-gnu.tar.gz"
tar -C target/release -czf "dist/$asset" \
  toolbox-bridge rex-doctor portman pulse tripwire rewind conductor rex-forge
( cd dist && sha256sum "$asset" > "$asset.sha256" )
gh release create vX.Y.Z dist/"$asset" dist/"$asset.sha256" \
  --repo tom2025b/linux-ops-suite --title "vX.Y.Z" --notes "in-workspace tools"
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
- writes `~/bin/r-<tool>` wrappers and aliases

### After either install path

If the installer reported that a directory is missing from `PATH`, add this to your shell rc:

```bash
export PATH="$HOME/.local/bin:$HOME/bin:$PATH"
[ -f "$HOME/.rust_aliases.sh" ] && source "$HOME/.rust_aliases.sh"
```

Then refresh the suite — see [Running a full suite refresh](#running-a-full-suite-refresh) below.

## How They Work Together

- Data flows **one way** through files (mostly JSON). No tool imports another tool's domain code.
- **Producers** emit versioned `workstate-feed` JSON: **ToolFoundry** (`toolfoundry.workstate-feed.v1`), **Bulwark** (`bulwark.workstate-feed.v1`), and **Proto** (`proto.workstate-feed.v1`), each pinned by a schema under `contracts/`.
- **Workstate** compiles those feeds into **one canonical snapshot** (`workstate.snapshot.json`). Its shape, schema version, and on-disk path are defined exactly once — in the **`workstate-schema`** crate, the suite's single source of truth for the snapshot. Every consumer reads the snapshot *through* that crate and nothing else, so the producer and its consumers can never drift. (The JSON shape is mirrored by `contracts/workstate.snapshot.schema.json` and validated in CI.)
- **Consumers** all read that one snapshot through `workstate-schema`: **RexOps** (the cockpit + launcher), **Conductor** (a *pure consumer* — it reads the snapshot, derives an ordered runbook, and walks you through it, writing nothing itself), and **Pulse** (a single calm health verdict).
- **Toolbox-Bridge** is both a consumer and a producer: it reads Bulwark's findings *from the snapshot* (never from Bulwark directly) and writes a versioned sidecar feed (`toolbox-bridge.workstate-feed.v1`) into Workstate's feeds directory for **ScriptVault** to consume.
- **Proto** reads human-authored protocols (YAML checklists) and emits one `session` JSON per run (`proto.session.schema.json`). It is read-only — it guides and records, it never acts on your behalf.

## Running a full suite refresh

The old all-in-one `rex` launcher has been retired. A refresh is now explicit and
one-way — producers write feeds, Workstate compiles the one snapshot, a consumer
reads it:

1. **Refresh the producer feeds** you have. Each writes a `workstate-feed` into
   `$XDG_DATA_HOME/workstate/feeds/` — e.g. `bulwark workstate-feed`,
   `toolfoundry workstate-feed <manifest-dir>`, `proto feed`. See
   [docs/INTEGRATION_MAP.md](docs/INTEGRATION_MAP.md) for the exact commands and paths.
2. **Compile the canonical snapshot.** Workstate fans the feeds in and writes one
   snapshot to `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json` — the canonical
   path defined by `workstate-schema`:

   ```bash
   workstate
   ```

3. **Read it with any consumer.** Each reads only that snapshot, through
   `workstate-schema`:

   ```bash
   conductor      # a guided, ordered runbook over the snapshot
   pulse          # one calm suite-health verdict
   rexops         # the cockpit + launcher
   ```

Every step is optional and best-effort: a missing producer just leaves its section
of the snapshot empty, and consumers degrade gracefully rather than failing.

## Design Principles

- One job per tool
- File-based contracts over shared code — for logic and data. Two sanctioned
  exceptions: the shared TUI *chrome* (`thomas-tui` + `suite-ui`; see below) and the
  snapshot *contract* crate (`workstate-schema`) — both are pure data/presentation
  with no domain behaviour, so neither reintroduces cross-tool logic coupling.
- Read-only by default
- Low-resource friendly (Linux Mint)
- Rust-first where it makes sense

## Repositories

- [Bulwark](https://github.com/tom2025b/bulwark) — Scanner & risk
- [ScriptVault](https://github.com/tom2025b/scriptvault) — Script launcher
- Toolbox-Bridge — lives in this repo: [`crates/toolbox-bridge`](crates/toolbox-bridge) (Bulwark → Workstate → ScriptVault adapter)
- ToolFoundry — lives in this repo: [`crates/toolfoundry`](crates/toolfoundry) — Lifecycle & ownership
- [Workstate](https://github.com/tom2025b/workstate) — State compiler
- Proto — lives in this repo: [`crates/proto`](crates/proto) — Guided protocol / checklist runner
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
