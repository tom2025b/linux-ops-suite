# Architecture

## Suite identity

The Linux Ops Suite is a set of focused, single-purpose Linux tools that cooperate
through stable **file-based contracts** (JSON/YAML). Each tool is independent, owns one
job, and never reaches into another tool's code.

## What this repository is — and is not

**This repo is the contract & index headquarters.** It holds the suite README, the
shared architecture, the integration map, the contract rules, the JSON schemas, and
example fixtures.

It is **not**:

- a monorepo — each tool lives in its own repository;
- a Rust workspace — there is no build here;
- a place that vendors or imports tool code.

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
ToolFoundry ───────────── rexops-feed JSON ─────────▶ RexOps
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

## Why shared code is forbidden

Shared libraries create hidden coupling: a change in one tool's internals can silently
break another. By forbidding cross-tool imports we force every dependency to pass
through a versioned, documented file contract — which is reviewable and stable. The
cost (a little duplication) is deliberately accepted.

## How RexOps consumes exports

RexOps reads each producer's export from its expected path (see
[INTEGRATION_MAP.md](INTEGRATION_MAP.md)), checks `schema_version`, ignores unknown
fields, and merges the results into its cockpit view. It treats every producer as
**optional**.

## Graceful degradation

If a producer's file is missing, stale, unreadable, or an unknown major version, RexOps
**does not fail**. It marks that producer as unavailable and renders everything else.
The suite is always usable with whatever subset of tools is currently present.
