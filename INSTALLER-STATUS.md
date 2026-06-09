# Installer Status

Focused status for the Linux Ops Suite one-command installer (`install.sh`).

## What it is

`install.sh` at the umbrella root is an **orchestrator** that rebuilds the whole
suite on a fresh Linux box. The umbrella is a "contracts HQ", not a monorepo, so
the installer clones/updates each tool's own repo and installs it — it does **not**
build one workspace.

Per tool: **clone or `git pull`** → **`cargo build --release`** → **copy
`target/release/<binary>` to `~/.local/bin/`**. It also installs the `rex`
launcher and writes a `r-<tool>` wrapper + `~/.rust_aliases.sh` alias per tool.

- Idempotent (skips what's built unless `--force`).
- Never edits the shell rc — prints the PATH/source lines to add.
- Flags: `--force`, `--local`, `--skip-aliases`, `--dry-run`, `--only a,b`, `--help`.

## Current state

| Item | State |
|---|---|
| `install.sh` (build-and-copy method) | ✅ committed |
| README "## Installation" section | ✅ committed |
| Branch `fix/installer-build-release` | ✅ pushed to origin |
| Validation | ✅ `bash -n` OK · `shellcheck` 0.9.0 clean · `--dry-run` exercises full flow |
| Real test | ✅ wrapper/alias generation tested in a sandbox (idempotent) |
| Merged to `main` | ✅ PR #4 merged 2026-06-09 (`fix/installer-build-release` → `main`) |
| First real end-to-end run | 🔄 in progress (full build+copy of all tools) |

### Branch lineage

```
main
 └─ a66d2eb  feat: one-command installer (#3)        [the ORIGINAL, merged — used cargo install]
fix/installer-build-release  (pushed, not merged)
 ├─ 6a6430f  fix(installer): cargo build --release + copy   [supersedes the cargo-install method]
 └─ f092df0  docs: README installation section
```

`feat/installer-docs` is now fully contained in `fix/installer-build-release`
(its single commit was fast-forward-merged in) and can be deleted.

> Note: `main` already has an earlier installer (`a66d2eb`) that used
> `cargo install --path`. The branch above replaces that method with
> `cargo build --release` + copy. Landing the branch supersedes it.

## What still needs to be done

1. ~~**Open a PR** for `fix/installer-build-release` → `main` and merge it.~~
   ✅ Done — PR #4 merged 2026-06-09; build-and-copy installer + README are now
   the canonical version on `main`.
2. ~~**Delete** the redundant installer/feat branches.~~ ✅ Done — redundant
   remote branches pruned; stale local branches deleted.
3. **First real end-to-end run** — `./install.sh` 🔄 in progress (this run is the
   first full build+copy of all six Rust tools + toolbox-bridge for real; prior
   to this only `--dry-run` + a sandboxed sub-step had been executed).
4. **Confirm binary names** hold after any tool restructure — the `repo:binary`
   map in `install.sh` is correct today (notably `rexops`' binary comes from the
   `rexops-cli` package but is named `rexops`). Verify against this real run.
5. **Optional:** richer `r-<tool>` aliases if `alias r-foo='foo'` isn't enough.
