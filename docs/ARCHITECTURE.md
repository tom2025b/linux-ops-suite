# Architecture

## Suite identity

The Linux Ops Suite is a set of focused, single-purpose Linux tools that cooperate
through stable **file-based contracts** (JSON/YAML). Each tool is independent, owns one
job, and never reaches into another tool's code.

## What this repository is — and is not

**This repo is the contract & index headquarters.** It holds the suite README, the
shared architecture, the integration map, the contract rules, the JSON schemas, and
example fixtures. It also hosts a small Cargo workspace with five members:
`thomas-tui`, the general-purpose terminal-UI toolkit; `suite-ui`, the suite's
common TUI chrome layered on top of it (see
[Shared UI chrome](#shared-ui-chrome-suite-ui) below); `toolbox-bridge`, the
thin Workstate-mediated adapter between Bulwark and ScriptVault; `linux-ops-install`,
the prebuilt-release installer; and `rex-check`, a fast suite-repo health summary
(per-repo git status + source line counts).

It is **not**:

- a monorepo for the *tools* — each tool still lives in its own repository
  (Toolbox-Bridge is the one deliberate exception: it is a ~400-line adapter
  whose whole job is defined by contracts that live here, so it lives next to
  them);
- a place that vendors or imports another tool's **domain logic**.

## Tool responsibilities

| Tool | Owns |
|---|---|
| **Bulwark** | Read-only inventory + risk/language classification. Source of truth for risk/inventory. |
| **ScriptVault** | Human-facing script search, preview, favorites/recents, and launch. |
| **Toolbox-Bridge** | Converts Bulwark findings — read from the Workstate snapshot, never from Bulwark — into ScriptVault sidecar metadata, published as a versioned Workstate feed. Pure Rust, dry-run-capable, atomic writes. |
| **ToolFoundry** | Tool lifecycle, ownership, health, drift, manifests. Source of truth for lifecycle. |
| **Workstate** | Read-only state compiler: ingests the per-tool `workstate-feed` JSONs and compiles one versioned `snapshot.json` (schema v3). Never mutates repos. The snapshot is the source of truth RexOps and Toolbox-Bridge read. |
| **RexOps** | The only suite-level consumer/orchestrator. Summarizes health/attention and launches. |

## One-way data flow

Data moves in **one direction**: producers write files, consumers read them.

```
Bulwark ───────────────── workstate-feed JSON ──────▶ Workstate
ToolFoundry ───────────── workstate-feed JSON ──────▶ Workstate
Workstate ──snapshot──▶ Toolbox-Bridge ──sidecar feed──▶ ScriptVault
Bulwark ───────────────── scan JSON ───────────────▶ RexOps
ScriptVault ───────────── export JSON ──────────────▶ RexOps
Workstate ─────────────── snapshot JSON ────────────▶ RexOps
```

RexOps reads; it does not write back into any tool. Specialist tools never read
RexOps. There are no cycles.

## Why file-based contracts

- **Decoupling** — a producer and consumer share only a file shape, not code or a
  running process. Either can be rebuilt independently.
- **Inspectable** — every contract is a plain file you can `cat`, diff, and version.
- **Testable** — fixtures double as test data (see ToolFoundry's contract test).
- **Resilient** — a missing file is a normal, handled state, not a crash.

## Why shared *logic* is forbidden

Shared libraries create hidden coupling: a change in one tool's internals can silently
break another. By forbidding cross-tool imports **of domain logic and data** we force
every such dependency to pass through a versioned, documented file contract — which is
reviewable and stable. The cost (a little duplication) is deliberately accepted. This
remains the default: tools talk through file contracts, not code.

One narrow carve-out: Toolbox-Bridge deserializes the snapshot through Workstate's
published `Snapshot` serde types (a git dependency pinned by rev). Those types ARE
the file contract in Rust form — pure data shapes, no behaviour — so consuming them
prevents producer/consumer drift instead of creating logic coupling.

## Shared UI chrome (`suite-ui`)

There is exactly **one** sanctioned exception to "no shared code": `suite-ui`, the
crate in [`crates/suite-ui`](../crates/suite-ui) that holds the suite's common TUI
*chrome* — the theme/palette (cyan/amber accents + a single `NO_COLOR` gate), the
rounded pane styling, health-status styles, the common overlays (help sheet,
confirm modal, toast, command-palette frame), and the shared status-line widgets:
a persistent **`StatusBar`** job-status segment (running / done / failed /
cancelled / idle), a **`SearchBar`** live-filter input (prompt + query + match
count), and a **`KeyHints`** footer strip of `key → label` shortcut hints (keys
accented, labels dim — the same `(key, label)` pairs the help-sheet popup uses, so
the inline hints and the popup can't drift apart). RexOps and ScriptVault are the
intended consumers.

This does **not** reopen the coupling the file-contract rule prevents, because of
*what* `suite-ui` shares:

- it is **pure presentation** — styles, borders, modal layout, key-name constants;
- it carries **no domain types, no data, and no cross-tool data flow**;
- every component takes a theme + a borrowed data slice + a `Rect` and draws; it never
  reaches into a tool's state, and command dispatch/filtering/effects stay in the
  consuming app (`suite-ui` draws the box; the app owns behaviour).

So a change in `suite-ui` can alter how a pane *looks*, but it cannot corrupt a
snapshot, change a risk classification, or couple two tools' logic — the failure modes
the file-contract rule exists to stop. Shared *chrome* is safe to share for the same
reason shared *logic* is not: it has no semantics that two tools could disagree about.

`suite-ui` lives here (rather than in its own repo) so the contract HQ also owns the
one cosmetic thing every front-end must agree on. It is built and tested in this repo's
workspace; consumers (Bulwark, RexOps, ScriptVault) wire it in per-repo as a **git
dependency pinned to a commit of this repo** — no `path =` deps — so each builds from a
fresh clone without a sibling checkout.

### Two layers: `thomas-tui` and `suite-ui`

The shared UI is split into two crates so the *general* terminal plumbing stays free
of suite vocabulary:

- **`thomas-tui`** ([`crates/thomas-tui`](../crates/thomas-tui)) — a
  project-agnostic terminal-UI toolkit with **no suite or domain vocabulary**: the
  `NO_COLOR`-aware `Theme`, the panic-safe `Tui` RAII terminal guard, centering and
  Unicode-truncation helpers, shared keymap constants, and the domain-free widgets
  (`SearchBar`, `KeyHints`, `EmptyState`, `Counted`, `FilterChips`, `StatusStrip`,
  `Freshness`) and generic overlays (confirm modal, help sheet, command-palette
  frame). It depends only on `ratatui` + `crossterm` (plus an optional `clap`
  feature for the `--theme`/`--color` value enums).
- **`suite-ui`** ([`crates/suite-ui`](../crates/suite-ui)) — the thin **suite
  chrome** layered on `thomas-tui`. It re-exports the whole toolkit (so consumers
  keep importing everything as `suite_ui::*`) and adds the few widgets welded to the
  suite's own `Severity`/`Health`/`JobState`/`Outcome` vocabulary: `SeverityBadge`,
  `AttentionFlag`, `HealthStrip`, the `StatusBar` job segment, and the `Toast`
  flash. Its `clap` feature forwards to `thomas-tui/clap`.

Consumers depend on `suite-ui` (pinned by git rev); `thomas-tui` is pulled in
transitively. The same "pure presentation, no domain data flow" reasoning above
applies to both layers — neither carries suite data or couples two tools' logic.

## How RexOps consumes exports

RexOps reads each producer's export from its expected path (see
[INTEGRATION_MAP.md](INTEGRATION_MAP.md)), checks `schema_version`, ignores unknown
fields, and merges the results into its cockpit view. It treats every producer as
**optional**.

## Graceful degradation

If a producer's file is missing, stale, unreadable, or an unknown major version, RexOps
**does not fail**. It marks that producer as unavailable and renders everything else.
The suite is always usable with whatever subset of tools is currently present.
