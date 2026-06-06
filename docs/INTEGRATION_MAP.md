# Integration Map

How tools produce and consume data across the suite. Contracts live in
[`../contracts/`](../contracts/); examples in [`../examples/`](../examples/).

## Producer → consumer

| Producer | Output | Consumer | Format | Schema | Status |
|---|---|---|---|---|---|
| Bulwark | risk data | Bridge | (internal to Bridge) | — | **now** |
| Bridge | sidecar metadata | ScriptVault | YAML sidecar | — | **now** |
| ToolFoundry | `workstate-feed` | Workstate | JSON | [toolfoundry.feed](../contracts/toolfoundry.feed.schema.json) | **real (v1)** |
| Bulwark | `workstate-feed` | Workstate | JSON | [bulwark.workstate-feed.v1](../contracts/bulwark.workstate-feed.v1.schema.json) | **real (v1)** |
| Bulwark | scan export | RexOps | JSON | [bulwark.scan](../contracts/bulwark.scan.schema.json) | provisional |
| ScriptVault | state export | RexOps | JSON | [scriptvault.export](../contracts/scriptvault.export.schema.json) | provisional |
| Workstate | snapshot | RexOps | JSON | [workstate.snapshot](../contracts/workstate.snapshot.schema.json) | provisional |
| Proto | session record | RexOps | JSON | [proto.session](../contracts/proto.session.schema.json) | **real (v1)** |
| RexOps | suite snapshot | (self/report) | JSON | [rexops.snapshot](../contracts/rexops.snapshot.schema.json) | provisional |

## Commands each producer should expose

| Producer | Command | Notes |
|---|---|---|
| ToolFoundry | `toolfoundry workstate-feed <manifest-dir> --as-of <YYYY-MM-DD>` | **Exists.** JSON-only, read-only, and exits zero even when attention is required. |
| Bulwark | `bulwark workstate-feed [PATHS] --output <path>` | **Exists.** Versioned Workstate feed; `scan --json` remains the general inventory report. |
| ScriptVault | (export subcommand TBD) | Should export scripts + favorites + recents. |
| Workstate | (snapshot subcommand TBD) | Read-only; emits versioned snapshot. |
| Proto | `proto run <id>` | **Exists.** Walks a protocol interactively and writes one session JSON per run. Read-only — it records outcomes, it never executes the steps. `proto list` / `proto validate` are the non-interactive companions. |

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
| Bulwark scan | `…/rexops/feeds/bulwark.scan.json` |
| ScriptVault export | `…/rexops/feeds/scriptvault.export.json` |
| Workstate snapshot | `…/rexops/feeds/workstate.snapshot.json` |
| Proto sessions | `…/proto/sessions/<protocol-id>-<timestamp>.json` (one file per run, not a single rolling feed) |

## What exists now vs planned

- **Now:** Bulwark → Bridge → ScriptVault (sidecar YAML). ToolFoundry and Bulwark
  `workstate-feed` JSON contracts are real v1 producer contracts with passing
  contract tests. Proto's `session` JSON is a real v1 producer contract
  ([example](../examples/proto.session.example.json)); RexOps consumption is planned.
- **Planned:** RexOps consuming the feeds above, in the order set by
  [ROADMAP.md](ROADMAP.md). ScriptVault/Workstate JSON exports are provisional
  until those tools ship versioned outputs.
