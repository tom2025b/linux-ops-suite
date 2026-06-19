# rewind

History, snapshot, and safe-rollback for the Linux Ops Suite.

Where **portman** watches the network surface and **tripwire** watches the
filesystem surface, `rewind` is the suite's **time axis**: it records the suite's
own state files into a content-addressed store, lets you list the timeline, and —
in later phases — compare any two points and **restore** under a hard safety
gate. Read-only by default; the only thing it writes routinely is its own store.

See [`REWIND_DESIGN.md`](../../REWIND_DESIGN.md) at the repo root for the full
design (storage model, the restore safety contract, JSON envelopes, roadmap).

## Status

**Phase 1** — the storage layer plus `capture`, the timeline view, and
`sources`. `show`, `diff`, and the guarded `restore` arrive in later phases.

## What it captures

By default (no `--path`/`--config`), the suite's own state — each existing-only,
a missing path is skipped:

- `~/.local/share/rexops/feeds/workstate.snapshot.json` — the compiled snapshot
  RexOps consumes (the flagship target)
- `~/.local/share/workstate/feeds/` — the producer feeds that fed the compile
- `~/.local/share/linux-ops-suite/tripwire/baseline.json` — tripwire's baseline,
  so even the integrity baseline is recoverable

Override with repeatable `--path`, or a line-based `capture.conf`
(`$XDG_CONFIG_HOME/linux-ops-suite/rewind/capture.conf`). Precedence:
`--path` > `--config`/default conf > built-in set. The active set and its source
are always shown by `rewind sources` — nothing is captured silently.

## Commands

```text
rewind                      # timeline view: captures, newest first  (default)
rewind log                  # alias for the timeline
rewind capture [--label L]  # record the current capture set as a new immutable capture
rewind sources              # show the resolved capture set, its source, and store stats
```

Global flags: `--json`, `--no-color`, `--store PATH`, `--path PATH` (repeatable),
`--config FILE`.

## Storage

A content-addressed store under `$XDG_DATA_HOME/linux-ops-suite/rewind/`
(override with `--store`):

```text
objects/<aa>/<sha256>          deduped file blobs (one per unique content)
captures/<timestamp>-<id>.json one manifest per capture (the timeline entry)
HEAD                           id of the most recent capture
```

Two captures of byte-identical content share one object, so a daily capture of
an unchanged snapshot costs one small manifest, not a copy. Writes go through a
temp file + atomic rename, so a crash leaves the store consistent.

## Exit codes

```text
0   success — timeline/sources rendered, or a capture was written
3   rewind itself could not run — no/corrupt store, empty capture set,
    a manifest from a newer schema, or no data dir to anchor the store
```

(Exit 1 for `diff` drift and exit 2 for a partial `restore` arrive with those
commands in later phases.)

## Lean by design

Dependencies are `clap` + `serde` + `serde_json` + `chrono` (for the capture
timestamp). SHA-256 and the directory walk are hand-rolled (lifted from
tripwire); the object store is a few dozen lines. No `sha2`, no `git2`, no
`walkdir`, no compression, no network, no async — the same philosophy as the
rest of the suite.
