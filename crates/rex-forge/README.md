# rex-forge

TUI-first project scaffolder for Rust and Go. Pick a base, multi-select
components, and rex-forge writes a complete, opinionated, secure starter project
that compiles on the first try.

```sh
rex-forge new                                              # interactive TUI
rex-forge new myapp --base rust-bin --with clap,tracing    # non-interactive
rex-forge new myapp --base rust-bin --dry-run              # preview the tree
rex-forge list                                             # show bases + components
```

## Bases

- `rust-bin` — Rust binary (CLI/app)
- `rust-lib` — Rust library crate
- `go-bin` — Go binary (CLI/app)
- `go-lib` — Go library / module

Every base is minimal but secure-by-default: `#![forbid(unsafe_code)]` +
deny-by-default clippy lints + a pinned toolchain on the Rust side.

## Components (v0.1)

- **Rust:** clap, config (figment), tracing, metrics, anyhow, thiserror,
  dockerfile, ci-github
- **Go:** flag, slog, dockerfile, ci-github *(stdlib-only; cobra/viper/zap land
  in v0.2)*

Components compose: dependencies, files, and anchored code wiring are merged
deterministically, so selecting several produces one coherent project.

## How it works

- The component library is authored as plain `.toml` + `.j2` files under
  `library/` and **embedded into the binary at build time** — `rex-forge new`
  runs fully offline.
- `build.rs` validates the library (every `[[inject]]` anchor must exist in its
  base) so a broken component can't ship.
- Generation is a pure engine (resolve → render → merge → in-memory tree); a
  single writer module is the only filesystem boundary. Output is byte-for-byte
  reproducible and covered by golden snapshots + a compile gate that actually
  builds the generated projects.

See `library/SCHEMA.md` for the component authoring format.

## Flags

- `--base <name>` — non-interactive base (omit to launch the TUI)
- `--with a,b,c` — components
- `--git` — `git init` + initial commit in the new project (off by default)
- `--dry-run` — print the file tree, write nothing
- `--force` — overwrite a non-empty target directory
- `--license`, `--author` — project metadata
