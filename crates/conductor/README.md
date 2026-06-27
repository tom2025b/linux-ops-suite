# conductor

The Linux Ops Suite's **guided operator**. Conductor reads the one canonical
Workstate snapshot, derives a short **ordered runbook** — *do these things, in this
order* — and walks you through it interactively, delegating each step to the tool
that owns it. It never writes a live file itself.

> Conductor is the full guided operator: bare `conductor` (or `conductor
> orchestrate`) opens the interactive driver on a real TTY and walks you through
> the plan step by step — read-only steps run on `enter`, and a state-changing
> (Ring-2) step runs only after you explicitly confirm its exact command.
> Conductor still writes nothing itself; every change is a confirmed spawn of the
> tool that owns it. Piped / non-TTY / `--json` invocations print `status`, so
> scripts keep working.

## What it does

- Reads the **one canonical Workstate snapshot** through `workstate-schema` (the
  shared contract — the single source of truth for feed freshness, findings, and
  jobs), plus two non-snapshot inputs (binary presence on `$PATH`, and an optional
  tripwire drift file), all **fault-tolerantly** — a missing or malformed input
  becomes "unavailable", never a crash.
- Derives a **deterministic, ordered plan** from built-in rules: refresh stale
  data first, capture a safety point before changes, investigate the worst
  findings (drift-correlated ones first), review failed jobs.
- Prints that plan as calm human text or a stable JSON envelope.

## Safety

Conductor never mutates state with its own code. Every step is classified by a
**ring**:

- `read-only` — would run a sibling that only reads (Ring 1).
- `changes state` — would run a sibling that writes; confirmed before it can run
  (Ring 2).
- `info` — shows a fix command, runs nothing (Ring 0).

In the guided driver, a **read-only** step hands the terminal to a sibling tool and
marks `✓` when it exits. A **changes-state (Ring-2)** step is shown with its exact
command and runs only after you explicitly confirm it — a stray `enter` never fires
a state change. Conductor still writes nothing itself: every change is a confirmed
spawn of the tool that owns it, with that tool's own safety gate on top.

## Interactive mode

Bare `conductor` (on a terminal) opens the interactive driver — the same thing as
`conductor orchestrate`. It shows the ordered plan and walks you through it, with
the current step marked `▸`:

    enter  run step    s  skip    a  advance    r  rexops    ?  help    q  quit

- `enter` runs the current step. A **read-only** step spawns its sibling
  immediately, hands over the terminal, and marks the step `✓`. A
  **changes-state** step first opens a confirm showing the exact command — it
  runs only when you press `y` (a stray `enter` never fires a state change),
  with `s` to skip and `q` to back out.
- `s` skip · `a` advance focus · `r` hand off to the RexOps cockpit · `?` help ·
  `q` quit. A step whose tool exits non-zero is marked failed (`✗`); the cursor
  stays so you can retry or skip.

Conductor still changes nothing with its own code: every state change is a
confirmed spawn of the tool that owns it, with that tool's own safety gate on top.

Exit codes for a guided run (bare `conductor` and `orchestrate`): `0` clean /
all steps done / nothing to conduct, `1` a step that ran failed, `2` you quit
with steps still pending or skipped, `3` conductor itself could not run.

When not a terminal (piped / CI) or with `--json`, both fall back to the
read-only `status` output so scripts keep working.

## Usage

```
conductor              open the interactive plan (TTY) or print status (piped)
conductor status       print the situation + ordered plan
conductor plan         just the ordered steps, no prose
conductor health       per-feed and per-tool readiness
conductor --json …     emit the JSON envelope (schema_version + source_tool)
conductor --no-color   force monochrome
conductor --data-dir D read suite state from D instead of the XDG default
```

Exit codes: `0` ok (including "nothing to conduct"); `3` conductor itself could
not run (no data dir). `1`/`2` come from the guided runner (a step that ran failed /
you quit with steps still pending) — see the guided-run exit codes above.

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
  "situation": ["workstate snapshot is stale (tools) — refresh before trusting feeds"],
  "step_count": 2,
  "steps": [
    { "id": "refresh-stale-data", "title": "refresh suite snapshot",
      "command": "workstate snapshot", "ring": "changes state" },
    { "id": "investigate-deploy-prod-sh", "title": "investigate deploy-prod.sh",
      "command": "bulwark show deploy-prod.sh", "ring": "read-only",
      "annotation": "same file as tripwire drift — start here" }
  ]
}
```

## Where it reads

Under `$XDG_DATA_HOME` (fallback `~/.local/share`):

- `rexops/feeds/workstate.snapshot.json` — the **single canonical Workstate
  snapshot**, read through `workstate-schema`. Every snapshot-derived fact — feed
  freshness, findings, and failed jobs — comes from this one artifact, so the
  contract can't drift and conductor tracks the schema version automatically.
- `tripwire/drift.json` — optional, not yet part of the snapshot contract; enables
  the drift × finding correlation. The one input still read as its own file.
- `$PATH` — probed for the suite binaries (live environment state, not snapshot
  data); drives the wiring-gap rule.

See `CONDUCTOR_DESIGN.md` at the repo root for the full design.
