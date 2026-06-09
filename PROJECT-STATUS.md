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
  bars); the sole workspace member in the root `Cargo.toml`. Several `feat/suite-ui-*`
  branches in flight (key-hints, search-bar, status-bar, toast kinds).
- **Installer:** `install.sh` built (build-and-copy method) on
  `fix/installer-build-release`, pushed, not yet merged. See `INSTALLER-STATUS.md`.

## Current state — the tools

| Tool | Lang | ~LOC | Working branch | Notes |
|---|---|---|---|---|
| **Bulwark** | Rust | ~5.6k | `main` | Scanner + risk classifier. Stable. |
| **ScriptVault** | Rust | ~13.5k | `feature/p2-tui-wiring` | Largest tool; mid-redesign (see below). |
| **Toolbox Bridge** | Python | ~0.7k | `main` | Bulwark→ScriptVault sidecars; has pytest suite. |
| **ToolFoundry** | Rust | ~4.4k | `main` | Lifecycle/ownership/health. |
| **Workstate** | Rust | ~3.2k | `cleanup-workstate-cruft` | State compiler (snapshot v3). |
| **Proto** | Rust | ~6.2k | `main` | Guided protocol/checklist runner. |
| **RexOps** | Rust | ~7.6k | `chore/suite-ui-git-dep` | Cockpit (cli + tui crates). |

All seven are functional ("Active"). Several sit on non-`main` working branches
with in-progress cleanup/feature work not yet merged.

## Where we are in development

- The suite is **past prototype**: all tools exist, the contract layer is real and
  CI-validated, and the end-to-end dataflow (feeds → Workstate snapshot → RexOps)
  is wired.
- Recent focus: **shared `suite-ui` chrome** extraction/adoption, and a
  **one-command installer** for fresh-machine reinstalls.
- **ScriptVault** is the most actively evolving tool — a phased "world-class TUI"
  redesign: P1 (a core query/filter/frecency-ranking engine) is merged; P2
  (wiring the TUI + CLI to that engine, deleting logic that leaked into the UI) is
  in progress on `feature/p2-tui-wiring`.

## Major remaining work

1. **Land the installer** (`fix/installer-build-release` → `main`) and do a real
   full run; then it's the canonical fresh-machine path.
2. **Merge the in-flight branches** across tools and `suite-ui` (key-hints,
   search-bar, status-bar, toast kinds; per-tool cleanup branches) back to `main`.
3. **Finish ScriptVault P2** (TUI/CLI on the core engine) and continue its phased
   plan (keymap/layout, parameterized run, tag browser, palette/bulk, polish).
4. **RexOps TUI** — promote from the bash `bin/rex` reference to the real
   interactive cockpit on the shared contracts + `suite-ui`.
5. **Suite-wide consistency** — keep `suite-ui` adoption and the contract schemas
   in lockstep as tools evolve; resolve the `suite-ui` dependency story (path vs.
   published git dep) so every consumer builds standalone.

---

*Generated as a point-in-time snapshot. See `INSTALLER-STATUS.md` for installer
detail and each tool's own repo/README for specifics.*
