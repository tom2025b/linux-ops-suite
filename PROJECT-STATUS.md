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
  coupling. Bulwark, RexOps, and ScriptVault consume it.

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

sidecar loop:  snapshot.json ──> Toolbox-Bridge ──> feeds/toolbox-bridge.json
               (risk/owner sidecar metadata for ScriptVault — via Workstate only)
```

`bin/rex` is the reference orchestrator (bash); the real RexOps TUI lives in its
own repo and will provide the interactive cockpit on the same contracts.

## Current state — umbrella

- **Contracts:** 9 schemas present (`bulwark.scan`, `*.workstate-feed.v1` for
  bulwark/proto/toolfoundry/toolbox-bridge, `proto.session`, `rexops.snapshot`,
  `scriptvault.export`, `workstate.snapshot`). CI validates JSON contracts.
- **`suite-ui`:** active shared crate (theme + overlays + key-hints/search/status
  bars + the `Tui` terminal guard); a workspace member in the root `Cargo.toml`
  alongside `toolbox-bridge`. The `feat/suite-ui-*` branches (key-hints, search-bar, status-bar,
  toast kinds, app-runtime) are all **merged to `main`**. **All three** consumers —
  Bulwark, RexOps, and ScriptVault — pull it as a **git dependency** pinned to
  umbrella `main` rev `cf97f07` (no `path =` deps), so each builds from a fresh
  clone with no sibling umbrella checkout. Verified by a fresh-clone simulation
  (empty `CARGO_HOME`, no sibling folder, `cargo build --locked`).
- **Installer:** `install.sh` (build-and-copy method) **merged to `main` (PR #4)**
  and exercised in a **real end-to-end run**; verified fresh-clone-safe. Now
  all-Rust: it builds the six sibling-repo tools plus the in-workspace
  `toolbox-bridge` (the Python venv/pipx path was removed with the Python
  bridge). The installer is the canonical build-and-copy path now; the last
  installer-specific verification was `bash -n`, `shellcheck`, `--dry-run`, and
  the sandboxed wrapper/alias check, and the old `cargo install --path` route
  is gone.

## Current state — the tools

| Tool | Lang | ~LOC | Working branch | Notes |
|---|---|---|---|---|
| **Bulwark** | Rust | ~5.6k | `main` | Scanner + risk classifier. Stable. Consumes suite-ui via git dep (`tui` feature). |
| **ScriptVault** | Rust | ~13.5k | `main` | Largest tool. Consumes suite-ui via git dep (`clap` feature). |
| **Toolbox-Bridge** | Rust | ~0.6k | `main` (umbrella workspace) | Workstate snapshot → ScriptVault sidecar feed; unit + integration tests. Replaced the Python bridge. |
| **ToolFoundry** | Rust | ~4.4k | `main` | Lifecycle/ownership/health. |
| **Workstate** | Rust | ~3.2k | `main` | State compiler (snapshot v3). |
| **Proto** | Rust | ~6.2k | `main` | Guided protocol/checklist runner. |
| **RexOps** | Rust | ~7.6k | `main` | Cockpit (cli + tui crates). Consumes suite-ui via git dep. |

All seven are functional ("Active") and currently sit on a clean `main`. Bulwark,
RexOps, and ScriptVault each carry one unpushed commit: the suite-ui
path→git-dependency conversion (see "Done since last snapshot"), pending push.

## Where we are in development

- The suite is **past prototype**: all tools exist, the contract layer is real and
  CI-validated, and the end-to-end dataflow (feeds → Workstate snapshot → RexOps)
  is wired.
- Recent focus (now landed): **shared `suite-ui` chrome** extraction/adoption,
  the **git-dependency refactor** so all three consumers build standalone from
  GitHub, and a **one-command installer** for fresh-machine reinstalls (proven by
  a real run).
- **ScriptVault** is the most actively evolving tool — a phased "world-class TUI"
  redesign: a core query/filter/frecency-ranking engine with the TUI + CLI wired
  to it. Latest work is merged to `main`.

## Done since last snapshot

- ✅ **Installer landed** (PR #4) + first real end-to-end run (all tools built,
  fresh-clone-safe).
- ✅ **`suite-ui` `feat/*` branches merged** to `main` (incl. the `Tui` guard;
  the unused `App` runner was later removed — all tools drive their own loops).
- ✅ **`suite-ui` path→git-dependency conversion across ALL consumers** — Bulwark,
  RexOps, and ScriptVault now pin suite-ui to umbrella rev `cf97f07` as a git dep;
  no `path =` deps remain. Each consumer's CI dropped its sibling-checkout
  workaround for a plain root checkout. Confirmed fresh-clone-safe by a no-sibling,
  empty-`CARGO_HOME`, `cargo build --locked` simulation for all three. (Commits
  pending push.)

## Major remaining work

1. **Push the suite-ui git-dep conversion** — Bulwark, RexOps, and ScriptVault
   each have the conversion committed on `main` but not yet pushed.
2. **Continue ScriptVault's phased TUI redesign** (keymap/layout, parameterized
   run, tag browser, palette/bulk, polish) on top of the merged core engine.
3. **RexOps TUI** — promote from the bash `bin/rex` reference to the real
   interactive cockpit on the shared contracts + `suite-ui`.
4. **Suite-wide consistency** — bump the `suite-ui` git-dep rev in consumers when
   the shared crate changes; keep contract schemas in lockstep as tools evolve.

---

*Generated as a point-in-time snapshot. See `INSTALLER-STATUS.md` for installer
detail and each tool's own repo/README for specifics.*
