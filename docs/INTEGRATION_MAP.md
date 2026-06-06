# Integration Map

How tools produce and consume data across the suite. Contracts live in
[`../contracts/`](../contracts/); examples in [`../examples/`](../examples/).

## Producer → consumer

| Producer | Output | Consumer | Format | Schema | Status |
|---|---|---|---|---|---|
| Bulwark | risk data | Bridge | (internal to Bridge) | — | **now** |
| Bridge | sidecar metadata | ScriptVault | YAML sidecar | — | **now** |
| ToolFoundry | `workstate-feed` | Workstate | JSON | [toolfoundry.feed](../contracts/toolfoundry.feed.schema.json) | **real (v1)** |
| Bulwark | `workstate-feed` | Workstate | JSON | [bulwark.workstate-feed.v1](../contracts/bulwark.workstate-feed.v1.json) | **real (v1)** |
| Bulwark | scan export | RexOps | JSON | [bulwark.scan](../contracts/bulwark.scan.schema.json) | provisional |
| ScriptVault | state export | RexOps | JSON | [scriptvault.export](../contracts/scriptvault.export.schema.json) | provisional |
| Workstate | snapshot | RexOps | JSON | [workstate.snapshot](../contracts/workstate.snapshot.schema.json) | provisional |
| RexOps | suite snapshot | (self/report) | JSON | [rexops.snapshot](../contracts/rexops.snapshot.schema.json) | provisional |

## Commands each producer should expose

| Producer | Command | Notes |
|---|---|---|
| ToolFoundry | `toolfoundry workstate-feed <manifest-dir> --as-of <YYYY-MM-DD>` | **Exists.** JSON-only, read-only, and exits zero even when attention is required. |
| Bulwark | `bulwark workstate-feed [PATHS] --output <path>` | **Exists.** Versioned Workstate feed; `scan --json` remains the general inventory report. |
| ScriptVault | (export subcommand TBD) | Should export scripts + favorites + recents. |
| Workstate | (snapshot subcommand TBD) | Read-only; emits versioned snapshot. |

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

## What exists now vs planned

- **Now:** Bulwark → Bridge → ScriptVault (sidecar YAML). ToolFoundry and Bulwark
  `workstate-feed` JSON contracts are real v1 producer contracts with passing
  contract tests.
- **Planned:** RexOps consuming the feeds above, in the order set by
  [ROADMAP.md](ROADMAP.md). ScriptVault/Workstate JSON exports are provisional
  until those tools ship versioned outputs.
