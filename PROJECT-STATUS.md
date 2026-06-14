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
- **The one shared-code exception:** the in-workspace TUI crates — `crates/thomas-tui`
  (general toolkit: terminal guard, theme, text/layout/keys, widgets) and
  `crates/suite-ui` (suite *chrome* layered on top: panes, overlays, status/search
  bars). Pure presentation — no domain logic or data flow, so they don't reintroduce
  coupling. Bulwark, RexOps, and ScriptVault depend on `suite-ui` (which pulls
  `thomas-tui` transitively).

### Data flow

```
ToolFoundry ─┐
Bulwark ─────┤  emit *.workstate-feed JSON
Proto ───────┘
                 │
            Workstate  ── compiles ──>  snapshot.json (schema v3)
                                             │
                                          RexOps  (cockpit / launcher; `rex run`)
                          (also reads Bulwark scan + ScriptVault export directly)

sidecar loop:  snapshot.json ──> Toolbox-Bridge ──> feeds/toolbox-bridge.json
               (risk/owner sidecar metadata for ScriptVault — via Workstate only)
```

`bin/rex` is the reference orchestrator (bash); the real RexOps TUI lives in its
own repo and will provide the interactive cockpit on the same contracts.

## Current state — umbrella

- **Contracts:** 9 JSON Schemas in `contracts/` (`bulwark.scan`, `proto.session`,
  `rexops.snapshot`, `scriptvault.export`, `workstate.snapshot`, plus the four
  `*.workstate-feed.v1` feeds for bulwark/proto/toolfoundry/toolbox-bridge). CI
  checks every JSON file is well-formed, every schema is a valid JSON Schema
  (metaschema check), and **every example validates against its schema**;
  `examples/` now carries one payload per schema (9 of 9), all CI-validated.
- **`suite-ui` / `thomas-tui`:** two in-workspace TUI crates (members of the root
  `Cargo.toml` alongside `toolbox-bridge`) — `thomas-tui` is the general toolkit
  (guard, theme, text/layout/keys, widgets) and `suite-ui` is the suite chrome on
  top (panes, overlays, key-hints/search/status bars). **All three** consumers —
  Bulwark, RexOps, and ScriptVault — pull `suite-ui` as a **git dependency** pinned
  to umbrella rev `2f5fa82` (no `path =` deps; `thomas-tui` comes in transitively),
  so each builds from a fresh clone with no sibling umbrella checkout. Verified by a
  fresh-clone simulation (empty `CARGO_HOME`, no sibling folder, `cargo build
  --locked`).
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
| **ToolFoundry** | Rust | ~4.4k | `main` | Lifecycle/ownership/health. |
| **Workstate** | Rust | ~3.2k | `main` | State compiler (snapshot v3). |
| **Proto** | Rust | ~6.2k | `main` | Guided protocol/checklist runner. |
| **RexOps** | Rust | ~7.6k | `main` | Cockpit (cli + tui crates). Consumes suite-ui via git dep. |

In-workspace crates (umbrella repo, not sibling tools):

| Crate | Lang | ~LOC | Notes |
|---|---|---|---|
| **thomas-tui** | Rust | ~3.2k | General TUI toolkit: terminal guard, theme, text/layout/keys, widgets. suite-ui builds on it. |
| **suite-ui** | Rust | ~1.6k | Suite TUI chrome (panes, overlays, status/search bars) on top of thomas-tui. Consumed by Bulwark/RexOps/ScriptVault via git dep. |
| **Toolbox-Bridge** | Rust | ~1.1k | Workstate snapshot → ScriptVault sidecar feed; unit + integration tests. Replaced the Python bridge. |

All six tools are functional ("Active") and sit on a clean `main`; the suite-ui
git-dependency conversion is landed and pushed across all three consumers.

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
- ✅ **`thomas-tui` extracted** as a second in-workspace crate (general TUI
  toolkit); `suite-ui` now layers its chrome on top and re-exports it, so the
  public API consumers see is unchanged.
- ✅ **`suite-ui` path→git-dependency conversion landed across ALL consumers** —
  Bulwark, RexOps, and ScriptVault pin suite-ui to umbrella rev `2f5fa82` as a git
  dep (no `path =` deps remain). Each consumer's CI dropped its sibling-checkout
  workaround for a plain root checkout. Confirmed fresh-clone-safe by a no-sibling,
  empty-`CARGO_HOME`, `cargo build --locked` simulation for all three.
- ✅ **`suite-ui` pin bumped `71a4fe5`→`2f5fa82`** across all three consumers
  (one PR each), adopting the latest shared-crate fixes (rendering refresh,
  `#[non_exhaustive]` enums, review fixes). RexOps gained wildcard match arms for
  the now-`#[non_exhaustive]` `suite_ui::Outcome`; all three green on CI.
- ✅ **Example payloads complete + CI-validated** — `examples/` now has one
  payload per schema (9 of 9), and CI validates every example against its schema
  (plus a metaschema check on the schemas themselves).

## Major remaining work

1. **Continue ScriptVault's phased TUI redesign** (keymap/layout, parameterized
   run, tag browser, palette/bulk, polish) on top of the merged core engine.
2. **RexOps TUI** — promote from the bash `bin/rex` reference to the real
   interactive cockpit on the shared contracts + `suite-ui`.
3. **Suite-wide consistency** — re-bump the `suite-ui` git-dep rev in consumers
   whenever the shared crate changes (one PR per consumer); keep contract schemas
   and their example payloads in lockstep as tools evolve.

---

*Generated as a point-in-time snapshot. For installer detail see `install.sh`;
for architecture and contracts see `docs/` (`ARCHITECTURE.md`,
`INTEGRATION_MAP.md`); and see each tool's own repo/README for specifics.*
