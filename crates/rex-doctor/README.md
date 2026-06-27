# rex-doctor

Diagnostics for the Linux Ops Suite: verifies the installed suite is wired up end-to-end and tells you the one command that fixes each finding.

Read-only and offline. It never changes anything — it inspects your environment and the installed binaries, reports `PASS` / `WARN` / `FAIL` / `SKIP`, and exits with a structured code so it can gate scripts and shell hooks.

## Usage

```bash
# Run every check (only WARN/FAIL are shown by default).
rex-doctor

# Show PASS lines too.
rex-doctor --verbose

# Fast subset for a prompt or pre-command hook: install dirs + binaries present.
rex-doctor --quick

# Run only some checks or whole categories.
rex-doctor --only env
rex-doctor --only bin.present,bin.version

# Run everything except some checks.
rex-doctor --skip bin.version

# List every check id grouped by category.
rex-doctor --list

# Machine-readable report.
rex-doctor --json
```

## Key features

- **Two check groups.** `env.*` — PATH, the XDG data dir, writability, aliases. `bin.*` — each suite binary is present, executable, actually runs, on the expected compatibility line, and not shadowed earlier on `PATH`.
- **Actionable.** Every WARN/FAIL prints the exact command that resolves it.
- **Selectable.** `--only` / `--skip` take check ids or whole categories; `--quick` is the cheap "is it installed at all?" subset for hooks.
- **Structured exit codes.** `0` clean · `1` warn · `2` fail · `3` the doctor itself couldn't run. Use `--fail-on warn` to treat warnings as failures.
- **No dependencies on the rest of the suite running** — it checks the install, not live data.

## How it fits into the suite

rex-doctor is the suite's pre-flight check. Before `workstate` compiles the snapshot (or any tool runs), rex-doctor confirms the tools are installed, on `PATH`, and version-compatible — so a broken refresh is diagnosed as a setup problem, not a data problem. It pairs with `rex-check` (the commit/hazard gate) as the suite's health and safety tooling.
