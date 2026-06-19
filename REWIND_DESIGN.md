# Rewind Design

## Status

Design draft for the next major Linux Ops Suite tool.

Rewind is not an implementation plan yet. This document defines the intended purpose, boundaries, safety rules, command surface, storage model, and integration points so the first implementation can be built cleanly instead of growing into a rollback goblin with a wrench.

## Core Concept

**Rewind** is a history, snapshot, diff, and safe rollback tool for the Linux Ops Suite.

Its job is to record important local states and make them easy to inspect later. It can capture Workstate snapshots, suite feeds, tool outputs, selected project files, important configs, and other explicitly configured artifacts. Later, the user can list snapshots, inspect them, compare two points in time, and restore selected files through a guarded restore workflow.

Rewind should feel like a local time machine for operational state, but with brakes, labels, receipts, and a giant red warning light before anything destructive happens.

## Suite Fit

Rewind follows the existing Linux Ops Suite rules:

- **One job per tool:** Rewind owns history and safe rollback only.
- **Read-only by default:** Normal commands inspect, list, diff, and validate. Restore is never implicit.
- **File-based contracts:** Rewind consumes JSON/YAML/files from other tools. It does not import their logic.
- **Graceful degradation:** Missing tools, missing feeds, or unavailable paths should produce warnings, not hard crashes, unless the requested operation depends on them.
- **Human + JSON output:** Every important command should support clean human output and machine-readable `--json`.
- **Low-resource friendly:** Designed for Linux Mint-class machines. No database server, no daemon requirement, no heavyweight indexing service.
- **Rust-first:** The main tool should be Rust. A TUI may come later using the shared suite TUI style/chrome.
- **Stable contracts over clever magic:** Snapshot manifests should be versioned, documented, and testable.

## What Rewind Is

Rewind is:

- A local snapshot recorder.
- A timeline of important suite states.
- A diff viewer between states.
- A guarded restore planner.
- A provenance trail for what changed, when, and why.
- A safety layer before risky config or feed changes.

## What Rewind Is Not

Rewind is not:

- A full system backup solution.
- A replacement for Timeshift, Borg, Restic, Git, or filesystem snapshots.
- A package manager rollback system.
- A root-level disaster recovery tool in v1.
- A daemon that watches every file forever.
- A tool that silently changes files.
- A second Workstate compiler.
- A second Tripwire drift engine.

Rewind should be narrow and excellent: capture known important artifacts, compare them, and restore them safely when explicitly told.

## Main Use Cases

### 1. Capture the suite state before a risky change

Before refactoring RexOps, changing Workstate schemas, editing feeds, or running a big cleanup pass:

```bash
rewind snapshot --label "before rexops adapter refactor"
```

Expected result:

- Captures the current Workstate snapshot.
- Captures configured suite feed files.
- Captures selected suite config files.
- Records tool versions where possible.
- Produces a snapshot ID that can be used later.

### 2. See what changed since yesterday

```bash
rewind list
rewind diff latest previous
```

Expected result:

- Shows added, removed, and changed files.
- Shows high-level changes in Workstate data if structured diff support is available.
- Falls back to text/file hash diff when semantic diff is not available.

### 3. Compare two Workstate snapshots

```bash
rewind diff 2026-06-19T100000Z 2026-06-19T180000Z --kind workstate
```

Expected result:

- Shows changed tools, findings, scripts, sessions, feeds, and summary fields.
- Supports `--json` for RexOps or other tools to consume.

### 4. Restore one file safely

```bash
rewind restore latest --path ~/.config/example/config.toml
```

Expected result:

- Does not restore immediately.
- Prints a restore plan.
- Shows current hash, target hash, size, modified time, and destination path.
- Warns if the current file has changed since the snapshot.
- Requires an explicit apply flag before writing.

Actual restore requires:

```bash
rewind restore latest --path ~/.config/example/config.toml --apply
```

### 5. Create a pre-restore safety snapshot

Before applying any restore, Rewind should automatically record the current version of the target files as a pre-restore snapshot unless disabled by an explicit expert flag.

This gives the user a way to undo the undo.

### 6. Help RexOps show history

RexOps can use Rewind output to show:

- Last snapshot time.
- Snapshot count.
- Last restore attempt.
- Whether current Workstate differs from the latest recorded snapshot.
- Safe launcher actions like `Open Rewind timeline`, `Diff latest`, or `Create snapshot now`.

### 7. Give Pulse better context

Pulse can use Rewind to answer one important question:

> Is the suite healthy right now, and do we have a recent safe point?

Pulse should not own history. It should only read Rewind summary output.

### 8. Pair with Tripwire

Tripwire detects integrity drift. Rewind records restorable history.

Tripwire can say:

> This file changed.

Rewind can say:

> Here is what it looked like before, here is the diff, and here is a safe restore plan.

## Proposed Command Structure

The command surface should stay boring, predictable, and scriptable.

### `rewind snapshot`

Create a new snapshot.

```bash
rewind snapshot [OPTIONS]
```

Useful options:

```bash
rewind snapshot --label "before workstate v5 test"
rewind snapshot --kind suite
rewind snapshot --kind workstate
rewind snapshot --kind config
rewind snapshot --include ~/.config/some-tool/config.toml
rewind snapshot --project .
rewind snapshot --json
```

Proposed flags:

- `--label <text>`: Human label for the snapshot.
- `--kind <kind>`: Snapshot scope. Possible values: `suite`, `workstate`, `feeds`, `configs`, `project`, `manual`.
- `--include <path>`: Add a file or directory to this snapshot.
- `--exclude <pattern>`: Exclude paths from this snapshot.
- `--project <path>`: Capture project-local configured files.
- `--note <text>`: Add a short note.
- `--json`: Emit machine-readable output.
- `--dry-run`: Show what would be captured without writing snapshot data.

Default behavior:

- Create a suite-level snapshot using configured default paths.
- Capture only known/allowed files.
- Warn and continue for missing optional files.
- Hard-fail only if no snapshot content can be captured.

### `rewind list`

List available snapshots.

```bash
rewind list [OPTIONS]
```

Examples:

```bash
rewind list
rewind list --kind workstate
rewind list --since 7d
rewind list --json
```

Output should include:

- Snapshot ID.
- Created time.
- Label.
- Kind/scope.
- Entry count.
- Total stored size.
- Whether verification passes.
- Whether it was created automatically or manually.

### `rewind show`

Show details for one snapshot.

```bash
rewind show <snapshot-id> [OPTIONS]
```

Examples:

```bash
rewind show latest
rewind show latest --files
rewind show latest --json
```

Output should include:

- Manifest metadata.
- Captured paths.
- Hashes.
- File sizes.
- Source tool versions if known.
- Warnings recorded during capture.

### `rewind diff`

Compare two snapshots or compare a snapshot with the current filesystem.

```bash
rewind diff <from> <to> [OPTIONS]
```

Examples:

```bash
rewind diff previous latest
rewind diff latest current
rewind diff latest current --path ~/.config/example/config.toml
rewind diff previous latest --kind workstate
rewind diff previous latest --json
```

Diff categories:

- `added`
- `removed`
- `modified`
- `metadata_changed`
- `unchanged`
- `missing_current`
- `unreadable_current`
- `unsupported_type`

For structured JSON files, Rewind can eventually support semantic diffs, but v1 can start with hash, size, and text diff.

### `rewind restore`

Plan or apply a restore.

```bash
rewind restore <snapshot-id> [OPTIONS]
```

Examples:

```bash
rewind restore latest --path ~/.config/example/config.toml
rewind restore latest --path ~/.config/example/config.toml --apply
rewind restore latest --kind workstate --dry-run
rewind restore latest --path ./contracts/workstate.snapshot.schema.json --apply
```

Important behavior:

- Without `--apply`, this command only prints a plan.
- With `--apply`, it writes files only after all safety checks pass.
- Restore should support selected paths first, not whole-system restore.
- Whole-snapshot restore should be a later feature, guarded by extra flags.

Proposed flags:

- `--path <path>`: Restore only this path. Can be repeated.
- `--kind <kind>`: Restore a known group such as `workstate` or `feeds`.
- `--to <path>`: Restore to an alternate destination instead of overwriting the original path.
- `--apply`: Actually perform writes.
- `--yes`: Non-interactive confirmation for scripts. Requires `--apply`.
- `--backup-current`: Force a pre-restore snapshot of current target files.
- `--no-backup-current`: Expert-only escape hatch. Should print a loud warning.
- `--json`: Emit restore plan/result as JSON.

### `rewind verify`

Verify stored snapshots and blobs.

```bash
rewind verify [snapshot-id] [OPTIONS]
```

Examples:

```bash
rewind verify
rewind verify latest
rewind verify --json
```

Checks:

- Manifest parses.
- Manifest schema version is supported.
- Referenced blobs exist.
- Blob SHA-256 matches manifest.
- Snapshot index is consistent.

### `rewind prune`

Remove old snapshots according to retention rules.

```bash
rewind prune [OPTIONS]
```

Examples:

```bash
rewind prune --dry-run
rewind prune --keep-last 50 --apply
rewind prune --older-than 90d --apply
```

Safety:

- Dry-run by default.
- Requires `--apply` to delete stored history.
- Never prunes snapshots marked `pinned` unless `--include-pinned` is explicitly passed.

### `rewind pin` / `rewind unpin`

Protect important snapshots from pruning.

```bash
rewind pin <snapshot-id>
rewind unpin <snapshot-id>
```

### `rewind export`

Export a snapshot bundle for sharing, archiving, or debugging.

```bash
rewind export <snapshot-id> --output rewind-bundle.tar.gz
```

This should be optional and can wait until after core snapshot/list/diff/restore works.

## Suggested Output Style

### Human output

Example snapshot output:

```text
Rewind snapshot created

ID:        rw-20260619-181530-a8f31c
Label:     before workstate v5 test
Kind:      suite
Entries:   14 files
Size:      2.4 MiB stored
Warnings:  1 optional feed missing

Next:
  rewind show rw-20260619-181530-a8f31c
  rewind diff rw-20260619-181530-a8f31c current
```

Example restore plan output:

```text
Restore plan only. No files were changed.

Snapshot: rw-20260619-181530-a8f31c
Target:   ~/.config/example/config.toml
Action:   replace current file with snapshot version

Current:
  sha256: 7e9...
  size:   4.1 KiB

Snapshot:
  sha256: a18...
  size:   3.9 KiB

Safety:
  [ok] destination is inside allowed roots
  [ok] snapshot blob verified
  [ok] current file can be backed up
  [warn] current file differs from latest captured version

To apply:
  rewind restore rw-20260619-181530-a8f31c --path ~/.config/example/config.toml --apply
```

### JSON output

Every command that reports meaningful state should support `--json`.

Example snapshot JSON shape:

```json
{
  "schema_version": 1,
  "status": "ok",
  "snapshot_id": "rw-20260619-181530-a8f31c",
  "kind": "suite",
  "label": "before workstate v5 test",
  "created_at": "2026-06-19T18:15:30Z",
  "entry_count": 14,
  "stored_bytes": 2516582,
  "warnings": [
    {
      "code": "optional_path_missing",
      "path": "$XDG_DATA_HOME/workstate/feeds/proto.sessions.json"
    }
  ]
}
```

## Storage Strategy

### Default storage root

Use XDG paths.

Preferred default:

```text
$XDG_STATE_HOME/rewind
```

Fallback:

```text
~/.local/state/rewind
```

Possible layout:

```text
$XDG_STATE_HOME/rewind/
  index.json
  snapshots/
    rw-20260619-181530-a8f31c.json
    rw-20260619-193012-b91d02.json
  blobs/
    sha256/
      a1/8f/a18f...
      7e/90/7e90...
  restore-plans/
    restore-20260619-200455.json
  locks/
    rewind.lock
  logs/
    rewind.log
```

### Manifest-first design

Each snapshot should be represented by a manifest JSON file. The manifest records metadata and references content-addressed blobs.

Proposed snapshot manifest fields:

```json
{
  "schema_version": 1,
  "snapshot_id": "rw-20260619-181530-a8f31c",
  "created_at": "2026-06-19T18:15:30Z",
  "label": "before workstate v5 test",
  "kind": "suite",
  "created_by": "rewind 0.1.0",
  "host": {
    "hostname": "mint-box",
    "os": "linux"
  },
  "sources": [
    {
      "tool": "workstate",
      "path": "$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json",
      "required": false
    }
  ],
  "entries": [
    {
      "logical_name": "workstate.snapshot",
      "original_path": "$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json",
      "entry_type": "file",
      "sha256": "a18f...",
      "size_bytes": 12345,
      "mode": "0644",
      "modified_at": "2026-06-19T18:10:02Z",
      "blob_ref": "sha256/a1/8f/a18f...",
      "restore_allowed": true
    }
  ],
  "warnings": []
}
```

### Content-addressed blobs

Store file contents by SHA-256.

Benefits:

- Deduplicates repeated snapshots.
- Makes verification simple.
- Makes restore safer because the content hash is known.
- Avoids storing duplicate copies of identical feeds or configs.

### Atomic writes

Snapshot creation should be crash-safe:

1. Create temp manifest.
2. Copy blobs to temp paths.
3. Verify hashes.
4. Move blobs into final content-addressed locations.
5. Move manifest into `snapshots/`.
6. Update `index.json` last.

### Locking

Rewind should use a simple file lock to prevent concurrent snapshot/restore/prune operations from corrupting the store.

Read commands like `list`, `show`, and `diff` may run concurrently where safe.

### Retention

Initial retention can be manual.

Later config can support:

```toml
[retention]
keep_last = 100
keep_daily = 14
keep_weekly = 8
keep_monthly = 6
prune_unlabeled_after_days = 90
```

Pruning should always be dry-run by default.

## What To Capture By Default

Default suite snapshot candidates:

### Workstate and suite feeds

- `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`
- `$XDG_DATA_HOME/workstate/feeds/*.json`
- Workstate adapter outputs when present
- Toolbox-Bridge sidecar feed when present
- Proto session summaries when present

### Suite contracts and configs

When run from the umbrella repo or a project repo:

- `contracts/*.schema.json`
- suite config files such as `*.toml`, `*.yaml`, `*.yml`, where explicitly included
- tool manifests where explicitly included

### Critical user configs

Not default in v1 unless configured. Config snapshots should be opt-in.

Example config:

```toml
[include]
paths = [
  "~/.config/rexops/config.toml",
  "~/.config/workstate/config.toml",
  "~/.config/toolfoundry/config.toml"
]
```

Rewind should not go spelunking through the user’s home directory on its own. No haunted attic crawls.

## Safety Rules

Restore is the dangerous part. It needs strict rules from day one.

### Rule 1: Restore is plan-only by default

This command must not write files:

```bash
rewind restore latest --path ~/.config/example/config.toml
```

It only prints the plan.

Writing requires:

```bash
rewind restore latest --path ~/.config/example/config.toml --apply
```

### Rule 2: Selected restore first

v1 should only restore explicitly selected paths.

Good:

```bash
rewind restore latest --path ~/.config/example/config.toml --apply
```

Avoid in v1:

```bash
rewind restore latest --all --apply
```

Whole-snapshot restore can come later, but it should require extra confirmation and probably a TUI review screen.

### Rule 3: Pre-restore snapshot

Before overwriting anything, Rewind should snapshot the current target file(s).

This creates a safety breadcrumb:

```text
pre-restore-rw-20260619-201200-c991e2
```

If restore goes wrong, the user can restore the pre-restore snapshot.

### Rule 4: Verify before writing

Before restore:

- Read manifest.
- Verify manifest schema version.
- Verify blob exists.
- Verify blob SHA-256.
- Verify destination path is allowed.
- Verify parent directory exists or can be created safely.
- Verify target type is supported.

If any required check fails, restore must abort.

### Rule 5: Path allowlist

By default, Rewind should only restore into allowed roots:

- The original captured path, if it is under the user home directory.
- The current project directory, if the snapshot was project-scoped.
- Configured allowed roots.
- Alternate destination passed with `--to`, if it is inside an allowed root.

Rewind should refuse system paths like these by default:

```text
/etc
/usr
/bin
/sbin
/lib
/boot
/dev
/proc
/sys
/run
```

Future expert support for system config restore can be added later, but not in the first safe version.

### Rule 6: No special files in v1

v1 should snapshot and restore regular files only.

Directories can be represented as containers, but restore should write regular files.

Skip or warn for:

- sockets
- pipes/FIFOs
- block devices
- character devices
- hardlink restoration
- symlinks that point outside allowed roots

### Rule 7: Conservative permissions

By default, restore file contents and safe user permissions only.

Do not restore owner/group unless explicitly supported later.

Do not set setuid/setgid/sticky bits in v1.

### Rule 8: Atomic restore

When replacing a file:

1. Write to a temp file in the same directory.
2. Verify temp file hash.
3. Preserve safe permissions.
4. Rename temp file over destination atomically.
5. Verify final file hash.
6. Record restore result.

### Rule 9: No command execution

Rewind should not run shell commands as part of snapshot or restore.

It can read files. It can write files during explicit restore. It should not execute scripts, package managers, migrations, or hooks in v1.

### Rule 10: Loud unsafe flags

Any future unsafe feature should require ugly, explicit flags.

Examples:

```bash
--allow-system-paths
--allow-owner-restore
--allow-special-files
--no-backup-current
```

These should not exist until there is a real use case.

## Integration With Existing Tools

### Workstate

Workstate is the central source of truth for normalized suite state. Rewind should treat Workstate output as one of its most important capture targets.

Rewind should capture:

- The compiled Workstate snapshot.
- Workstate feed inputs when available.
- Workstate schema/contract versions when available.
- Workstate warnings or metadata if present in the snapshot.

Boundary:

- Workstate compiles current truth.
- Rewind records historical truth.
- Workstate should not become a history database.
- Rewind should not become a Workstate compiler.

Possible future command:

```bash
rewind snapshot --kind workstate
```

Possible future integration:

```bash
workstate build --snapshot-label "before proto schema change"
rewind snapshot --kind workstate --label "before proto schema change"
```

### RexOps

RexOps is the cockpit and top-level consumer. It should launch or summarize Rewind, not own its logic.

RexOps can show:

- Latest Rewind snapshot.
- Snapshot health.
- Last diff summary.
- Whether current Workstate differs from the latest Rewind snapshot.
- Safe actions:
  - `Create Rewind snapshot`
  - `List Rewind snapshots`
  - `Diff latest vs current`
  - `Open restore planner`

Boundary:

- RexOps can call Rewind CLI and read JSON output.
- RexOps should not parse Rewind internals directly.
- RexOps should not perform restore itself.

### Pulse

Pulse gives one calm suite-health verdict.

Pulse can consume a tiny Rewind summary:

```bash
rewind status --json
```

Possible fields:

```json
{
  "latest_snapshot_at": "2026-06-19T18:15:30Z",
  "snapshot_count": 42,
  "latest_verify_status": "ok",
  "last_restore_status": "none",
  "warnings": []
}
```

Pulse can then report:

- No recent snapshot.
- Latest snapshot is stale.
- Snapshot store verification failed.
- Rewind is healthy.

Boundary:

- Pulse reads status only.
- Pulse does not create snapshots by default.
- Pulse does not restore.

### Tripwire

Tripwire owns integrity baseline and drift detection. Rewind owns historical snapshots and safe restore.

Good integration:

- Tripwire detects drift in a watched file.
- Rewind can show the previous captured content.
- Rewind can produce a restore plan.

Possible flow:

```bash
tripwire diff --json > drift.json
rewind diff latest current --from-tripwire drift.json
rewind restore latest --path <changed-path>
```

Boundary:

- Tripwire should not store full restorable file history.
- Rewind should not replace Tripwire’s baseline/drift role.
- Tripwire answers “what changed?”
- Rewind answers “what did it look like before, and can I safely restore it?”

### Proto

Proto is a guided protocol/checklist runner. It can use Rewind as a safety step before a risky protocol.

Example protocol step:

```yaml
- name: Create safety snapshot
  command: rewind snapshot --label "before protocol: {{ protocol_name }}"
  required: true
```

Proto can also emit session records that Rewind may capture.

Boundary:

- Proto guides humans.
- Rewind records state.
- Proto should not hide restore behind a checklist step without explicit user review.

### ToolFoundry

ToolFoundry owns tool lifecycle, ownership, and health. Rewind can capture ToolFoundry manifests and feeds.

Useful captures:

- Tool manifests.
- Workstate feed emitted by ToolFoundry.
- Tool health output if written as JSON.

Boundary:

- ToolFoundry owns desired lifecycle state.
- Rewind records historical copies of that state.

### Bulwark

Bulwark is a read-only scanner/risk classifier. Rewind can capture Bulwark feed output through Workstate or direct configured feed paths.

Preferred path:

- Bulwark emits findings.
- Workstate normalizes findings.
- Rewind captures Workstate snapshot and feeds.

Boundary:

- Rewind does not rescan scripts.
- Rewind does not classify risk.

### ScriptVault

ScriptVault owns script discovery, preview, launch UX, favorites, recents, and sidecar metadata.

Rewind can capture:

- ScriptVault sidecar metadata feed.
- Favorites/recents if stored as JSON/config and explicitly included.
- Generated sidecar files before bridge changes.

Boundary:

- Rewind does not launch scripts.
- Rewind does not own ScriptVault metadata generation.

### Toolbox-Bridge

Toolbox-Bridge transforms Workstate-derived information into ScriptVault sidecar metadata.

Rewind can capture bridge outputs before and after transformations so changes can be compared.

Boundary:

- Toolbox-Bridge transforms.
- Rewind records before/after state.

## Possible `rewind status` Command

A small status command would help Pulse and RexOps.

```bash
rewind status
rewind status --json
```

Human output:

```text
Rewind status: ok
Latest snapshot: rw-20260619-181530-a8f31c, 2 hours ago
Snapshots: 42
Store: verified
Last restore: none
Warnings: none
```

This can be implemented after snapshot/list/show, but before deep restore work.

## Configuration

Suggested config path:

```text
$XDG_CONFIG_HOME/rewind/config.toml
```

Fallback:

```text
~/.config/rewind/config.toml
```

Example config:

```toml
[defaults]
kind = "suite"
json = false

[storage]
state_dir = "~/.local/state/rewind"

[include]
paths = [
  "$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json",
  "$XDG_DATA_HOME/workstate/feeds"
]

[exclude]
patterns = [
  "*.tmp",
  "*.lock",
  "target/",
  ".git/"
]

[restore]
allowed_roots = [
  "~/.config",
  "~/.local/share",
  "~/.local/state",
  "~/projects"
]
backup_current = true
require_apply_flag = true
```

Config loading should degrade gracefully:

- Missing config: use safe defaults.
- Invalid config: print a clear error and refuse operations that need it.
- Invalid include path: warn and continue unless explicitly required.

## Data Contracts

Rewind should eventually have schemas in the umbrella repo, likely under `contracts/`:

```text
contracts/rewind.snapshot.schema.json
contracts/rewind.diff.schema.json
contracts/rewind.restore-plan.schema.json
contracts/rewind.status.schema.json
```

These schemas let RexOps, Pulse, and future tools consume Rewind safely.

Initial contract types:

### Snapshot manifest

Describes one snapshot and all captured entries.

### Diff result

Describes comparison between two snapshots or snapshot vs current.

### Restore plan

Describes what would be restored, what safety checks passed/failed, and what command would apply it.

### Restore result

Describes what was actually changed after `--apply`.

### Status summary

Small machine-readable health summary for Pulse/RexOps.

## Suggested Module Layout

If built as a standalone Rust repo:

```text
rewind/
  Cargo.toml
  README.md
  REWIND_DESIGN.md
  src/
    main.rs
    cli.rs
    error.rs
    output.rs
    config/
      mod.rs
      model.rs
      load.rs
    model/
      mod.rs
      snapshot.rs
      entry.rs
      diff.rs
      restore.rs
      status.rs
    storage/
      mod.rs
      paths.rs
      blobs.rs
      manifest.rs
      index.rs
      lock.rs
    capture/
      mod.rs
      planner.rs
      filesystem.rs
      workstate.rs
      suite.rs
    diff/
      mod.rs
      engine.rs
      text.rs
      json.rs
    restore/
      mod.rs
      planner.rs
      safety.rs
      apply.rs
      backup.rs
    verify/
      mod.rs
      store.rs
    commands/
      mod.rs
      snapshot.rs
      list.rs
      show.rs
      diff.rs
      restore.rs
      verify.rs
      prune.rs
      status.rs
```

Rules:

- `main.rs` stays tiny.
- CLI parsing stays separate from command execution.
- Models are serializable and schema-friendly.
- Storage does not know CLI details.
- Restore safety checks are isolated and heavily tested.
- Output rendering is separate from core logic.

## Testing Strategy

Minimum useful tests:

### Unit tests

- Snapshot ID generation.
- Path normalization.
- Include/exclude matching.
- SHA-256 calculation.
- Manifest serialization/deserialization.
- Restore safety checks.
- Diff classification.

### Integration tests

- Create snapshot from temp directory.
- List snapshots.
- Show snapshot.
- Diff two snapshots.
- Restore plan without writing.
- Restore apply writes expected content.
- Pre-restore snapshot is created.
- Verify detects missing/corrupt blob.
- Prune dry-run does not delete.

### Safety tests

- Refuse `/etc/passwd` by default.
- Refuse symlink escape.
- Refuse unsupported special file.
- Refuse restore without `--apply`.
- Refuse corrupt blob.
- Refuse owner/setuid restoration in v1.

### Golden output tests

Use stable fixture output for:

- human list output
- human diff output
- JSON snapshot output
- JSON restore plan output

## Implementation Phases

### Phase 0: Design and contracts

- Add this design document.
- Decide repo location.
- Draft initial JSON schemas.
- Define default capture paths.

### Phase 1: Read-only core

Commands:

```bash
rewind snapshot --dry-run
rewind snapshot
rewind list
rewind show
rewind verify
rewind status
```

No restore yet, except maybe restore planning.

### Phase 2: Diff engine

Commands:

```bash
rewind diff previous latest
rewind diff latest current
```

Start with file/hash/text diff. Add JSON semantic diff later.

### Phase 3: Restore planner

Command:

```bash
rewind restore latest --path <path>
```

Plan only. No writes.

This phase should obsess over safety checks.

### Phase 4: Restore apply

Command:

```bash
rewind restore latest --path <path> --apply
```

Requirements:

- Pre-restore snapshot.
- Atomic write.
- Hash verification.
- Restore result record.

### Phase 5: RexOps/Pulse integration

- `rewind status --json`
- `rewind list --json`
- `rewind diff --json`
- RexOps launcher actions.
- Pulse summary check.

### Phase 6: TUI

Optional later TUI:

```bash
rewind tui
```

Possible screens:

- Timeline
- Snapshot details
- Diff viewer
- Restore planner
- Store health

TUI must not make restore feel casual. The restore screen should be deliberate, explicit, and hard to trigger by accident.

## Open Design Questions

1. **Standalone repo or umbrella crate?**
   - Rewind feels like a major suite tool, so a standalone `tom2025b/rewind` repo may fit the existing pattern.
   - If it starts inside the umbrella repo, it should still keep clean contracts and avoid importing other tools’ logic.

2. **What is the default snapshot scope?**
   - Suite state only?
   - Workstate snapshot only?
   - Workstate plus feeds?
   - Project-local contracts/configs?

3. **How much config should v1 capture?**
   - Critical configs are useful, but auto-capturing home config files can get creepy and noisy.
   - Best v1 answer may be explicit opt-in config paths.

4. **Should Rewind capture large files?**
   - Need a default size limit.
   - Suggested v1 default: warn and skip files larger than a configured limit, maybe 25 MiB.

5. **Should snapshots be encrypted?**
   - Not needed for v1 unless sensitive configs are captured.
   - If added later, use explicit user-controlled encryption, not homebrew crypto.

6. **How semantic should Workstate diffs be?**
   - v1 can do file/hash/text diff.
   - Later versions can understand Workstate snapshot sections and show domain-level changes.

7. **Should restore support directories?**
   - v1 should restore selected files.
   - Directory restore should come later with a full plan showing every file.

8. **Should restore support system paths?**
   - v1 should say no.
   - Later versions could generate manual instructions for `/etc` instead of writing there.

9. **How does Rewind avoid overlapping with Tripwire?**
   - Keep the boundary clear:
     - Tripwire detects drift.
     - Rewind records history and restores selected files.

10. **How does Rewind avoid overlapping with Workstate?**
    - Workstate owns current normalized state.
    - Rewind owns historical recorded states.

11. **Should snapshot creation be automatic during `rex run`?**
    - Maybe eventually.
    - Safer v1: RexOps can offer `create snapshot before run`, but automatic snapshots should be opt-in.

12. **What should labels look like?**
    - Free text is useful.
    - Also consider structured tags: `schema-change`, `pre-refactor`, `pre-install`, `manual`, `auto`, `pre-restore`.

13. **What is the restore UX in non-interactive mode?**
    - Likely require both `--apply` and `--yes` when stdin is not interactive.

14. **Should Rewind record Git metadata?**
    - Useful for project snapshots:
      - repo path
      - branch
      - commit hash
      - dirty status
    - But Rewind should not replace Git.

15. **Should Rewind emit events to Workstate?**
    - Maybe Rewind status can become a Workstate feed.
    - But be careful: Workstate current state and Rewind history should not create circular weirdness.

## Initial Recommendation

Build Rewind as a standalone Rust CLI with no TUI at first.

First implementation target:

```bash
rewind snapshot
rewind list
rewind show
rewind verify
rewind status --json
```

Then add:

```bash
rewind diff previous latest
rewind diff latest current
```

Only after that should restore be implemented, starting with plan-only restore.

The first real restore implementation should support one selected regular file, inside allowed roots, with mandatory pre-restore snapshot and atomic write.

That gives the suite a strong safety spine without turning Rewind into a reckless time cannon.

## Final Boundary Statement

Rewind is the suite’s memory and rollback guardrail.

Workstate tells the suite what is true now.
Tripwire tells the suite what drifted.
Pulse tells the user whether things look healthy.
RexOps gives the user a cockpit.
Rewind remembers the important before-and-after states and restores only when the user deliberately asks.

That boundary should stay sacred.
