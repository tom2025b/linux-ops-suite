# Roadmap

Integrations land in dependency order. Read-only first; mutations last.

| Phase | Goal | Status |
|---|---|---|
| 1 | Suite docs and contracts (this repo) | **in progress** |
| 2 | Example fixtures for every contract | partial (`toolfoundry.feed` done) |
| 3 | ToolFoundry `workstate-feed` -> Workstate snapshot -> RexOps status | done locally |
| 4 | Bulwark scan contract fixture | planned |
| 5 | ScriptVault export contract fixture | planned |
| 6 | Workstate implementation + JSON snapshot export | active |
| 7 | RexOps launcher screen | planned |
| 8 | Safe confirmed actions (dry-run + audit log) | planned |

## Notes

- **Phase 8 is the only mutating phase.** Every action there requires explicit
  confirmation, a dry-run first, and an audit log. Nothing before it mutates anything.
- **scan-tools-rs: undecided / legacy.** Its inventory role overlaps Bulwark. It is
  deliberately **left out of the suite docs** until its final role is decided (fold into
  Bulwark, keep as a separate member, or retire). Revisit before Phase 4.
- Bulwark, ScriptVault, and RexOps contracts are still provisional. ToolFoundry's
  `workstate-feed` and Workstate's v3 snapshot are real versioned contracts.
