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
| **Workstate**   | Read-only project/repo health dashboard           | Architecture |
| **RexOps**      | Operations cockpit + suite launcher               | In progress  |

## How They Work Together

- Data flows **one way** through files (mostly JSON).
- No tool imports code from another tool.
- **RexOps** is the front door and only consumer — it reads summaries and lets you launch the other tools.
- Most integrations are still being built. The main real connection right now is **Bulwark → Toolbox Bridge → ScriptVault**.

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
- [Workstate](https://github.com/tom2025b/workstate) — Health dashboard
- [RexOps](https://github.com/tom2025b/rexops) — Suite cockpit

---

Built for personal use. Keep it simple.
