# Linux Ops Suite

A personal toolkit of focused, single-purpose Linux tools that work together through clean file-based contracts.

This repository is the **contract and index headquarters** for the suite. Each tool lives in its own repo. This repo defines how they should talk to each other.

## The Tools

| Tool            | Role                                              | Status       |
|-----------------|---------------------------------------------------|--------------|
| **Bulwark**     | Read-only scanner + risk classifier               | Active       |
| **ScriptVault** | Fast TUI script launcher + favorites & recents    | Active       |
| **Toolbox Bridge** | Converts Bulwark risk data into ScriptVault sidecars | Active    |
| **ToolFoundry** | Tool lifecycle, ownership, and health             | Active       |
| **Workstate**   | Read-only state compiler — emits the v3 snapshot  | Active       |
| **Proto**       | Guided protocol / checklist runner — emits session records | Active       |
| **RexOps**      | Operations cockpit + suite launcher               | Active       |

## How They Work Together

- Data flows **one way** through files (mostly JSON).
- No tool imports code from another tool.
- **RexOps** is the front door and only consumer — it reads summaries and lets you launch the other tools.
- **ToolFoundry** emits `toolfoundry workstate-feed`; the shape is pinned by `contracts/toolfoundry.workstate-feed.v1.schema.json`.
- **Workstate** compiles the other tools' feeds into one versioned `snapshot.json` (schema v3) that **RexOps** consumes as its source of truth. The shape is pinned by `contracts/workstate.snapshot.schema.json` and validated in both repos' CI.
- Also live: **Bulwark → Toolbox Bridge → ScriptVault** for risk sidecars.
- **Proto** reads human-authored protocols (YAML checklists) and emits one `session` JSON per run, pinned by `contracts/proto.session.schema.json`. It is read-only — it guides and records, it never acts on your behalf.

## Design Principles

- One job per tool
- File-based contracts over shared code
- Read-only by default
- Low-resource friendly (Linux Mint)
- Rust-first where it makes sense

## Repositories

- [Bulwark](https://github.com/tom2025b/bulwark) — Scanner & risk
- [ScriptVault](https://github.com/tom2025b/scriptvault) — Script launcher
- [Toolbox Bridge](https://github.com/tom2025b/toolbox-bridge) — Bulwark → ScriptVault connector
- [ToolFoundry](https://github.com/tom2025b/toolfoundry) — Lifecycle & ownership
- [Workstate](https://github.com/tom2025b/workstate) — State compiler
- [Proto](https://github.com/tom2025b/proto) — Guided protocol / checklist runner
- [RexOps](https://github.com/tom2025b/rexops) — Suite cockpit

---

Built for personal use. Keep it simple.
