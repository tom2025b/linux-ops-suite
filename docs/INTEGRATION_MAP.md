# Integration Map

How tools produce and consume data across the suite. Contracts live in
[`../contracts/`](../contracts/); examples in [`../examples/`](../examples/).

## Producer → consumer

| Producer | Output | Consumer | Format | Schema | Status |
|---|---|---|---|---|---|
| Workstate | snapshot | Toolbox-Bridge | JSON | [workstate.snapshot](../contracts/workstate.snapshot.schema.json) | **now** |
| Toolbox-Bridge | `workstate-feed` (sidecar metadata) | ScriptVault | JSON | [toolbox-bridge.workstate-feed.v1](../contracts/toolbox-bridge.workstate-feed.v1.schema.json) | **real (v1)** |
| ToolFoundry | `workstate-feed` | Workstate | JSON | [toolfoundry.workstate-feed.v1](../contracts/toolfoundry.workstate-feed.v1.schema.json) | **real (v1)** |
| Bulwark | `workstate-feed` | Workstate | JSON | [bulwark.workstate-feed.v1](../contracts/bulwark.workstate-feed.v1.schema.json) | **real (v1)** |
| Bulwark | scan export | RexOps | JSON | [bulwark.scan](../contracts/bulwark.scan.schema.json) | provisional |
| ScriptVault | state export | RexOps | JSON | [scriptvault.export](../contracts/scriptvault.export.schema.json) | provisional |
| Workstate | snapshot | RexOps | JSON | [workstate.snapshot](../contracts/workstate.snapshot.schema.json) | provisional |
| Proto | session record | RexOps | JSON | [proto.session](../contracts/proto.session.schema.json) | **real (v1)** |
| Proto | `workstate-feed` | Workstate | JSON | [proto.workstate-feed.v1](../contracts/proto.workstate-feed.v1.schema.json) | **real (v1)** |
| RexOps | suite snapshot | (self/report) | JSON | [rexops.snapshot](../contracts/rexops.snapshot.schema.json) | provisional |

## Commands each producer should expose

| Producer | Command | Notes |
|---|---|---|
| ToolFoundry | `toolfoundry workstate-feed <manifest-dir> --as-of <YYYY-MM-DD>` | **Exists.** JSON-only, read-only, and exits zero even when attention is required. |
| Bulwark | `bulwark workstate-feed [PATHS] --output <path>` | **Exists.** Versioned Workstate feed; `scan --json` remains the general inventory report. |
| Toolbox-Bridge | `toolbox-bridge [--snapshot <path>] [--output <path>] [--dry-run]` | **Exists.** Reads the Workstate snapshot's findings section (never Bulwark directly), converts to ScriptVault sidecar metadata, writes the versioned feed. `--dry-run` previews to stdout. |
| ScriptVault | (export subcommand TBD) | Should export scripts + favorites + recents. |
| Workstate | (snapshot subcommand TBD) | Read-only; emits versioned snapshot. |
| Proto | `proto run <id>` | **Exists.** Walks a protocol interactively and writes one session JSON per run. Read-only — it records outcomes, it never executes the steps. `proto list` / `proto validate` are the non-interactive companions. |
| Proto | `proto feed` | **Exists.** Regenerates the rolling `workstate-feed` (a summary of recent sessions) into `…/workstate/feeds/proto.json`. `proto run` writes it automatically after each run; `proto feed` is the manual/cron refresh. `--no-feed` suppresses the automatic write. |

## Launching from RexOps

RexOps's Launcher screen runs a specialist tool in the foreground. It resolves
the command by tool **id**: it shells out to `which <id>` (a non-interactive
subprocess) and falls back to a per-adapter `binary` path in RexOps config. It
then runs the bare command, hands over the real terminal, and reports the child's
**exit code** (0 = success).

Two consequences for a tool that wants to be launchable:

1. **A real binary named `<id>` must be on `$PATH`.** A shell *alias* does NOT
   work — `which` runs in a subprocess where interactive aliases don't exist.
   Install with `cargo install --path .` (lands in `~/.cargo/bin/<id>`), like
   ScriptVault. (A `~/bin/r-<id>` wrapper or shell alias is a convenience for the
   human, not a substitute for the PATH binary RexOps needs.)
2. **Bare `<id>` should DO something useful**, since RexOps launches it with no
   arguments. The tool should drop the operator into its primary interface.

| Tool | Launch id | Bare-invocation behaviour |
|---|---|---|
| Bulwark | `bulwark` | Opens its TUI. |
| Proto | `proto` | On a TTY, shows an interactive protocol **picker** → run; non-TTY prints help (stays scriptable). Installed via `cargo install --path .`. **Registered in the RexOps launcher catalog.** |
| Workstate / Scripts / Tools | (feed-only) | No executable; RexOps shows "no launch command yet". |

## Expected output paths

Paths are RexOps's read locations; producers may also print to stdout. Defaults under
`$XDG_DATA_HOME` (fallback `~/.local/share/`):

| Feed | Suggested path |
|---|---|
| ToolFoundry workstate-feed | `…/workstate/feeds/toolfoundry.json` |
| Bulwark workstate-feed | `…/workstate/feeds/bulwark.json` |
| Toolbox-Bridge workstate-feed | `…/workstate/feeds/toolbox-bridge.json` (sidecar metadata for ScriptVault, derived from the snapshot's findings) |
| Bulwark scan | `…/rexops/feeds/bulwark.scan.json` |
| ScriptVault export | `…/rexops/feeds/scriptvault.export.json` |
| Workstate snapshot | `…/rexops/feeds/workstate.snapshot.json` |
| Proto sessions | `…/proto/sessions/<protocol-id>-<timestamp>.json` (one file per run, not a single rolling feed) |
| Proto workstate-feed | `…/workstate/feeds/proto.json` (one rolling file summarizing recent sessions, alongside bulwark.json / toolfoundry.json) |

## What exists now vs planned

- **Now:** Bulwark → Workstate → Toolbox-Bridge → sidecar feed for ScriptVault —
  the bridge is pure Rust ([`crates/toolbox-bridge`](../crates/toolbox-bridge)) and
  talks ONLY through Workstate artifacts (it replaced the retired Python bridge,
  which invoked Bulwark and wrote sidecar YAML directly). ToolFoundry and Bulwark
  `workstate-feed` JSON contracts are real v1 producer contracts with passing
  contract tests. Proto's `session` JSON is a real v1 producer contract
  ([example](../examples/proto.session.example.json)); RexOps consumption is planned.
  Proto also emits a real v1 `workstate-feed`
  ([example](../examples/proto.workstate-feed.example.json)) into
  `…/workstate/feeds/proto.json`, the same envelope as Bulwark/ToolFoundry, so
  Workstate ingests recent Proto runs the same way it ingests its other feeds.
- **Planned:** RexOps consuming the feeds above, in the order set by
  [ROADMAP.md](ROADMAP.md). ScriptVault/Workstate JSON exports are provisional
  until those tools ship versioned outputs. ScriptVault reading the
  Toolbox-Bridge sidecar feed (merging it with its usual sidecar-wins rules) is
  the consumption half of the bridge contract.
