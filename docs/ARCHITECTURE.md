# Architecture

## Suite identity

The Linux Ops Suite is a set of focused, single-purpose Linux tools that cooperate
through stable **file-based contracts** (JSON/YAML). Each tool is independent, owns one
job, and never reaches into another tool's code.

## What this repository is — and is not

**This repo is the contract & index headquarters.** It holds the suite README, the
shared architecture, the integration map, the contract rules, the JSON schemas, and
example fixtures. It also hosts exactly **one** shared crate — `suite-ui`, the common
TUI chrome (see [Shared UI chrome](#shared-ui-chrome-suite-ui) below) — which makes
this a small Cargo workspace with a single member.

It is **not**:

- a monorepo for the *tools* — each tool still lives in its own repository;
- a place that vendors or imports another tool's **domain logic**.

The one build that lives here is `suite-ui`. It is pure presentation; it is not a
tool and owns no domain logic or data flow.

## Tool responsibilities

| Tool | Owns |
|---|---|
| **Bulwark** | Read-only inventory + risk/language classification. Source of truth for risk/inventory. |
| **ScriptVault** | Human-facing script search, preview, favorites/recents, and launch. |
| **Bridge** | Converts Bulwark risk data into ScriptVault sidecar YAML. Idempotent, dry-run-first, non-clobbering. |
| **ToolFoundry** | Tool lifecycle, ownership, health, drift, manifests. Source of truth for lifecycle. |
| **Workstate** | Read-only project/repository health. Never mutates repos. (Architecture phase.) |
| **RexOps** | The only suite-level consumer/orchestrator. Summarizes health/attention and launches. |

## One-way data flow

Data moves in **one direction**: producers write files, consumers read them.

```
Bulwark ──risk──▶ Bridge ──sidecar YAML──▶ ScriptVault
Bulwark ───────────────── scan JSON ───────────────▶ RexOps
ToolFoundry ───────────── workstate-feed JSON ──────▶ Workstate
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

## Shared UI chrome (`suite-ui`)

There is exactly **one** sanctioned exception to "no shared code": `suite-ui`, the
crate in [`crates/suite-ui`](../crates/suite-ui) that holds the suite's common TUI
*chrome* — the theme/palette (cyan/amber accents + a single `NO_COLOR` gate), the
rounded pane styling, health-status styles, and the common overlays (help sheet,
confirm modal, toast, command-palette frame). RexOps and ScriptVault are the intended
consumers.

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
workspace; consumers wire it in per-repo (git or path dependency) as a documented
follow-up.

## How RexOps consumes exports

RexOps reads each producer's export from its expected path (see
[INTEGRATION_MAP.md](INTEGRATION_MAP.md)), checks `schema_version`, ignores unknown
fields, and merges the results into its cockpit view. It treats every producer as
**optional**.

## Graceful degradation

If a producer's file is missing, stale, unreadable, or an unknown major version, RexOps
**does not fail**. It marks that producer as unavailable and renders everything else.
The suite is always usable with whatever subset of tools is currently present.
