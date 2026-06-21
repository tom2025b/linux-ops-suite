# rex-forge — Design Document

**Status:** Approved (design + open questions locked 2026-06-20)
**Author:** tomb / Claude
**Crate:** `crates/rex-forge` (13th member of the `linux-ops-suite` Cargo workspace)

---

## Locked decisions (open questions resolved)

| # | Question | Decision |
|---|----------|----------|
| Q1 | Where does rex-forge live for v0.1? | **Sibling in the suite umbrella** as `crates/rex-forge`. Component library lives **in-tree** for v0.1 (`crates/rex-forge/library/`), extractable to a separate repo later. |
| Q2 | Binary name / wrapper? | Binary is **`rex-forge`**, installed as a **bare binary** on PATH. No `r-*` wrapper, no alias (per standing user rule). |
| Q3 | Go base depth | **Flat `main.go`** skeleton for v0.1. `/cmd` + `/internal` layout deferred to a later selectable component. |
| Q4 | Git init on generate? | **Off by default**, `--git` opt-in (and a toggle on the confirm step). Writer stays the single pure I/O boundary; git is a clearly-bounded opt-in second side effect. |
| Q5 | Security default depth | **Baked into bases:** deny-by-default clippy lints, `#![forbid(unsafe_code)]` on libs, pinned toolchain (`rust-toolchain.toml` / Go version). **Opt-in components:** Dockerfile, CI, govulncheck. Bases stay minimal but secure-by-default. |
| Q6 | License / author metadata | Optional, **skippable "details" step** in the TUI (project name shown, license picker, author). Non-interactive defaults: license `MIT`, author from `$CARGO_*`/git config when available, else placeholder. |
| Q7 | v0.1 component menu | Rust: `clap`, `config`, `tracing`, `metrics`, `anyhow`, `thiserror`, `dockerfile`, `ci-github`. Go (revised 2026-06-20): **stdlib-only** — `flag`, `slog`, `dockerfile`, `ci-github`. `cobra`/`viper`/`zap` **deferred to v0.2**: they need external `require` lines in `go.mod`, and the engine holds `go.mod` aside (no Go dep-merge in v0.1), so wiring them would either break the offline/CI compile-gate (module downloads) or require an engine change out of scope for v0.1. Stdlib `flag`/`slog` compile offline and keep the compile-gate green. |

---

## 1. Vision & Goals

**rex-forge** is a TUI-first project scaffolder for Rust and Go. Run `rex-forge new`,
a fast and beautiful terminal UI opens, you pick a base project type, multi-select
the components you want with arrow keys + Enter, and rex-forge writes a complete,
opinionated, secure starter project to disk — ready to build and run.

Guiding principle: **the happy path is instant, offline, and opinionated.** No
network round-trips in the hot path, no decision paralysis, no half-configured
output. You get a project that compiles, lints clean, has CI, and follows current
best practices on the first try.

### Goals
- `rex-forge new` → working, buildable project in under 60 seconds of interaction.
- Zero network dependency for the core generate flow (library ships *in* the binary).
- Generated projects are correct by construction: they compile, `clippy`/`vet` clean,
  and pass their own generated CI.
- Components compose cleanly — `tracing` + `clap` + `dockerfile` produce one coherent
  project, not three stapled-together fragments.
- The component library is authored as plain files, so contribution is a normal PR,
  not a plugin SDK.
- Visually consistent with the rest of the suite via **suite-ui** (ratatui + crossterm).

### Non-goals (v1)
- Not a build tool, package manager, or task runner. Scaffold, then get out of the way.
- Not a live template marketplace with runtime fetching (later, additive layer).
- Not a general code generator — Rust and Go only, four base types.
- Not a migration/refactor tool — `new` creates fresh trees only.

### Anti-goals (deliberately refused)
- No runtime GitHub/network calls, auth, or rate limits on the generate path.
- No deep configurability matrix. Opinionated defaults beat 40 flags.

---

## 2. Core Features (v1)

- **F1 — `rex-forge new` interactive TUI.** Base selection → optional details →
  component multi-select → confirm → generate. Fully keyboard-driven.
- **F2 — Four base templates.** `rust-bin`, `rust-lib`, `go-bin`, `go-lib`. Each is a
  minimal, secure, idiomatic skeleton that compiles on its own with zero components.
- **F3 — Composable component library (bundled).** Curated components per language,
  embedded in the binary at build time. Each injects files, dependencies, and wiring
  into the base, and declares applicable bases + conflicts + requires.
- **F4 — Deterministic generation engine.** Given `(base, [components], name)`, output
  is byte-for-byte reproducible. No timestamps or random ordering leak into output.
- **F5 — Safety rails.** Refuses to write into a non-empty dir without `--force`.
  Validates project name (valid crate/module identifier). `--dry-run` available.
- **F6 — Non-interactive mode.** `rex-forge new <name> --base rust-bin --with clap,tracing,ci-github`.
  Same engine, no TUI. Makes the tool testable from day one.
- **F7 — `rex-forge list`.** Print bases and components (descriptions, applicable bases,
  conflicts) as plain text. Discoverability + shell docs.
- **F8 — Post-generate summary.** Print a tree of what was created, the exact next
  commands, and any TODO notes a component left behind.

**Deferred to v2 (named, not built):** `rex-forge update` (refresh bundled index from
GitHub), live/cached remote components, custom user component dirs, presets/bundles,
version-pinning UI, `/cmd`+`/internal` Go layout component, **Go dependency components
(`cobra`/`viper`/`zap`) + `go.mod` `require`-injection** (v0.1 Go is stdlib-only to keep
the offline compile-gate intact).

---

## 3. User Experience (TUI Flow)

Launch: `rex-forge new myapp` (name optional; if omitted, a name field appears as step 0).

Global keys: `↑/↓` or `j/k` move · `Space`/`Enter` toggle · `Tab`/`→` advance ·
`Shift+Tab`/`←` back · `/` filter · `?` help overlay · `q`/`Esc` quit (confirm if mid-flow).

### Step 1 — Choose a base (single-select)
```
+- rex-forge - new project ----------------------- myapp --+
|  Choose a base project type:                             |
|   > (o) rust-bin    Rust binary (CLI/app)                |
|     ( ) rust-lib    Rust library crate                   |
|     ( ) go-bin      Go binary (CLI/app)                  |
|     ( ) go-lib      Go library / module                  |
|  A minimal, secure, idiomatic skeleton. Compiles as-is.  |
+----------------------------------------------------------+
|  up/dn move   Enter select   Tab next   ? help   q quit  |
+----------------------------------------------------------+
```

### Step 1.5 — Details (optional, skippable)
Project name (pre-filled), license picker (MIT / Apache-2.0 / dual / none), author.
`Tab` skips with defaults.

### Step 2 — Select components (multi-select, grouped; only base-applicable shown)
```
+- rex-forge - components ---------------------- rust-bin -+
|  /clap_                                    3 selected     |
|  CLI & ARGS                                              |
|   > [x] clap            Arg parsing (derive API)         |
|     [ ] config          Layered config (figment)         |
|  OBSERVABILITY                                           |
|     [x] tracing         Structured logging + spans       |
|     [ ] metrics         Prometheus endpoint              |
|  ERRORS                                                  |
|     [x] anyhow          App-level error handling         |
|     [ ] thiserror       Typed library errors  (lib-only) |
|  OPS                                                     |
|     [ ] dockerfile      Multi-stage, distroless image    |
|     [ ] ci-github       Actions: build/test/clippy       |
|  -- Selected: clap, tracing, anyhow -------------------- |
+----------------------------------------------------------+
|  Space toggle   / filter   Tab next   Shift+Tab back     |
+----------------------------------------------------------+
```
Behaviors:
- Typing after `/` filters live across all categories; `Esc` clears.
- A conflicting component shows dimmed with a reason (`conflicts: anyhow`) and refuses
  toggle-on with a one-line status flash.
- A component that `requires` another auto-pulls it and notes it
  (`+config (required by metrics)`); deselecting the dependent releases it.
- Footer always shows the current selection set — no surprises at confirm.

### Step 3 — Confirm & generate (last keystroke before disk is touched)
```
+- rex-forge - confirm ------------------------------------+
|  Create ./myapp                                          |
|  base: rust-bin   components: clap, tracing, anyhow      |
|  [ ] git init                                            |
|  myapp/                                                  |
|   |- Cargo.toml            (clap, tracing, anyhow added) |
|   |- src/main.rs           (clap parser + tracing init)  |
|   |- src/cli.rs            (clap)                        |
|   |- rust-toolchain.toml                                 |
|   |- .gitignore                                          |
|   `- README.md                                          |
|  6 files · 0 conflicts · target dir is empty            |
|   [ Generate ]      Esc to go back                      |
+----------------------------------------------------------+
```

### Step 4 — Result (drops back to shell with a printed summary)
```
Created ./myapp  (6 files, base rust-bin)
  components: clap, tracing, anyhow
  next:
    cd myapp
    cargo run -- --help
  notes:
    • src/cli.rs has a TODO: add your subcommands
```
If the target dir is non-empty, Step 3 blocks with
`./myapp is not empty — re-run with --force to overwrite` and writes nothing.

---

## 4. Architecture

Three layers, each independently testable. The TUI is a thin front-end over a **pure
engine**; the engine never talks to a terminal or a network.

```
+----------------------------------------------------------+
|  CLI (clap)   new · list · --with/--base · --dry-run     |
+--------------+----------------------------+--------------+
               | interactive                | non-interactive
      +--------v--------+                   |
      |  TUI (suite-ui) |   selections      |  selections
      |  ratatui/cross  |------------------>|<-------------
      +-----------------+                   |
                          +-----------------v-------------+
                          |        ENGINE (pure)          |
                          |  resolve -> render -> plan    |
                          |  • registry (embedded)        |
                          |  • dependency/conflict solver |
                          |  • template renderer (minijinja)
                          |  • merge layer (deps, files)  |
                          |  -> FileTree (in memory)      |
                          +-----------------+-------------+
                                            | FileTree
                          +-----------------v-------------+
                          |   WRITER  (only I/O sink)     |
                          |  empty-dir check · write/force|
                          |  · dry-run prints tree only   |
                          |  · optional git init (--git)  |
                          +-------------------------------+
```

### Engine pipeline (pure functions, no I/O)
1. **Resolve.** Take `(base, requested_components)`. Validate each applies to the base.
   Run the dependency/conflict solver: auto-add `requires`, reject `conflicts` pairs,
   produce a final **ordered** component set or a structured error. *Most logic lives
   here; gets the most tests.*
2. **Render.** Each component owns template files (minijinja) + a manifest. Templates
   render against a shared context (`project_name`, `base`, selected-component flags,
   derived identifiers, license, author). Deterministic.
3. **Merge / plan.** Components contribute fragments that must merge:
   - **Manifest deps** (`Cargo.toml [dependencies]` / `go.mod` requires) merged and
     sorted deterministically.
   - **Anchored injections** into shared files applied at named anchors
     (`// rex:imports`, `// rex:init`, `// rex:main`), not by blind concatenation.
   - Result is an in-memory **FileTree** (`path -> bytes`) + a plan summary. Nothing on
     disk yet.
4. **Write.** The single I/O boundary. Checks target dir is empty (or `--force`), writes
   the FileTree, or — in `--dry-run` — prints the tree and exits. Optional `git init` +
   initial commit only when `--git`. The only module that touches the filesystem.

### Bundled library build path (GitHub-authored → binary; the A1 model)
- Library authored as plain files under `crates/rex-forge/library/` (in-tree for v0.1).
- At **rex-forge build time**, a `build.rs` step validates every component manifest +
  template (schema check, anchor references resolve, no unknown bases) and embeds the
  whole library into the binary via `include_dir`.
- Generated registry + templates are compiled in → **offline, reproducible**, nothing
  to install or fetch at runtime.
- **CI golden tests** build rex-forge and run a base × component matrix; nothing merges
  if generated output doesn't compile + lint clean. Bad components can't ship.
- A future `rex-forge update` (v2) would fetch a newer pre-validated index — v1 never
  needs the network.

### suite-ui integration
The TUI is built entirely from suite-ui widgets (lists, multi-select, header/footer
chrome, key-hint bar, status flash) on ratatui + crossterm, matching pulse and the rest
of the suite. rex-forge uses the suite **`Tui` guard** for raw-mode/alt-screen setup and
panic-safe teardown — no bespoke terminal handling. Missing widgets (e.g. grouped
multi-select) are added **to suite-ui** via the normal PR-then-bump-pin flow, not forked
locally. Since rex-forge is an in-workspace member depending on `suite-ui` by path, a
suite-ui change lands in the same workspace and no external rev-pin bump is needed.

### Error handling
Engine returns typed errors (`ResolveError::{Conflict, UnknownComponent, BaseMismatch}`,
`WriteError::TargetNotEmpty`, …). TUI renders them as inline status flashes and never
crashes the loop; CLI maps them to non-zero exit codes with a one-line message. The
writer builds the full FileTree before writing anything, so a resolve/render failure
leaves disk untouched.

---

## 5. Component Library Structure (in-tree for v0.1)

Location: `crates/rex-forge/library/`. Plain files, reviewable as PRs, no SDK.

```
crates/rex-forge/library/
|- SCHEMA.md                     # component.toml spec + anchor reference
|- bases/
|   |- rust-bin/
|   |   |- base.toml             # name, language, description, anchors
|   |   `- files/                # skeleton, with rendered anchors
|   |       |- Cargo.toml.j2
|   |       |- src/main.rs.j2    # contains // rex:imports, // rex:init ...
|   |       |- rust-toolchain.toml
|   |       |- .gitignore
|   |       `- README.md.j2
|   |- rust-lib/...
|   |- go-bin/...
|   `- go-lib/...
`- components/
    |- rust/
    |   |- clap/  { component.toml, files/ }
    |   |- tracing/ ...  anyhow/ ...  thiserror/ ...  config/ ...  metrics/ ...
    |   |- dockerfile/ ...  ci-github/ ...
    `- go/                          # v0.1: stdlib-only (cobra/viper/zap -> v0.2)
        |- flag/ ...  slog/ ...
        |- dockerfile/ ...  ci-github/ ...
```

### `component.toml` (the contract)
```toml
name        = "tracing"
language    = "rust"
category    = "Observability"
summary     = "Structured logging + spans (tracing + tracing-subscriber)"
bases       = ["rust-bin", "rust-lib"]   # where it applies
requires    = []                          # auto-added components
conflicts   = []                          # mutually exclusive components

[dependencies]                            # merged into Cargo.toml
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[[files]]                                 # standalone files to add
path = "src/telemetry.rs"
template = "files/telemetry.rs.j2"

[[inject]]                                # anchored fragments
target = "src/main.rs"
anchor = "rex:imports"
template = "files/inject/imports.rs.j2"

[[inject]]
target = "src/main.rs"
anchor = "rex:init"
template = "files/inject/init.rs.j2"

[[note]]                                  # shown on the result screen
text = "Logging level is read from RUST_LOG (defaults to info)."
```

### Conventions
- One directory per component; everything it does is declared in its `component.toml`.
  No cross-component file edits.
- Templates use `.j2` and render against a fixed context documented in `SCHEMA.md`.
- Anchors are a **closed, documented set per base**. An unknown anchor reference fails
  the build.
- CI builds rex-forge and runs golden tests over a base × component matrix — nothing
  merges if output doesn't compile and lint clean.

---

## 6. Technical Decisions

| Concern | Decision | Why |
|---------|----------|-----|
| Language | Rust | Suite-native; single static binary; strong template/embed ecosystem. |
| TUI stack | suite-ui (ratatui + crossterm) | Consistency with pulse & suite; `Tui` guard = raw-mode/panic safe. |
| CLI parsing | clap (derive) | Standard; powers `new` and the non-interactive flags. |
| Templating | minijinja | Jinja2 syntax, pure-Rust, fast, no sandbox-escape surprises, great for `.j2` fragments. |
| Library embedding | `include_dir` via `build.rs` | Whole library compiled into the binary → offline, reproducible. |
| Manifest format | TOML (serde) | Matches Cargo; easy to author/validate; humans read it. |
| Dep/file merging | Custom, deterministic | Anchored injection + sorted dep merge → predictable golden output. |
| Fs writing | Single writer module (`std::fs`) | One I/O boundary → engine stays pure and snapshot-testable. |
| Testing | Golden/snapshot (`insta`) + compile gate | Generate → compare tree; plus a matrix that actually `cargo build` / `go build`s output. |
| Error model | `thiserror` typed enums | Map cleanly to TUI flashes and CLI exit codes. |

**Key rationale — anchored injection over string-append.** The hard part of a composable
scaffolder is that several components must edit the *same* `main.rs`/`go.mod` coherently.
Blind concatenation produces garbage ordering and duplicate imports. Named anchors in base
files, with components contributing fragments per anchor, give deterministic, readable,
conflict-free merges — and make golden tests meaningful.

**Key rationale — bundled, not live.** Embedding the library trades "always latest" for
"always works, instantly, offline." The flagship promise is a beautiful, fast TUI; a
network call (auth, rate limits, failure modes) in the hot path is the wrong trade.
GitHub still governs the library — at build time.

---

## 7. Implementation Plan (TDD, vertical slices)

Aggressive but real. Every phase is test-first and ends with something runnable. Each
engine stage gets its tests written before its implementation.

**Step 0 — Worktree (DONE).** Work proceeds in an isolated git worktree off
`linux-ops-suite`. *(Already created for this session.)*

**Phase 0 — Workspace member + engine spine (~0.5 day).**
- Add `crates/rex-forge` to the umbrella `[workspace].members`; wire `suite-ui`,
  `clap`, `thiserror`, `minijinja`, `include_dir`, `insta` (dev) via workspace deps.
- `lib.rs` engine module skeleton: `FileTree` type, single `writer` module.
- *Tests first:* `FileTree` insert/render-order tests; writer empty-dir + `--force` +
  dry-run tests (using a tempdir).
- One hardcoded `rust-bin` base rendered via minijinja and written.
- **Milestone:** `rex-forge new x --base rust-bin` produces a compiling project; engine
  unit tests green.

**Phase 1 — Component model + resolver (~1 day).**
- *Tests first:* resolver table tests — applies-to, requires auto-add, conflict
  rejection, ordering determinism, each typed error variant.
- `component.toml`/`base.toml` serde types; `build.rs` validation + `include_dir` embed.
- Resolve stage + merge stage (anchored injection + deterministic dep merge), each
  behind its tests.
- 3–4 real Rust components (clap, tracing, anyhow, ci-github).
- Non-interactive `--with` path fully wired.
- **Milestone:** `new x --base rust-bin --with clap,tracing` compiles clean; resolver +
  merge fully unit-tested.

**Phase 2 — Golden test harness (~0.5 day).**
- `insta` snapshots of generated trees for representative `(base, components)` combos.
- Compile gate: a test that `cargo build`/`clippy` (and `go build`) the generated
  projects in a tempdir. The Go portion is gated on a toolchain probe — skipped with a
  warning locally when `go` is absent, but **required green in CI** (where Go is installed).
- CI wired so bad components/combos fail.
- **Milestone:** regression-proof engine; safe to add components freely.

**Phase 3 — The TUI (~1.5 days).**
- suite-ui front-end: base select → details → grouped multi-select → confirm → result.
- Live filter, conflict/requires feedback, footer selection summary, `git` confirm
  toggle.
- `Tui` guard wiring (raw mode, alt screen, panic-safe teardown).
- *Tests:* state-machine unit tests for the TUI model (selection/resolve integration),
  driven without a real terminal.
- **Milestone:** `rex-forge new` is the interactive flow from §3.

**Phase 4 — Fill the library + bases (~1 day).**
- Remaining bases: `rust-lib`, `go-bin`, `go-lib` (with `go.mod` merge).
- Go components (cobra, viper, zap), plus `config`, `metrics`, `thiserror`,
  `dockerfile`, `ci-github` to complete the drafted menu.
- `rex-forge list`, post-gen notes/TODO surfacing, `--dry-run`, `--force`, `--git`.
- Golden tests extended to cover all four bases.
- **Milestone:** all four bases + the full drafted component set, all golden-tested.

**Phase 5 — Polish, docs, release (~0.5 day).**
- README, `SCHEMA.md`, `--help` text, exit codes, error copy.
- CI golden matrix green; update root `CHANGELOG.md` / `LAST_WORK.md`.
- Tag/build a v0.1 binary inside the workspace release flow.
- **Milestone:** shippable v0.1.

**Total:** ~5 working days to a real v0.1.

**Critical path:** Phase 1 (resolver + merge) is the riskiest; the visual work (Phase 3)
sits on a proven engine, so TUI work can't be blocked by generation bugs. Building
non-interactive first (Phases 0–2) means the engine is fully testable before a single
widget exists.

---

## 8. Open Questions

All v0.1 open questions are **resolved** (see the locked-decisions table above). Items
intentionally deferred past v0.1: `rex-forge update` / remote components, presets/bundles,
Go `/cmd`+`/internal` layout component, and eventual extraction of the in-tree library to
a standalone `rex-forge-components` repo.
