# Tripwire Design

Tripwire is a read-only file-integrity instrument for the Linux Ops Suite. It
answers one question:

> Has anything important on this machine changed since I last looked?

It is the filesystem counterpart to **portman** (which watches the *network*
surface) and sits beside **rex-doctor** (install health) and **pulse** (suite
verdict) under the same house rule: single-purpose, read-only by default,
graceful degradation, JSON + human output, exit codes that drop straight into
cron.

## Core Direction

Tripwire records a **baseline** — a content hash and metadata snapshot of a set
of watched files and directories — and later **diffs** the live filesystem
against it, reporting what was **added**, **removed**, **modified**, or had its
**metadata changed** (mode, owner, size). That is the whole job.

It deliberately is *not*:

- An AIDE/Tripwire(tm)/Samhain replacement with a policy DSL, encrypted
  databases, signed reports, or kernel-level inotify monitoring.
- A daemon. There is no resident process. Tripwire runs, prints, and exits —
  the same shape as portman and rex-doctor. "Monitoring" means *cron runs
  `tripwire diff`*, exactly as portman documents itself as "tripwire-friendly."
- A repair tool. It never writes to a watched path, never restores, never
  quarantines. The only file it ever writes is its own baseline.

The guiding instinct, borrowed from portman: **the unit of work is one watched
path's recorded state, and a restart/no-op is not a change.** A file touched
but byte-identical is not a modification; a hash that differs is.

## How It Fits The Suite

```text
portman  ---- listening sockets -----> "what is open on the network?"
tripwire ---- watched files ----------> "what changed on disk?"
rex-doctor -- install health ---------> "is the suite wired up?"
pulse ------- suite verdict ----------> "is anything asking for attention?"
```

Tripwire is the suite's **filesystem-surface lens**, the on-disk analogue to
portman's network-surface lens. Like every suite tool it is read-only,
single-purpose, and emits a JSON envelope shaped like the other producers, so
its output can feed the snapshot pipeline later without redesign. It reaches
into nothing else's internals: it reads the filesystem and its own baseline,
nothing more.

## What Tripwire Watches

A baseline covers a **watch set**: an explicit list of paths plus per-path
options. The watch set is resolved from, in order of precedence:

1. `--path <PATH>` flags on the command line (repeatable). When present, these
   *are* the watch set for that run.
2. A config file: `--config <FILE>`, else the default
   `$XDG_CONFIG_HOME/linux-ops-suite/tripwire/watch.conf`
   (falling back to `~/.config/...`).
3. If neither exists, a small **built-in default set** of the paths an operator
   most often wants to know changed, each existing-only (missing = silently
   skipped, never an error):
   - `/etc/passwd`, `/etc/group`, `/etc/shadow` (readable only as root —
     degrades to a metadata-only entry otherwise; see Graceful Degradation)
   - `/etc/ssh/sshd_config`, `/etc/ssh/ssh_config`
   - `/etc/hosts`, `/etc/hostname`, `/etc/fstab`, `/etc/sudoers`
   - `/etc/crontab`, `/etc/cron.d` (directory)
   - `~/.ssh/authorized_keys`, `~/.bashrc`, `~/.zshrc`, `~/.profile`

The built-in set is a *convenience*, printed by `tripwire watch` so the operator
can see exactly what is covered and copy it into a config file to customize. It
is never silently authoritative: the JSON envelope always records which source
the watch set came from (`cli`, `config`, or `builtin`).

### Per-path options

Each watch entry has:

- `path` — the file or directory to watch.
- `recursive` (dirs only, default **true**) — descend into subdirectories.
- `follow_symlinks` (default **false**) — by default a symlink is recorded *as a
  symlink* (target string + metadata), not followed. Following is opt-in
  because a followed symlink can escape the watch set, which is a footgun for an
  integrity tool.
- `content` (default **true** for files) — hash file contents. Set false to
  watch metadata only (useful for large/append-only files like logs, or
  unreadable-but-present files).
- `exclude` — glob patterns pruned during a recursive walk (e.g. `*.log`,
  `__pycache__`, `.git`).

## The Recorded State

For each resolved path, the baseline records an **entry**. The entry is the
unit of diffing, and its identity is its **absolute path** (the analogue of
portman's `proto/addr:port` key). Owner/mtime are *not* part of identity — a
file rewritten in place is the same entry, modified; a renamed file is a
remove + add, which is correct and honest.

Each entry carries:

| field        | meaning                                                            |
|--------------|--------------------------------------------------------------------|
| `path`       | absolute path — the stable diff key                                |
| `kind`       | `file` / `dir` / `symlink` / `other` (socket, fifo, device, …)     |
| `size`       | bytes (files only)                                                 |
| `mode`       | octal permission bits, e.g. `0644` (the security-relevant part)    |
| `uid`/`gid`  | numeric owner/group (names are a render concern, not stored)       |
| `mtime`      | modification time, RFC3339 — informational, **not** identity       |
| `hash`       | content digest, files only, when `content` and readable            |
| `target`     | symlink target string, symlinks only                               |
| `unreadable` | true when the path exists but content couldn't be read (see below) |

### Hashing

Content hash is **SHA-256**, computed by a tiny dependency-free implementation
in `hash.rs` (the suite already hand-rolls small primitives — portman reads
`/proc` directly rather than depend on `ss`; `linux-ops-install` shells to
`sha256sum`). A from-scratch SHA-256 is ~150 lines, has zero supply-chain
surface, and is trivially testable against the FIPS-180 known-answer vectors.
We do **not** add a `sha2`/`ring` dependency: the suite's whole identity is lean
crates with `serde` + `clap` and nothing else.

Files are streamed in fixed-size chunks so a large watched file never loads
wholesale into memory.

## Commands

The command surface mirrors portman exactly — a default view, plus `baseline`
and `diff` — with one read-only addition (`watch`) and one inspection aid
(`verify`, a cron-quiet diff). Absent subcommand = the current view.

```text
tripwire                 # current view: scan the watch set and show its state now
tripwire watch           # show the resolved watch set + its source (cli/config/builtin)
tripwire baseline        # record the current state as the baseline to diff against
tripwire diff            # show what changed since the recorded baseline  ← the workhorse
tripwire verify          # like `diff` but prints nothing on clean (cron-quiet)
```

Mapped to portman's structure:

- `tripwire` ≙ `portman` (enumerate + render now)
- `tripwire baseline` ≙ `portman baseline` (record)
- `tripwire diff` ≙ `portman diff` (compare, exit 1 on change)
- `tripwire watch` — new: dumps the effective watch set, because unlike portman
  (whose subject is "all sockets") tripwire's subject is *configured*, so the
  operator must be able to see exactly what is and isn't covered.
- `tripwire verify` — `diff` with clean output suppressed, for silent cron use.

### Global flags (mirror portman)

```text
--json                 emit the JSON envelope instead of human output (any command)
--no-color             force monochrome (auto-off when stdout isn't a TTY)
-v, --verbose          show extra columns (hash prefix, uid/gid, mtime) in the view
--baseline-file PATH   use this baseline instead of the default XDG data path
--path PATH            watch this path (repeatable); overrides config/builtin
--config FILE          read the watch set from this config file
```

## Exit Codes (identical contract to portman)

```text
0   clean — current view rendered, or diff found no changes
1   drift — `diff`/`verify` found at least one change
3   tripwire itself could not run (no baseline, unreadable baseline,
    no data dir, watch set resolved to nothing)
```

The `diff` → exit 1 contract is the entire point: `tripwire diff` (or the
quieter `tripwire verify`) drops into cron/CI and *fails the job* the moment a
watched file drifts. This is the same promise portman's README makes about its
own diff, now for the filesystem.

## Output

### Human — current view (`tripwire`)

Aligned by hand, no table-drawing dependency, readable with color stripped —
the same renderer style as portman.

```text
tripwire — what is being watched, and its state now

PATH                          KIND  MODE   SIZE     STATE
/etc/passwd                   file  0644   2.1 KB   ok
/etc/shadow                   file  0640   1.3 KB   unreadable
/etc/ssh/sshd_config          file  0644   3.4 KB   ok
/etc/cron.d                   dir   0755   —        12 entries
~/.ssh/authorized_keys        file  0600   563 B    ok

14 watched · 1 unreadable · source: builtin
(not root: some system files show as ‘unreadable’; re-run with sudo for full coverage)
```

### Human — diff (`tripwire diff`)

```text
tripwire diff — changes since baseline

  + /etc/cron.d/backup            new file              0644
  - /home/tom/.ssh/old_key.pub    removed
  ~ /etc/ssh/sshd_config          content changed       hash 3f9c… → a17b…
  ~ /etc/passwd                   mode 0644 → 0666       [PERM]
  ~ /etc/sudoers                  owner 0:0 → 1000:0     [OWNER]

1 added · 1 removed · 3 modified
```

Change markers reuse portman's vocabulary and color rules:

- `+` green — added (a watched path now present that the baseline lacked).
- `-` red — removed (a baselined path now gone).
- `~` amber — modified. The modification is spelled out: `content changed`
  (hash differs), `mode A → B`, `owner u:g → u:g`, `size A → B`, or
  `type changed` (file became symlink, etc.).
- `[PERM]` / `[OWNER]` — a dim/amber tag flagging the security-relevant
  modifications (mode loosened, owner changed), the analogue of portman's
  `[PUBLIC]` scope note: the one bit of editorializing the tool does.

Clean diff: a single green line, like portman:

```text
No changes — the watch set matches the baseline.
```

`tripwire verify` prints nothing at all on clean (cron-quiet); on drift it
prints the same body as `diff`. Both exit 1 on drift.

### Color rules (suite-standard, same as portman/pulse)

- Green: clean verdict, added entries.
- Amber: modifications, the `[PERM]`/`[OWNER]` security tags.
- Red: removed entries.
- Cyan: the tool wordmark / headers only.
- Dim gray: labels, sizes, hashes, the not-root hint, mtimes.

State is always carried by the word and the marker shape too, never color
alone, and the whole interface is legible under `NO_COLOR` / non-TTY.

## JSON Envelope

Same shape family as portman's (`schema_version` + `source_tool` + payload), so
it slots into the suite's file-contract model. Current-view envelope:

```json
{
  "schema_version": 1,
  "source_tool": "tripwire",
  "watch_source": "builtin",
  "count": 14,
  "unreadable": 1,
  "entries": [
    {
      "path": "/etc/ssh/sshd_config",
      "kind": "file",
      "size": 3481,
      "mode": "0644",
      "uid": 0, "gid": 0,
      "mtime": "2026-06-01T12:00:00Z",
      "hash": "a17b9e…"
    }
  ]
}
```

Diff envelope (each change carries enough to render without a re-lookup, exactly
like portman's `ChangeOut`):

```json
{
  "schema_version": 1,
  "source_tool": "tripwire",
  "clean": false,
  "added": 1, "removed": 1, "modified": 3,
  "changes": [
    { "kind": "added",    "path": "/etc/cron.d/backup" },
    { "kind": "removed",  "path": "/home/tom/.ssh/old_key.pub" },
    { "kind": "modified", "path": "/etc/ssh/sshd_config",
      "fields": ["content"], "was_hash": "3f9c…", "now_hash": "a17b…" },
    { "kind": "modified", "path": "/etc/passwd",
      "fields": ["mode"], "was_mode": "0644", "now_mode": "0666", "security": true }
  ]
}
```

The `baseline` confirmation envelope is the same one-line shape portman uses:
`{"source_tool":"tripwire","action":"baseline","path":"…","count":14}`.

`#[serde(skip_serializing_if = "Option::is_none")]` on every optional field, so
a metadata-only or unreadable entry simply omits `hash`, the same way portman
omits unresolved owner links — the absence *is* the signal.

## Graceful Degradation

This is the heart of the design, mirrored from portman's "a socket we can't
resolve is never an error." For tripwire:

- **A path that doesn't exist** at scan time is silently skipped in the *view*
  but, in a *diff*, a baselined path now missing is a `removed` change (that's
  the whole point). A *configured* path that has never existed is simply absent.
- **A path readable as metadata but not content** (e.g. `/etc/shadow` as a
  non-root user) is recorded with all metadata and `unreadable: true`, **no
  hash**. In a diff, an entry that is unreadable in *both* baseline and live is
  reported as unchanged (we can't claim drift we can't see); a transition
  readable↔unreadable is itself reported as a modification.
- **Not root** never blocks a run. The view fills in as far as privileges allow
  and prints portman's exact-style hint: *"(not root: some system files show as
  unreadable; re-run with sudo for full coverage)."*
- **A symlink** is recorded as a symlink by default (never silently followed),
  so a swapped symlink target is caught as a `modified`/`target changed`, which
  is exactly the kind of tampering an integrity tool exists to catch.
- **The baseline file** distinguishes *not recorded yet* (→ exit 3 with "run
  `tripwire baseline`") from *recorded but corrupt* (→ exit 3 with the parse
  detail), the same `NoBaseline` vs `BadBaseline` split portman makes.
- **A baseline from a newer schema** is rejected loudly rather than silently
  misread — the versioned envelope exists for exactly this.

Errors (exit 3) are reserved for "tripwire could not even produce a view": no
data dir to anchor the baseline, the baseline unreadable/corrupt/absent on
`diff`, or a watch set that resolves to zero paths. Anything about an individual
file is data, not an error.

## Crate Layout

Modeled directly on portman's structure (`main` is a thin shell, the library
does the work and returns values, renderers derive everything from the model):

```text
crates/tripwire/
  Cargo.toml          # workspace deps: clap + serde + serde_json; dev: tempfile
  README.md           # same shape as portman's README
  src/
    main.rs           # thin CLI: parse flags, dispatch, render, structured exit code
    lib.rs            # public API: current() / save_baseline() / diff_against_baseline()
    error.rs          # TripwireError enum (NoBaseline/BadBaseline/SaveFailed/NoDataDir/EmptyWatchSet)
    model.rs          # Entry, EntryKind, the diff key + helpers (the portman model.rs analogue)
    watch.rs          # WatchSet resolution: cli > config > builtin; per-path options
    scan/
      mod.rs          # scan(): resolve watch set → walk → Entry list, sorted & stable
      walk.rs         # recursive directory walk with excludes, symlink policy, depth guard
      meta.rs         # stat → kind/mode/uid/gid/size/mtime via libc, dependency-free
    hash.rs           # from-scratch streaming SHA-256 (+ FIPS known-answer tests)
    baseline.rs       # Baseline envelope (save/load, versioned) + diff() + Change/Diff
    report.rs         # human table + diff renderer + JSON envelopes (Style mirrors portman)
    util.rs           # stdout_is_tty / is_root / XDG data_dir + baseline_path (portman parity)
```

Dependencies stay minimal and match the workspace set portman uses:

```toml
[dependencies]
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
[dev-dependencies]
tempfile = "3"
```

No `sha2`, no `walkdir`, no `notify`, no network, no async — same lean
philosophy as portman/rex-doctor/rex-check. The two hand-rolled primitives
(SHA-256 and the directory walk) are small, self-contained, and unit-tested.

## Design Principles (the suite contract, restated for tripwire)

- One question owns the tool: *what changed on disk since the baseline?*
- Read-only, always. The only file tripwire writes is its own baseline.
- The diff key is the absolute path; mtime/owner are data, not identity.
- A byte-identical file is not a change; only a differing hash is.
- Graceful degradation: an unresolvable file is data with `None`/`unreadable`,
  never an error. Errors mean tripwire couldn't run at all.
- Never silently follow symlinks; never silently descend where excluded.
- Exit 1 on drift so `diff`/`verify` is a real cron tripwire; exit 3 only when
  the tool itself failed.
- JSON envelope on every command, shaped like the rest of the suite's feeds.
- Lean crate: clap + serde only; hand-roll the two small primitives.
- The watch set is always visible (`tripwire watch`) and its source is always
  recorded — nothing is covered silently.

## Open Questions For Review

1. **Built-in default watch set** — is the list above the right starting set,
   or should `tripwire` with no config simply print "no watch set configured"
   and do nothing? (Leaning toward: ship the built-in set, because a tool that
   does nothing out of the box gets deleted — but I want your call.)
2. **`verify` subcommand** — worth having a cron-quiet variant, or is
   `tripwire diff >/dev/null` (relying purely on the exit code) enough and
   `verify` is one command too many for a single-purpose tool?
3. **Config format** — a real TOML parser would add a dependency, breaking the
   clap+serde-only rule. The lean alternative is a dead-simple line-based
   `watch.conf`: one path per line, optional `key=value` options after it
   (`recursive=false`, `content=false`, `exclude=*.log`). I'm leaning to the
   **line-based format** to hold the zero-extra-deps line. Agree?
4. **`baseline --init-config`** — should `baseline` also offer to write the
   resolved watch set to a config file the first time, so the operator's set is
   pinned? Or keep `baseline` doing exactly one thing.

Once you've signed off (and answered the open questions), I'll implement it
step by step: model + hash + watch resolution first with their unit tests, then
scan, then baseline/diff, then the renderers and CLI, with the full
fmt/clippy/test gate green before anything lands.
