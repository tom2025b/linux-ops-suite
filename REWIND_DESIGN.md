# Rewind Design

Rewind is the **history and safe-rollback instrument** for the Linux Ops Suite.
It answers one question:

> What did the suite's state look like before, and how do I get back there safely?

Where **portman** watches the network surface, **tripwire** watches the
filesystem surface, and **pulse** renders the one-line verdict, Rewind is the
suite's **time axis**: it records important suite states (Workstate snapshots,
producer feeds, the rolled-up RexOps snapshot), keeps a deduplicated history of
them, lets you compare any two points in time, and — uniquely in the suite —
can **restore** a recorded state, but only one Rewind itself captured and only
through a hard safety gate.

It follows every house rule the other tools follow — single-purpose, JSON +
human output, graceful degradation, exit codes that drop into cron, a lean
`clap + serde` crate that hand-rolls its one primitive (SHA-256). It breaks
exactly one rule, deliberately and narrowly: it is **read-only by default but
not read-only always**, because rollback is the entire point. That single
exception is fenced in by the safety rules in this document.

## Core Concept

The suite already *produces* state continuously: ToolFoundry, Bulwark, and Proto
emit feeds; Workstate compiles them into a versioned `snapshot.json`; RexOps
consumes it. But that state is **ephemeral** — each run overwrites the last.
There is no answer today to "what did the snapshot say yesterday?", "when did
this tool start drifting?", or "the feed compiler just produced garbage, give me
back the last good snapshot."

Rewind adds the missing axis. It is the suite's **black box recorder**: a
periodic or on-demand capture of the suite's own state files, stored
content-addressed so a hundred near-identical snapshots cost almost nothing,
queryable as a timeline, diffable point-to-point, and restorable under guard.

The unit of work, borrowed from the rest of the suite: **a capture is one
immutable point in time of a named set of source files.** A capture is never
mutated after it is written; "restore" does not edit history, it writes a past
capture's content back onto the live paths (after first capturing the present,
so the restore is itself undoable).

### Main use cases

1. **Suite black box.** A cron job runs `rewind capture` after each `rex run`,
   so there is always a rolling history of the compiled Workstate snapshot and
   every producer feed. When something looks wrong, the operator can see exactly
   what changed and when.
2. **Recover from a bad compile.** Workstate or a producer emits a broken/empty
   feed and RexOps now shows nonsense. `rewind restore --latest-good` writes the
   last captured-and-valid snapshot back into place so the cockpit works again
   while the root cause is investigated.
3. **Forensic timeline.** "This tool was healthy on Monday and `attention` by
   Wednesday — which capture did it flip in?" `rewind log` + `rewind diff`
   answer it without spelunking through overwritten files.
4. **Pin a known-good state before a risky change.** Before editing configs or
   running an upgrade, `rewind capture --label pre-upgrade` pins the current
   suite state so it can be compared or rolled back afterward.

### What Rewind is *not*

- **Not a general backup tool.** It is not restic/borg/timeshift. It captures a
  *configured set of suite state files*, not the whole disk, not `/home`, not
  block devices. The default set is the suite's own state; the operator can add
  paths, but the framing is "suite history," not "backup everything."
- **Not a VCS.** No branches, no merges, no rewriting history, no working tree.
  A capture is a flat immutable point; the only operations are capture / list /
  show / diff / restore / prune.
- **Not a daemon.** Like every suite tool it runs, prints, and exits. "Continuous
  history" means *cron runs `rewind capture`*, exactly as tripwire means *cron
  runs `tripwire diff`*.
- **Not a config editor.** It never edits a watched file in place. The only
  write to a watched path is a whole-file `restore`, and only of content Rewind
  itself recorded.

## How It Fits The Suite

```text
portman   ---- listening sockets ----> "what is open on the network?"   (read-only)
tripwire  ---- watched files --------> "what changed on disk?"          (read-only)
pulse     ---- suite verdict --------> "is anything asking for attention?" (read-only)
rewind    ---- captured state -------> "what did it look like before,    (read by default,
                                        and can I get back there safely?"  restore under guard)
```

Rewind is the suite's **temporal lens**. The others answer *what is true now*;
Rewind answers *what was true then, and how to return*. It is purely a
*consumer-side* tool: it reads the state files the other tools already produce
(it never reaches into their live internals or process state), records them, and
can write them back. It introduces no new coupling — it only knows file paths
and the universal JSON envelope (`schema_version` + `source_tool`), exactly the
contract the suite already pins under `contracts/`.

Concrete relationships:

- **Workstate** — its compiled `snapshot.json`
  (`$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`) is Rewind's flagship
  capture target. Rewind never *interprets* the snapshot; it stores the bytes
  and records the schema version so a restore can warn if it would put an old
  schema under a newer consumer.
- **RexOps** — the primary beneficiary. After a bad compile, restoring the last
  good snapshot is what gets the cockpit working again. (Future: RexOps could
  surface "N captures available, last good 2h ago" and offer a guarded restore
  from its UI — out of scope for v1, but the JSON envelope is shaped to allow
  it.)
- **Pulse** — Rewind emits the same JSON envelope family, so Pulse could later
  add a Rewind source ("history healthy: last capture 1h ago, 0 failed") with no
  redesign. Not built in v1; the shape simply doesn't preclude it.
- **Tripwire** — complementary, not overlapping. Tripwire tells you a file
  *changed* (and refuses to store content or roll back — that's a non-goal it
  states explicitly). Rewind stores the content and *can roll it back*. A
  natural pairing: tripwire detects the drift, Rewind restores the known-good
  copy. Rewind can also capture tripwire's own baseline file so the integrity
  baseline itself is recoverable.
- **The producer feeds** (`$XDG_DATA_HOME/workstate/feeds/*.json`) are
  secondary capture targets, so a forensic timeline can show which *input* feed
  caused a bad compiled snapshot.

## What Rewind Captures

A capture covers a **capture set**: a named list of source paths. The set is
resolved, in order of precedence (mirroring tripwire's resolution exactly):

1. `--path <PATH>` flags on the command line (repeatable). When present, these
   *are* the capture set for that run.
2. A config file: `--config <FILE>`, else the default
   `$XDG_CONFIG_HOME/linux-ops-suite/rewind/capture.conf`
   (falling back to `~/.config/...`). Same dead-simple line-based format
   tripwire uses — one path per line, optional `key=value` options after it —
   so we add **zero** extra dependencies (no TOML).
3. If neither exists, a small **built-in default set** of the suite's own state
   files, each existing-only (missing = silently skipped, never an error):
   - `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json` — the compiled
     snapshot (the flagship target)
   - `$XDG_DATA_HOME/workstate/feeds/` — the producer feeds directory
     (recursive, the inputs to the compile)
   - `$XDG_DATA_HOME/linux-ops-suite/tripwire/baseline.json` — tripwire's
     baseline (its real path; `linux-ops-suite/<tool>` is the suite-standard
     per-tool data dir), so the integrity baseline is itself recoverable

The built-in set is a convenience, printed by `rewind sources` so the operator
can see exactly what is covered and copy it into a config file. It is never
silently authoritative: every capture records which source the set came from
(`cli`, `config`, or `builtin`).

### Per-path options (subset of tripwire's, same syntax)

- `path` — the file or directory to capture.
- `recursive` (dirs only, default **true**) — descend into subdirectories.
- `follow_symlinks` (default **false**) — a symlink is recorded as a symlink
  (target string), not followed. Following is opt-in for the same footgun reason
  tripwire gives.
- `exclude` — glob patterns pruned during a recursive walk (e.g. `*.tmp`).

Content is **always** stored for files (that is Rewind's whole job — unlike
tripwire, which can watch metadata-only). A file Rewind cannot read is recorded
as a metadata-only entry with `unreadable: true` and no blob (see Graceful
Degradation); it simply isn't restorable.

## Storage Strategy

Rewind uses a **content-addressed store plus per-capture manifests**, under the
suite's standard XDG data root:

```text
$XDG_DATA_HOME/linux-ops-suite/rewind/        (default; --store overrides)
  objects/                # content-addressed blobs, sharded by hash prefix
    a1/                    #   first 2 hex chars
      a1b2c3…              #   file = the exact bytes of one captured file version
  captures/
    2026-06-19T14-22-05Z-3f9c.json   # one manifest per capture (the index entry)
  HEAD                    # optional: id of the most recent capture, for --latest
```

**Content-addressed objects.** Each captured file's bytes are hashed (SHA-256,
the same hand-rolled streaming implementation tripwire already ships — we lift
`hash.rs`) and stored once at `objects/<aa>/<full-hash>`. Two captures of an
unchanged snapshot reference the *same* object — so a daily capture of a
rarely-changing snapshot costs one manifest (a few hundred bytes), not a full
copy. This is the git-blob idea with none of git: no packs, no deltas, no
dependency, ~40 lines of store code.

**Manifest = the capture.** A manifest is a small JSON file listing, for each
captured path: its absolute path, kind, mode, uid/gid, size, mtime, the object
hash (its content key), the `source_tool` and `schema_version` parsed from the
blob if it is a recognized suite envelope, plus capture-level metadata
(timestamp, optional label, capture-set source, tool version). The manifest *is*
the unit `list`/`show`/`diff`/`restore`/`prune` operate on. Objects are an
implementation detail; the operator thinks in captures.

**Atomicity.** Objects are written to a temp name and renamed into place (rename
is atomic on the same filesystem); the manifest is written last and renamed in,
so a capture is either fully present or absent — never a half-written manifest
pointing at missing blobs. A crashed capture leaves at most an orphan object,
reclaimed by `rewind prune --gc`.

**Pruning.** `rewind prune` removes old captures by age or count
(`--keep-last N`, `--older-than 30d`); a manifest delete plus a mark-and-sweep
garbage collect of unreferenced objects (`--gc`). Nothing is ever auto-pruned;
the operator (or their cron line) decides retention. The current store size and
capture count are always visible in `rewind sources`/`--json`.

## Safety Rules (the restore contract)

Restore is the one place Rewind writes to live paths, so it is the most
carefully fenced part of the design. The rules, in priority order:

1. **Restore only Rewind's own captures.** Rewind can only write back content it
   recorded into its own store, to the exact absolute path that content was
   captured *from*. It cannot restore to an arbitrary target, cannot restore a
   file it never captured, and cannot reach into another tool's live internals.
   The store is the only source of restorable bytes.
2. **Dry-run by default; `--apply` to write.** Bare `rewind restore <capture>`
   prints exactly what it *would* do — per-path: unchanged / would-overwrite
   (with content + mode/owner diff) / would-create / target-missing-now /
   skipped (unreadable in capture) — and writes **nothing**. Only
   `rewind restore <capture> --apply` performs the writes. This makes "preview
   then commit" the default muscle memory, the inverse of an accidental
   clobber.
3. **Auto-capture the present first.** On `--apply`, Rewind first takes an
   automatic `pre-restore` capture of the *current* live state of every path it
   is about to touch, labeled `pre-restore:<target-capture-id>`. Every restore
   is therefore itself undoable by restoring that auto-capture. (Skippable with
   `--no-safety-capture` for the operator who knows what they're doing, but it
   is on by default and announced.)
4. **Atomic per-file writes.** Each restored file is written to a temp file in
   the target directory and renamed over the original (atomic same-fs replace),
   preserving the captured mode and — when running as the owner/root — uid/gid.
   A restore that can't set ownership warns and continues with content+mode
   (graceful degradation), it does not abort the whole batch.
5. **Schema-downgrade guard.** If restoring a recognized suite envelope
   (e.g. a Workstate snapshot) whose `schema_version` is *older* than the one
   currently live, Rewind prints a loud warning that it is putting an older
   schema under a possibly-newer consumer, and requires `--apply` to have been
   given knowingly. It never silently downgrades a contract.
6. **Never partial-silent.** If any path in a multi-path restore fails, Rewind
   reports per-path outcomes and exits non-zero; it never claims success for a
   batch where some files didn't land. Successfully written files stay written
   (they're atomic individually) but the summary is honest about the failures.

`restore --latest-good` is sugar for "find the most recent capture in which the
flagship snapshot parses as a valid envelope of the supported schema, and
restore that" — the common recovery path, with the same `--apply`/dry-run gate.

## Command Structure

The surface mirrors the suite's record/compare shape (tripwire's
view/baseline/diff, portman's baseline/diff) plus the history-specific verbs.
Absent subcommand = the timeline view (the most-asked question, "what history do
I have?").

```text
rewind                      # timeline view: list recent captures, newest first  ← default
rewind sources              # show the resolved capture set + its source (cli/config/builtin) + store stats
rewind capture [--label L]  # record the current capture set as a new immutable capture  ← the workhorse
rewind log                  # same as bare `rewind` (explicit alias for the timeline)
rewind show <capture>       # show one capture's manifest: paths, sizes, hashes, schema versions
rewind diff <a> [<b>]       # compare two captures (or capture-vs-live if <b> omitted)
rewind restore <capture>    # DRY-RUN by default: preview the writes; --apply to actually restore
rewind prune                # remove old captures by age/count; --gc reclaims unreferenced objects
```

`<capture>` accepts: a full id, a unique id prefix, `latest`, `latest-good`, or a
relative index (`~1` = one before latest), so the operator rarely types a full
timestamp.

### Global flags (mirror tripwire/portman)

```text
--json                  emit the JSON envelope instead of human output (any command)
--no-color              force monochrome (auto-off when stdout isn't a TTY)
-v, --verbose           extra columns (hash prefix, uid/gid, mtime, per-path schema)
--store PATH            use this store dir instead of the default XDG data path
--path PATH             capture this path (repeatable); overrides config/builtin
--config FILE           read the capture set from this config file
```

### Restore-only flags

```text
--apply                 actually perform the restore (without it, restore is a dry run)
--no-safety-capture     skip the automatic pre-restore capture (default: take it)
--latest-good           restore the most recent capture whose flagship snapshot is a valid envelope
```

## Exit Codes (suite contract, restore-aware)

```text
0   success — view/log/show/diff/sources rendered; capture written; prune done;
    restore dry-run rendered; restore --apply fully succeeded.
1   difference / drift — `rewind diff` found changes between the two points
    (parallel to tripwire/portman diff: a real difference is exit 1, so
    `rewind diff <id>` drops into cron as a "did the live state drift from this
    pinned capture?" check).
2   restore partially failed — --apply wrote some files but at least one path
    failed (permission, vanished dir, can't set owner-and-content). Distinct
    from a clean failure so cron can tell "nothing happened" from "half done."
3   rewind itself could not run — no/corrupt store, capture set resolved to
    nothing, requested capture id not found, unreadable/corrupt manifest,
    a manifest from a newer schema than this rewind understands.
```

The `diff → exit 1` contract makes `rewind diff <pinned-capture>` a cron
tripwire ("alert me if the live suite state ever drifts from this known-good
pin"), exactly the promise tripwire and portman make for their own diffs. Exit 2
is the one addition the suite doesn't have elsewhere, because Rewind is the one
tool that can half-write.

## Output

### Human — timeline (`rewind` / `rewind log`)

Hand-aligned, no table-drawing dependency, legible with color stripped — the
same renderer style as tripwire/portman.

```text
rewind — capture history (newest first)

ID         WHEN              LABEL          PATHS  SIZE     NOTE
3f9c…  latest  2026-06-19 14:22   pre-upgrade       3   8.1 KB   good
a17b…          2026-06-19 02:00   (cron)            3   8.0 KB   good
c0de…          2026-06-18 02:00   (cron)            3   7.9 KB   snapshot invalid
9b1a…          2026-06-17 02:00   (cron)            3   7.9 KB   good

4 captures · 11.4 KB on disk (deduped) · store: ~/.local/share/linux-ops-suite/rewind
```

`good` / `snapshot invalid` is the one bit of editorializing Rewind does: it
records whether the flagship snapshot in that capture parsed as a valid
supported envelope, which is what makes `--latest-good` meaningful. It is a tag,
carried by the word (never color alone).

### Human — diff (`rewind diff a b`)

```text
rewind diff — c0de… → 3f9c…  (2026-06-18 02:00 → 2026-06-19 14:22)

  ~ workstate.snapshot.json     content changed     7.9 KB → 8.1 KB   schema 4 = 4
  + tripwire/baseline.json      added in 3f9c…
  = workstate/feeds/            unchanged

1 changed · 1 added · 1 unchanged
```

`rewind diff <a>` with no second argument compares capture `<a>` against the
live files (the "has the live state drifted from this pin?" check, exit 1 on
difference). Markers reuse the suite vocabulary: `+` green added, `-` red
removed, `~` amber changed, `=` dim unchanged.

### Human — restore dry-run (the default, the safety-critical screen)

```text
rewind restore (DRY RUN) — would restore from a17b… (2026-06-19 02:00)
nothing has been written. re-run with --apply to perform the restore.

  ~ workstate.snapshot.json     would OVERWRITE   live 8.1 KB → captured 8.0 KB
  + tripwire/baseline.json      would CREATE      (not present live)
  = workstate/feeds/run.json    unchanged         (live already matches)
  ! workstate/feeds/old.json    SKIPPED           (unreadable in capture)

a safety capture of the current state will be taken before any write.
2 would change · 1 unchanged · 1 skipped
```

On `--apply`, the same body prints with past-tense outcomes (`RESTORED`,
`CREATED`, `unchanged`, `FAILED: <reason>`) and the safety-capture id is printed
first:

```text
rewind restore — safety capture taken: 8e2f… (pre-restore:a17b…)
  ~ workstate.snapshot.json     RESTORED          8.1 KB → 8.0 KB
  + tripwire/baseline.json      CREATED
2 restored · 0 failed · safety capture 8e2f…
```

### Color rules (suite-standard, same as tripwire/portman/pulse)

- Green: success verdicts, added entries, `RESTORED`/`CREATED`.
- Amber: changes, the `DRY RUN` banner, the schema-downgrade and safety warnings.
- Red: removals, `FAILED`, `snapshot invalid`.
- Cyan: the tool wordmark / headers only.
- Dim gray: labels, sizes, hashes, ids, mtimes, `unchanged`.

State is always carried by the word and the marker shape, never color alone; the
whole interface is legible under `NO_COLOR` and on a non-TTY.

## JSON Envelope

Same shape family as the rest of the suite (`schema_version` + `source_tool` +
payload), so it slots straight into the file-contract model and can later feed
Pulse/RexOps without redesign. A capture/log envelope:

```json
{
  "schema_version": 1,
  "source_tool": "rewind",
  "store": "/home/tom/.local/share/linux-ops-suite/rewind",
  "capture_count": 4,
  "store_bytes": 11680,
  "captures": [
    {
      "id": "3f9c1a…",
      "captured_at": "2026-06-19T14:22:05Z",
      "label": "pre-upgrade",
      "set_source": "builtin",
      "snapshot_valid": true,
      "path_count": 3,
      "bytes": 8281
    }
  ]
}
```

A `show` envelope expands one capture's per-path manifest entries:

```json
{
  "schema_version": 1,
  "source_tool": "rewind",
  "capture": {
    "id": "3f9c1a…",
    "captured_at": "2026-06-19T14:22:05Z",
    "label": "pre-upgrade",
    "set_source": "builtin",
    "entries": [
      {
        "path": "/home/tom/.local/share/rexops/feeds/workstate.snapshot.json",
        "kind": "file",
        "size": 8192,
        "mode": "0644",
        "uid": 1000, "gid": 1000,
        "mtime": "2026-06-19T14:21:00Z",
        "hash": "a17b9e…",
        "envelope_tool": "workstate",
        "envelope_schema_version": 4
      }
    ]
  }
}
```

A diff envelope (each change carries enough to render without re-lookup, like
tripwire's `ChangeOut`):

```json
{
  "schema_version": 1,
  "source_tool": "rewind",
  "from": "c0de…", "to": "3f9c…",
  "clean": false,
  "changed": 1, "added": 1, "removed": 0, "unchanged": 1,
  "changes": [
    { "kind": "changed", "path": "…/workstate.snapshot.json",
      "was_hash": "…", "now_hash": "…", "was_bytes": 7900, "now_bytes": 8281,
      "was_schema": 4, "now_schema": 4 }
  ]
}
```

A restore envelope reports the plan (dry-run) or outcome (`--apply`), including
the safety-capture id and per-path result, so a script can tell exactly what
happened:

```json
{
  "schema_version": 1,
  "source_tool": "rewind",
  "action": "restore",
  "from": "a17b…",
  "applied": true,
  "dry_run": false,
  "safety_capture": "8e2f…",
  "restored": 2, "failed": 0, "unchanged": 1, "skipped": 1,
  "results": [
    { "path": "…/workstate.snapshot.json", "outcome": "restored",
      "was_bytes": 8281, "now_bytes": 8000 },
    { "path": "…/old.json", "outcome": "skipped", "reason": "unreadable_in_capture" }
  ]
}
```

`#[serde(skip_serializing_if = "Option::is_none")]` on every optional field, so a
metadata-only/unreadable entry simply omits `hash` and `envelope_*` — the
absence *is* the signal, exactly as tripwire and portman do it.

## Graceful Degradation

Mirrored from tripwire's "a file we can't resolve is data, not an error":

- **A capture-set path that doesn't exist** at capture time is silently skipped
  (the built-in set is existing-only). A path present in a *baseline capture* but
  gone from a later one shows as `removed` in a diff — the point of the timeline.
- **A path readable as metadata but not content** is recorded metadata-only with
  `unreadable: true` and no blob; it appears in `show`/`diff` but is `SKIPPED`
  (not restorable) on restore, never an error.
- **Not root / not owner** never blocks a run. Capture records what privileges
  allow; restore writes content+mode and warns (continues) when it can't also
  set uid/gid, with the suite-standard hint.
- **A symlink** is captured as a symlink (target string), never silently
  followed — so a restore re-creates the symlink, it does not splat the target's
  bytes somewhere unexpected.
- **A blob referenced by a manifest but missing from `objects/`** (store
  corruption) makes that path un-restorable: it is reported as
  `missing_object`, the rest of the capture still restores, and `rewind prune
  --gc` / a future `rewind fsck` can report it. Store damage degrades to honest
  per-path failure, never a panic.
- **Store distinctions** parallel tripwire's `NoBaseline`/`BadBaseline`: *no
  store yet* (→ exit 3, "run `rewind capture`") vs *store present but a manifest
  is corrupt* (→ exit 3 with the parse detail). A manifest from a **newer
  schema** is rejected loudly, never silently misread.

Errors (exit 3) are reserved for "rewind couldn't even produce a view": no/
corrupt store, an empty capture set, an unknown/corrupt requested capture.
Anything about an individual file is data, not an error.

## Crate Layout

Modeled directly on tripwire/portman (thin `main`, the library does the work and
returns values, renderers derive everything from the model), and it **reuses
tripwire's already-proven primitives** (`hash.rs`, the directory `walk.rs`, the
`meta.rs` stat helper) rather than re-deriving them:

```text
crates/rewind/
  Cargo.toml          # workspace deps: clap + serde + serde_json; dev: tempfile
  README.md           # same shape as tripwire's README
  src/
    main.rs           # thin CLI: parse flags, dispatch, render, structured exit code
    lib.rs            # public API: capture() / log() / show() / diff() / restore() / prune()
    error.rs          # RewindError (NoStore/BadManifest/UnknownCapture/EmptySet/NoDataDir/RestoreFailed)
    model.rs          # Manifest, CaptureEntry, CaptureId, diff key + helpers
    set.rs            # CaptureSet resolution: cli > config > builtin; per-path options (tripwire parity)
    scan/
      mod.rs          # resolve set → walk → read+hash each file into the store, build a Manifest
      walk.rs         # recursive walk with excludes/symlink policy/depth guard (lifted from tripwire)
      meta.rs         # stat → kind/mode/uid/gid/size/mtime via libc, dependency-free (lifted)
    hash.rs           # from-scratch streaming SHA-256 (+ FIPS known-answer tests) (lifted from tripwire)
    store.rs          # content-addressed object store: put/get/has, atomic rename, mark-and-sweep gc
    capture.rs        # Capture/Manifest save+load (versioned), the capture workhorse, envelope sniffing
    diff.rs           # capture-vs-capture and capture-vs-live diff → Change/Diff
    restore.rs        # the guarded restore: dry-run plan, safety capture, atomic writes, per-path outcome
    report.rs         # human timeline/show/diff/restore renderers + JSON envelopes (Style mirrors tripwire)
    util.rs           # stdout_is_tty / is_root / XDG data_dir + store_path (tripwire parity)
```

Dependencies stay minimal and identical to tripwire's set:

```toml
[dependencies]
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
[dev-dependencies]
tempfile = "3"
```

No `sha2`, no `git2`, no `walkdir`, no compression crate, no network, no async —
same lean philosophy as the rest of the suite. The hash and walk primitives are
already written and tested in tripwire; the only genuinely new code is the
object store (~tiny), the manifest/capture layer, and the guarded restore.

> **Note on code reuse.** Three modules (`hash.rs`, `scan/walk.rs`,
> `scan/meta.rs`) are nearly identical between tripwire and rewind. Two honest
> options: (a) copy them into rewind now (fastest, keeps each crate
> self-contained, the suite already hand-rolls per-crate primitives), or (b)
> extract a tiny shared `crates/suite-fs` helper crate both depend on. I lean
> (a) for v1 — copying ~250 lines is cheaper than the abstraction, and matches
> the suite's "lean crate" instinct — with (b) as a clean follow-up if a third
> consumer appears. Flagged as an open question below.

## Design Principles (the suite contract, restated for rewind)

- One question owns the tool: *what was the suite's state before, and how do I
  get back there safely?*
- Read-only **by default**; the single write path (`restore`) is gated by:
  own-captures-only, dry-run-by-default, auto-safety-capture, atomic per-file
  writes, and a schema-downgrade guard.
- A capture is immutable. Restore writes content back; it never edits history.
- Content-addressed storage: identical bytes stored once; a capture is a
  manifest, not a copy.
- The diff key is the absolute path; mtime/owner are data, not identity (a
  byte-identical file is unchanged).
- Graceful degradation: an unreadable file or a missing blob is honest per-path
  data, never a panic. Errors mean rewind couldn't run at all.
- Never silently follow symlinks; never silently descend where excluded; never
  silently downgrade a schema; never claim a partial restore succeeded.
- Exit 1 on diff (cron drift check); exit 2 on partial restore; exit 3 only when
  the tool itself failed.
- JSON envelope on every command, shaped like the rest of the suite's feeds.
- Lean crate: clap + serde only; reuse tripwire's hand-rolled primitives.
- The capture set is always visible (`rewind sources`) and its source always
  recorded — nothing is captured silently.

## Open Questions For Review

1. **Storage model** — designed for a **content-addressed object store +
   manifests** (git-blob idea, no git, automatic dedup so daily captures of an
   unchanged snapshot are nearly free) — your confirmed choice. Recorded here so
   the decision is explicit in the doc; the simpler dir-per-capture alternative
   was rejected because the flagship target is captured hourly and is mostly
   identical between captures, so dedup is the difference between a tiny store
   and a bloated one.
2. **Restore safety default** — confirmed: `restore` is a **dry run by default**
   (`--apply` required) **and** auto-takes a `pre-restore` safety capture before
   any write. Two gates on the one dangerous command — defense in depth on the
   single rule-breaking path.
3. **Default capture set** — is "compiled Workstate snapshot + producer feeds
   dir + tripwire baseline" the right out-of-the-box set, or should it be just
   the flagship snapshot (narrowest, clearest) and let the operator opt the rest
   in via config? (I lean the three-item set: a black box that records only one
   file misses the forensic "which input feed broke it?" use case.)
4. **Shared-primitive reuse** — copy tripwire's `hash.rs`/`walk.rs`/`meta.rs`
   into rewind (self-contained, ~250 lines duplicated) vs extract a shared
   `suite-fs` crate now. I lean copy-for-v1, extract-if-a-third-consumer-appears
   (see the crate-layout note). Agree?
5. **`rewind fsck`** — worth a store-integrity subcommand (verify every
   manifest's referenced objects exist and re-hash to their key) in v1, or is
   `prune --gc` plus honest per-path `missing_object` reporting enough, with
   `fsck` deferred? (I lean defer — keep the v1 surface tight.)
6. **Compression** — store blobs raw (zero deps, snapshots are small JSON and
   gzip would add a crate) or accept one compression dependency for the case
   where someone captures large feeds? (I lean raw for v1 — the suite's targets
   are small JSON, and a dependency for a hypothetical large file violates the
   lean rule.)

Once you've signed off (and answered the remaining open questions, 3–6), I'll
implement it step by step: store + hash + manifest model first with their unit
tests, then capture + set resolution, then diff, then the guarded restore (with
the heaviest test coverage — it's the one writing path), then the renderers and
CLI, with the full `fmt --check` / `clippy -D warnings` (default **and**
`--all-features`) / full workspace test gate green before anything lands — the
same bar Tripwire cleared.
