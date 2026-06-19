# conductor

The Linux Ops Suite's **guided operator**. Conductor reads the suite's own state
files, derives a short **ordered runbook** — *do these things, in this order* —
and (in later phases) walks you through it, delegating each step to the tool that
owns it. It never writes a live file itself.

> **Phase 1 (this build)** is the read-only foundation: `status`, `health`,
> `plan`. The interactive TUI (bare `conductor`) and the guided `orchestrate`
> runner land in later phases.

## What it does

- Reads every suite contract (Workstate snapshot, RexOps aggregate, Bulwark
  feed, Proto sessions, an optional tripwire drift file) **fault-tolerantly** —
  a missing or malformed feed becomes "unavailable", never a crash.
- Derives a **deterministic, ordered plan** from built-in rules: refresh stale
  data first, capture a safety point before changes, investigate the worst
  findings (drift-correlated ones first), review failed jobs.
- Prints that plan as calm human text or a stable JSON envelope.

## Safety

Conductor never mutates state with its own code. Every step is classified by a
**ring**:

- `read-only` — would run a sibling that only reads (Ring 1).
- `changes state` — would run a sibling that writes; confirmed before it can run
  (Ring 2, Phase 3).
- `info` — shows a fix command, runs nothing (Ring 0).

**Phase 1 is entirely Ring 0**: it reads and renders, and runs nothing — no
subprocess, no writes, no TUI.

## Usage

```
conductor              print the situation + ordered plan (default)
conductor status       same as above
conductor plan         just the ordered steps, no prose
conductor health       per-feed and per-tool readiness
conductor --json …     emit the JSON envelope (schema_version + source_tool)
conductor --no-color   force monochrome
conductor --data-dir D read suite state from D instead of the XDG default
```

Exit codes: `0` ok (including "nothing to conduct"); `3` conductor itself could
not run (no data dir). `1`/`2` are reserved for the guided runner.

### JSON envelope

`--json` emits a stable object: `schema_version`, `source_tool: "conductor"`, a
deterministic `plan_id`, and `steps[]` each with a stable kebab `id`, `title`,
`command`, `ring`, and optional `annotation`. The ids let later tooling address a
plan/step across runs.

```json
{
  "schema_version": 1,
  "source_tool": "conductor",
  "plan_id": "plan-2f0a…",
  "situation": ["workstate snapshot is stale — refresh before trusting feeds"],
  "step_count": 2,
  "steps": [
    { "id": "refresh-stale-data", "title": "refresh stale data",
      "command": "workstate snapshot", "ring": "changes state" },
    { "id": "investigate-deploy-prod-sh", "title": "investigate deploy-prod.sh",
      "command": "bulwark show deploy-prod.sh", "ring": "read-only",
      "annotation": "same file as tripwire drift — start here" }
  ]
}
```

## Where it reads

Under `$XDG_DATA_HOME` (fallback `~/.local/share`), per
`docs/INTEGRATION_MAP.md`:

- `rexops/feeds/workstate.snapshot.json` — feed freshness + build time
- `rexops/snapshot.json` — aggregated findings (richest input)
- `workstate/feeds/bulwark.json` — findings fallback (high/critical)
- `proto/sessions/*.json` — failed jobs
- `tripwire/drift.json` — optional; enables the drift × finding correlation

See `CONDUCTOR_DESIGN.md` at the repo root for the full design.
