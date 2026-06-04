# Integration Map

How tools produce and consume data across the suite. Contracts live in
[`../contracts/`](../contracts/); examples in [`../examples/`](../examples/).

## Producer → consumer

| Producer | Output | Consumer | Format | Schema | Status |
|---|---|---|---|---|---|
| Bulwark | risk data | Bridge | (internal to Bridge) | — | **now** |
| Bridge | sidecar metadata | ScriptVault | YAML sidecar | — | **now** |
| ToolFoundry | `rexops-feed` | RexOps | JSON | [toolfoundry.rexops-feed](../contracts/toolfoundry.rexops-feed.schema.json) | **real (v1)** |
| Bulwark | scan export | RexOps | JSON | [bulwark.scan](../contracts/bulwark.scan.schema.json) | provisional |
| ScriptVault | state export | RexOps | JSON | [scriptvault.export](../contracts/scriptvault.export.schema.json) | provisional |
| Workstate | snapshot | RexOps | JSON | [workstate.snapshot](../contracts/workstate.snapshot.schema.json) | provisional |
| RexOps | suite snapshot | (self/report) | JSON | [rexops.snapshot](../contracts/rexops.snapshot.schema.json) | provisional |

## Commands each producer should expose

| Producer | Command | Notes |
|---|---|---|
| ToolFoundry | `toolfoundry rexops-feed <manifest-dir> --json --as-of <YYYY-MM-DD>` | **Exists.** Exits **non-zero** when attention is required (a behavioral part of the contract). |
| Bulwark | `bulwark scan --json` | Risk/inventory export. Stable versioned shape TBD. |
| ScriptVault | (export subcommand TBD) | Should export scripts + favorites + recents. |
| Workstate | (snapshot subcommand TBD) | Read-only; emits versioned snapshot. |

## Expected output paths

Paths are RexOps's read locations; producers may also print to stdout. Defaults under
`$XDG_DATA_HOME` (fallback `~/.local/share/`):

| Feed | Suggested path |
|---|---|
| ToolFoundry rexops-feed | `…/rexops/feeds/toolfoundry.rexops-feed.json` |
| Bulwark scan | `…/rexops/feeds/bulwark.scan.json` |
| ScriptVault export | `…/rexops/feeds/scriptvault.export.json` |
| Workstate snapshot | `…/rexops/feeds/workstate.snapshot.json` |

## What exists now vs planned

- **Now:** Bulwark → Bridge → ScriptVault (sidecar YAML). ToolFoundry `rexops-feed` JSON
  (real, v1, with a passing contract test).
- **Planned:** RexOps consuming the feeds above, in the order set by
  [ROADMAP.md](ROADMAP.md). Bulwark/ScriptVault/Workstate JSON exports are provisional
  until those tools ship versioned outputs.
