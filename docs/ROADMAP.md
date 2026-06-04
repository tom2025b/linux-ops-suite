# Roadmap

Integrations land in dependency order. Read-only first; mutations last.

| Phase | Goal | Status |
|---|---|---|
| 1 | Suite docs and contracts (this repo) | **in progress** |
| 2 | Example fixtures for every contract | partial (rexops-feed done) |
| 3 | RexOps consumes ToolFoundry `rexops-feed` | next |
| 4 | RexOps consumes Bulwark scan JSON | planned |
| 5 | RexOps consumes ScriptVault export JSON | planned |
| 6 | Workstate implementation + JSON snapshot export | planned |
| 7 | RexOps launcher screen | planned |
| 8 | Safe confirmed actions (dry-run + audit log) | planned |

## Notes

- **Phase 8 is the only mutating phase.** Every action there requires explicit
  confirmation, a dry-run first, and an audit log. Nothing before it mutates anything.
- **scan-tools-rs: undecided / legacy.** Its inventory role overlaps Bulwark. It is
  deliberately **left out of the suite docs** until its final role is decided (fold into
  Bulwark, keep as a separate member, or retire). Revisit before Phase 4.
- Contracts for Bulwark, ScriptVault, Workstate, and RexOps are **provisional stubs**
  until each tool ships a real versioned export; the stubs only fix the envelope fields.
