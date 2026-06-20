# rewind

History, snapshot, and safe-rollback for the Linux Ops Suite.

Where **portman** watches the network surface and **tripwire** watches the
filesystem surface, `rewind` is the suite's **time axis**: it records the suite's
own state files into a content-addressed store, lets you list the timeline,
compare any two points, and **restore** under a hard safety gate. Read-only by
default; the only write to a live path is a guarded, dry-run-by-default
`restore`, and the only thing it writes routinely is its own store.

See [`REWIND_DESIGN.md`](../../REWIND_DESIGN.md) at the repo root for the full
design (storage model, the restore safety contract, JSON envelopes, roadmap).

## Status

**Phase 3** — the full surface: the storage layer plus `capture`, the timeline
view, `sources`, `show` (one capture's manifest), `diff` (two captures, or a
capture against the live files), the guarded `restore`, and store maintenance
with `prune`.

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
rewind show <capture>       # show one capture's manifest: paths, sizes, hashes, schema
rewind diff <a> [<b>]       # compare two captures, or capture <a> against the live files
rewind restore <capture>    # restore a capture to its files — DRY-RUN by default
rewind prune [...]          # remove old captures by count/age; --gc reclaims objects
```

A `<capture>` is a full id, a unique id prefix, `latest`, `latest-good` (the
newest capture whose snapshot is a valid envelope), or a relative index like
`~1` (one before latest). `rewind diff <a>` with no second argument compares the
capture against the current live files — the "has the live state drifted from
this pin?" check, which exits `1` on any difference (and re-walks captured
directories, so a newly-appeared or deleted file counts as drift).

### restore — the one guarded write

`rewind restore <capture>` writes a capture's recorded files back to their
original paths, under a hard safety contract:

- **Dry-run by default.** With no `--apply` it prints the plan (what would be
  overwritten / created / left unchanged / skipped) and writes nothing.
- **`--apply` performs the writes**, and *first* takes a `pre-restore:<id>`
  safety capture of the current live state — so every restore is itself
  undoable. Skip it with `--no-safety-capture` (the plan warns you when it will).
- Each file is written to a temp file in the target directory then **renamed
  over** the original (atomic same-filesystem replace), restoring the captured
  mode and — best-effort — uid/gid (a can't-set-owner warns and continues).
- Restoring an envelope whose schema is **older** than the live one is flagged
  as a downgrade.
- A per-path failure never aborts the batch and never silently "succeeds": any
  failure is reported and surfaces as **exit 2**.

`rewind restore --latest-good` is shorthand for restoring the newest capture
with a valid snapshot (the `<capture>` argument is then optional).

### prune — store maintenance

`rewind prune` removes old captures and optionally garbage-collects their
objects. It is **immediate** (no dry run) and nothing is ever auto-pruned:

```text
rewind prune --keep-last N     # keep only the newest N captures
rewind prune --older-than 30d  # remove captures older than a duration (s/m/h/d)
rewind prune --keep-last 5 --gc# ...and sweep objects no surviving capture references
```

Removing a capture deletes only its manifest; the shared blobs stay until
`--gc` mark-and-sweeps the objects nothing references anymore.

Global flags: `--json`, `--no-color`, `-v/--verbose` (extra `show` columns:
mode, owner, hash prefix, mtime), `--store PATH`, `--path PATH` (repeatable),
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
0   success — timeline/sources/show/diff rendered, capture written, a diff
    found no differences, a restore dry-run/apply completed cleanly, or a
    prune ran
1   diff drift — `rewind diff` found a difference between the two points
    (so `rewind diff <pin>` drops into cron as a live-state drift check)
2   restore partial failure — `rewind restore --apply` could not write one or
    more files; what succeeded is reported, what failed is named (R6)
3   rewind itself could not run — no/corrupt store, empty capture set,
    an unknown/ambiguous capture selector, a manifest from a newer schema,
    a bad `--older-than` duration, or no data dir to anchor the store
```

## Lean by design

Dependencies are `clap` + `serde` + `serde_json` + `chrono` (for the capture
timestamp). SHA-256 and the directory walk are hand-rolled (lifted from
tripwire); the object store is a few dozen lines. No `sha2`, no `git2`, no
`walkdir`, no compression, no network, no async — the same philosophy as the
rest of the suite.
