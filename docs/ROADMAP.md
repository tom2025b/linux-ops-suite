# Roadmap

Integrations land in dependency order. Read-only first; mutations last.

| Phase | Goal | Status |
|---|---|---|
| 1 | Suite docs and contracts (this repo) | **done** |
| 2 | Example fixtures for every contract | **done** (9 of 9, CI-validated) |
| 3 | ToolFoundry `workstate-feed` -> Workstate snapshot -> RexOps status | **done** |
| 4 | Bulwark scan contract fixture | **done** |
| 5 | ScriptVault export contract fixture | **done** |
| 6 | Workstate implementation + JSON snapshot export | **done** (snapshot v3) |
| 7 | RexOps launcher screen | **done** (in the `rexops` repo) |
| 8 | Safe confirmed actions (dry-run + audit log) | planned |

The end-to-end read-only chain is live:
`Bulwark → Workstate → Toolbox-Bridge → ScriptVault`, with ToolFoundry and Proto
also feeding Workstate, and RexOps consuming the compiled snapshot. The only
remaining roadmap phase is Phase 8 (mutating actions).

## Notes

- **Phase 8 is the only mutating phase.** Every action there requires explicit
  confirmation, a dry-run first, and an audit log. Nothing before it mutates anything.
- **scan-tools-rs: retired / not part of the suite.** Its inventory role overlapped
  Bulwark; it was never folded in and is not a workspace member or sibling repo. The
  suite's own repo-inventory helper is now [`rex-check`](../crates/rex-check) (git
  status + LOC across the suite repos), a different job from Bulwark's risk scan.
- Bulwark, ScriptVault, and RexOps export contracts remain **provisional** until
  those tools pin versioned outputs; ToolFoundry/Bulwark/Proto `workstate-feed`s,
  Workstate's v3 snapshot, and the Toolbox-Bridge sidecar feed are **real versioned
  contracts** with passing contract tests and CI-validated examples.
