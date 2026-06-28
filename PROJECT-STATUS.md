# Linux Ops Suite — Project Status

A snapshot of the whole project: goal, architecture, where each piece stands, and
what's left.

_Last updated: 2026-06-27 — just after the repo-to-crate consolidation landed._

## Goal

A personal toolkit of **focused, single-purpose Linux tools** that compose into an
operations workflow — scan your tools, classify risk, track lifecycle/ownership,
launch scripts fast, run guided protocols, and roll it all up into one cockpit
(**RexOps**). Built for personal use on modest Linux hardware. Keep it simple.

## Architecture & philosophy

This repo is now a **single Cargo workspace (a monorepo)** holding every suite tool.
It used to be a thin "contract & index HQ" pointing at one-repo-per-tool; as of the
June 2026 consolidation, the six standalone tools (Proto, ToolFoundry, Bulwark,
RexOps, ScriptVault, Workstate) live in-tree under `crates/`. The design rules that
made the tools composable still hold — consolidation changed *where the code lives*,
not *how the tools talk to each other*:

- **One job per tool.** Each tool is its own crate (or small crate group) with a
  single responsibility. They are decoupled by contract, not by repo boundary.
- **File-based contracts over shared code.** Tools communicate one-way through JSON
  files whose shapes are pinned by JSON Schemas in `contracts/` (and validated in
  CI). No tool imports another tool's *domain* code — the only shared libraries are
  the deliberate, contract/presentation ones below.
- **Read-only by default.** Tools observe and report; they don't act on your behalf
  (Proto guides and records, it never executes).
- **Sanctioned shared crates (the only cross-tool `use`):**
  - `crates/workstate-schema` — the snapshot **contract** (model, `SCHEMA_VERSION`,
    canonical path, atomic write / validating load). The single source of truth for
    the snapshot; every consumer reads it through this crate and nothing else.
  - `crates/suite-ui` (+ `crates/thomas-tui`) — pure TUI **presentation** (panes,
    overlays, status/search bars; theme/guard/widgets underneath). No domain logic
    or data flow.
  - `crates/suite-core` — a dependency-free env/path/xdg/fmt foundation.

  Since consolidation these are consumed as ordinary **in-tree path dependencies**
  via workspace inheritance (`{ workspace = true }`) — they were cross-repo **git**
  dependencies pinned by rev before the tools moved in-tree.

### Data flow

```
ToolFoundry ─┐
Bulwark ─────┤  emit *.workstate-feed JSON
Proto ───────┘
                 │
            Workstate  ── compiles ──>  snapshot.json (schema v5)
                                             │
                          ┌──────────────────┼───────────────────┐
                       RexOps            Conductor              Pulse
                    (cockpit)        (guided runbook)     (health verdict)
                  — every consumer reads the one snapshot through workstate-schema —
                          (RexOps also reads Bulwark scan + ScriptVault export directly)

sidecar loop:  snapshot.json ──> Toolbox-Bridge ──> feeds/toolbox-bridge.json
               (risk/owner sidecar metadata for ScriptVault — via Workstate only)
```

A refresh is explicit and one-way — producers write feeds, `workstate` compiles the
one snapshot, and any consumer reads it. The all-in-one `rex` bash launcher was
retired earlier; RexOps is the interactive cockpit, now an in-tree crate group.

## Current state — the workspace

- **One workspace, 27 member crates.** Every tool builds from one `Cargo.toml` /
  `Cargo.lock`, shares one version (`[workspace.package] version = 0.3.1`), and is
  released together in one `linux-ops-suite-<target>` archive. **No git references
  to any sibling repo remain** in the manifests or the lockfile.
- **Dependency hygiene.** Third-party and shared in-tree crate versions are
  centralized in `[workspace.dependencies]`; members opt in with `{ workspace = true }`.
  Editions are mixed but explicit (umbrella root 2021; the four edition-2024 tools
  set it per-crate). MSRV is `rust-version = "1.85"`, and the toolchain is pinned
  via `rust-toolchain.toml`.
- **Contracts:** 9 JSON Schemas in `contracts/` (`bulwark.scan`, `proto.session`,
  `rexops.snapshot`, `scriptvault.export`, `workstate.snapshot`, plus the four
  `*.workstate-feed.v1` feeds). CI checks every JSON file is well-formed, every
  schema is a valid JSON Schema, and **every example validates against its schema**
  (`examples/` carries one payload per schema, 9 of 9).
- **CI gate (`.github/workflows/ci.yml`)** runs `cargo fmt --check`, `cargo clippy
  --workspace --all-targets --all-features -D warnings`, `cargo build`, and
  `cargo test --workspace --all-features`, plus the JSON-contract validation.
- **Installer:** two paths. (1) `install.sh` builds-and-copies from this workspace;
  its sibling-repo list is now **empty** (every tool is in-workspace). (2)
  `linux-ops-install` installs **prebuilt release binaries** — it now pulls every
  tool from this one repo's release, SHA256-verifying each asset fail-closed.
  Releases come from `.github/workflows/release.yml` (tag `v*` → x86_64 + aarch64
  `.tar.gz` + `.sha256`), which packages all tool binaries into the one archive.

## Current state — the tools

All six former-standalone tools are now in-tree crates on `main`, alongside the
crates that were always in the umbrella.

| Tool | Crates | ~LOC | Notes |
|---|---|---|---|
| **Proto** | `proto` | ~6.5k | Guided protocol/checklist runner; emits session records + a workstate feed. |
| **ToolFoundry** | `toolfoundry`, `toolfoundry-core` | ~4.7k | Tool lifecycle/ownership/health; emits a workstate feed. |
| **Bulwark** | `bulwark`, `bulwark-core` | ~6.5k | Read-only scanner + risk classifier (`tui` feature → suite-ui). |
| **RexOps** | `rexops-{core,adapters,app,cli,tui}` | ~15.6k | The cockpit + launcher. `rexops` binary launches the TUI; `rexops-tui` binary also builds but isn't shipped. |
| **ScriptVault** | `scriptvault`, `scriptvault-core` | ~14.6k | Fast TUI script launcher (favorites/recents); the dep-heaviest tool. |
| **Workstate** | `workstate`, `workstate-schema` | ~5.0k | State compiler (snapshot v5). `workstate-schema` is the shared contract. |

Always-in-umbrella crates:

| Crate | ~LOC | Notes |
|---|---|---|
| **thomas-tui** | ~3.2k | General TUI toolkit: guard, theme, text/layout/keys, widgets. |
| **suite-ui** | ~1.6k | Suite TUI chrome on thomas-tui. Consumed by Bulwark/RexOps/ScriptVault (now a path dep). |
| **suite-core** | — | Dependency-free env/path/xdg/fmt foundation. |
| **toolbox-bridge** | ~1.1k | Workstate snapshot → ScriptVault sidecar feed. Consumes `workstate-schema` + `workstate`. |
| **conductor** | — | Guided operator: reads the canonical snapshot, walks an ordered runbook (pure consumer). |
| **pulse / rewind / tripwire / portman / rex-doctor** | — | Health verdict / rollback captures / file-integrity drift / socket inventory / diagnostics. |
| **linux-ops-install** | ~1.5k | Release-binary installer: fetch latest release, SHA256-verify (fail-closed), install. |
| **rex-check / rex-forge** | — | Suite-health dashboard / TUI-first Rust+Go project scaffolder. |

## Where we are in development

- The suite is **well past prototype**: all tools exist, the contract layer is real
  and CI-validated, the end-to-end dataflow (feeds → Workstate snapshot → RexOps) is
  wired, and everything now lives and builds as one workspace.
- **Just landed: the repo-to-crate consolidation** — six tools pulled in-tree across
  six PRs (#57, #59, #60, #61, #63, #64), each green on the full CI gate before
  merge. `suite-ui` and the Workstate contract are now in-tree path deps; the
  installer/release/docs were updated to match.

## Major remaining work

1. **Archive the six superseded standalone repos** (`bulwark`, `scriptvault`,
   `toolfoundry`, `proto`, `rexops`, `workstate`) now that their code lives in-tree.
2. **Optional follow-ups from consolidation:** collapse the remaining edition
   differences if desired; revisit whether any of the now-inline exceptions
   (`bulwark-core`'s `serde_yaml_bw`) should converge.
3. **Continue ScriptVault's phased TUI redesign** and **RexOps cockpit** polish on
   top of the shared contracts + `suite-ui`.
4. **Keep contracts and example payloads in lockstep** as tools evolve; cut a
   tagged release when ready (the `[Unreleased]` CHANGELOG entry covers the
   consolidation).

---

*Point-in-time snapshot. For the consolidation detail see `CHANGELOG.md` and
`LAST_WORK.md`; for architecture and contracts see `docs/` (`ARCHITECTURE.md`,
`INTEGRATION_MAP.md`); for the installer see `install.sh` / `crates/linux-ops-install`.*
