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
| Merged to `main` | ❌ not yet (PR open below) |

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

1. **Open a PR** for `fix/installer-build-release` → `main` and merge it (this
   makes the build-and-copy installer + README the canonical version on `main`).
2. **Delete** the now-redundant `feat/installer-docs` branch (local + remote if
   pushed).
3. **First real end-to-end run** on a clean-ish environment: `./install.sh`
   (currently only `--dry-run` + a sandboxed sub-step have been executed; the
   full build+copy of all six Rust tools has not been run for real yet).
4. **Confirm binary names** hold after any tool restructure — the `repo:binary`
   map in `install.sh` is correct today (notably `rexops`' binary comes from the
   `rexops-cli` package but is named `rexops`).
5. **Optional:** richer `r-<tool>` aliases if `alias r-foo='foo'` isn't enough.
