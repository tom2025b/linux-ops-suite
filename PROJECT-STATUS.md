# Linux Ops Suite — Project Status

A snapshot of the whole project: goal, architecture, where each piece stands, and
what's left.

## Goal

A personal toolkit of **focused, single-purpose Linux tools** that compose into an
operations workflow — scan your tools, classify risk, track lifecycle/ownership,
launch scripts fast, run guided protocols, and roll it all up into one cockpit
(`rex`). Built for personal use on modest Linux hardware. Keep it simple.

## Architecture & philosophy

This umbrella repo is the **contract and index HQ**, not a monorepo. The rules:

- **One job per tool.** Each tool is its own repo with its own lifecycle.
- **File-based contracts over shared code.** Tools communicate one-way through
  JSON files whose shapes are pinned by JSON Schemas in `contracts/` (and
  validated in CI). No tool imports another tool's code.
- **Read-only by default.** Tools observe and report; they don't act on your
  behalf (Proto guides and records, it never executes).
- **The one shared-code exception:** `crates/suite-ui` — pure TUI *chrome* (theme,
  panes, overlays), no domain logic or data flow, so it doesn't reintroduce
  coupling. RexOps and ScriptVault consume it.

### Data flow

```
ToolFoundry ─┐
Bulwark ─────┤  emit *.workstate-feed / scan JSON
Proto ───────┤
ScriptVault ─┘
                 │
            Workstate  ── compiles ──>  snapshot.json (schema v3)
                                             │
                                          RexOps  (cockpit / launcher; `rex run`)

side channel:  Bulwark ──> Toolbox Bridge ──> ScriptVault sidecars (risk tags)
```

`bin/rex` is the reference orchestrator (bash); the real RexOps TUI lives in its
own repo and will provide the interactive cockpit on the same contracts.

## Current state — umbrella

- **Contracts:** 8 schemas present (`bulwark.scan`, `*.workstate-feed.v1` for
  bulwark/proto/toolfoundry, `proto.session`, `rexops.snapshot`,
  `scriptvault.export`, `workstate.snapshot`). CI validates JSON contracts.
- **`suite-ui`:** active shared crate (theme + overlays + key-hints/search/status
  bars); the sole workspace member in the root `Cargo.toml`. The `feat/suite-ui-*`
  branches (key-hints, search-bar, status-bar, toast kinds) are all **merged to
  `main`**. Consumers pull it as a **git dependency** pinned to an umbrella `main`
  rev (currently `0a8dadc`) — no `path =` deps, so consumers build from a fresh
  clone with no sibling checkout.
- **Installer:** `install.sh` (build-and-copy method) **merged to `main` (PR #4)**
  and exercised in a **real end-to-end run** — all 6 Rust tools + the Python tool
  built and installed; verified fresh-clone-safe. See `INSTALLER-STATUS.md`.

## Current state — the tools

| Tool | Lang | ~LOC | Working branch | Notes |
|---|---|---|---|---|
| **Bulwark** | Rust | ~5.6k | `main` | Scanner + risk classifier. Stable. |
| **ScriptVault** | Rust | ~13.5k | `feature/p2-tui-wiring` | Largest tool; mid-redesign (see below). |
| **Toolbox Bridge** | Python | ~0.7k | `main` | Bulwark→ScriptVault sidecars; has pytest suite. |
| **ToolFoundry** | Rust | ~4.4k | `main` | Lifecycle/ownership/health. |
| **Workstate** | Rust | ~3.2k | `cleanup-workstate-cruft` | State compiler (snapshot v3). |
| **Proto** | Rust | ~6.2k | `main` | Guided protocol/checklist runner. |
| **RexOps** | Rust | ~7.6k | `main` | Cockpit (cli + tui crates). suite-ui git-dep landed; builds standalone from a fresh clone. |

All seven are functional ("Active"). RexOps' suite-ui git-dependency refactor is
merged to `main`. A couple of tools still sit on working branches with
in-progress work: **ScriptVault** (`feature/p2-tui-wiring`) and **Workstate**
(`cleanup-workstate-cruft`, dirty + 1 unpushed commit). Proto's `main` also has
substantial uncommitted local changes (~40 files) not yet reviewed/landed.

## Where we are in development

- The suite is **past prototype**: all tools exist, the contract layer is real and
  CI-validated, and the end-to-end dataflow (feeds → Workstate snapshot → RexOps)
  is wired.
- Recent focus (now landed): **shared `suite-ui` chrome** extraction/adoption,
  the **git-dependency refactor** so consumers build standalone from GitHub, and a
  **one-command installer** for fresh-machine reinstalls (proven by a real run).
- **ScriptVault** is the most actively evolving tool — a phased "world-class TUI"
  redesign: P1 (a core query/filter/frecency-ranking engine) is merged; P2
  (wiring the TUI + CLI to that engine, deleting logic that leaked into the UI) is
  in progress on `feature/p2-tui-wiring`.

## Done since last snapshot

- ✅ **Installer landed** (PR #4) + first real end-to-end run (all tools built,
  fresh-clone-safe).
- ✅ **`suite-ui` `feat/*` branches merged** to `main`.
- ✅ **`suite-ui` git-dependency refactor landed** on RexOps `main` (PR #3, rev
  bumped to `0a8dadc`); a fresh clone with no sibling umbrella builds via the git
  dep — confirmed by an actual no-sibling `cargo build --release`. ScriptVault
  doesn't consume `suite-ui`, so nothing to convert there.

## Major remaining work

1. **Finish ScriptVault P2** (TUI/CLI on the core engine) and continue its phased
   plan (keymap/layout, parameterized run, tag browser, palette/bulk, polish).
   Working branch: `feature/p2-tui-wiring`.
2. **RexOps TUI** — promote from the bash `bin/rex` reference to the real
   interactive cockpit on the shared contracts + `suite-ui`.
3. **Reconcile in-progress tool work** not yet landed:
   - **Workstate** (`cleanup-workstate-cruft`): dirty tree + 1 unpushed commit.
   - **Proto** (`main`): ~40 files of uncommitted local changes — decide whether
     they move to a feature branch and land.
4. **Suite-wide consistency** — bump the `suite-ui` git-dep rev in consumers when
   the shared crate changes; keep contract schemas in lockstep as tools evolve.

---

*Generated as a point-in-time snapshot. See `INSTALLER-STATUS.md` for installer
detail and each tool's own repo/README for specifics.*
