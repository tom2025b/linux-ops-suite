# Conductor → suite-ui migration

**Date:** 2026-06-22
**Status:** design, awaiting approval
**Crate:** `crates/conductor` (workspace member of `linux-ops-suite`)

## Problem

Conductor is the only TUI in the suite that does **not** use `suite-ui`. It
ships its own hand-rolled terminal stack (`src/tui/term.rs` raw-mode driver +
ioctl `TIOCGWINSZ`), its own ANSI string renderers (`src/tui/frame.rs`), and its
own colour resolver (`src/tui/style.rs`). It depends on neither `suite-ui`, nor
`ratatui`, nor `crossterm`. The result renders nothing like RexOps: different box
glyphs, different colours, no shared panes/overlays. Pulse was migrated to
suite-ui already (PR #40); Conductor is the leftover from the old pre-suite-ui
"modeled on pulse" pattern.

## Goal

Bare `conductor` on a TTY should present a clean, full-screen, professional
interface consistent with RexOps — built on `suite_ui::Tui` + `ratatui` + the
shared `Theme`, `pane`, `ConfirmModal`, and `HelpSheet` widgets — while
preserving Conductor's existing behaviour exactly:

- the guided runbook: a focused (`▸`) step, navigation, run/skip/advance;
- the Ring-2 safety gate (a changes-state step opens a confirm; only `y` runs it);
- the `r` handoff to the rexops cockpit;
- the exit-code contract (1 = a step failed, 2 = quit with steps unfinished,
  0 = clean / nothing to do);
- the scriptable paths: non-TTY / `--json` falls back to `status`; `--dump-view`
  renders one frame for snapshot tests.

## Decisions (from the user)

1. **Full rewrite mirroring RexOps**, not a minimal render-only swap. Adopt
   RexOps's *structure* — a crossterm event loop with a dirty-flag draw loop, an
   `App` state struct, the `suite_ui::Tui` guard, a `ForegroundRunner` for child
   spawns — and its *look* (panes/overlays/Theme). Conductor has one screen (the
   plan) plus two overlays, so we adopt the shape, not invent screens it has no
   data for.
2. **Alternate screen**, like RexOps: full-screen app in the alt buffer; quit
   restores the prior terminal. Scriptable paths are unaffected.

## What stays vs. changes

The domain layer is untouched: `plan/` (Plan, Step, Ring, StepStatus, rules),
`state.rs`, `sources.rs`, `run.rs` (the `Spawner` trait, `run_step`,
`confirm_command`, `RunOutcome`), `report.rs` (the scriptable status/health/JSON
renderers), `error.rs`, `util.rs`, `lib.rs`.

Replaced wholesale — everything under `src/tui/`:

| Old | New |
|-----|-----|
| `tui/term.rs` (hand-rolled raw mode + ioctl size) | deleted; `suite_ui::Tui` owns terminal setup/teardown/panic-restore/size |
| `tui/style.rs` (ANSI escape-string `Style`) | deleted; `suite_ui::Theme` owns colour (incl. `NO_COLOR`) |
| `tui/frame.rs` (String renderers) | `tui/render.rs` — ratatui pane/widget renderers drawing into a `Frame` |
| `tui/mod.rs` (state machine + own `run` loop + ioctl) | split into `tui/app.rs` (state + key→action), `tui/runtime.rs` (Tui draw/event loop), `tui/mod.rs` (wiring) |

The **state-machine logic** in today's `tui/mod.rs` — `AppState`, `step`,
`confirm_key`, `run_current`, `report_from`, `RunReport`, `Screen`, `Action` —
is preserved in behaviour. Because the user chose a full rewrite, it is
re-expressed against a crossterm `KeyEvent` input (via `suite_ui::keys`) rather
than the old `term::Key` enum, and re-homed into `tui/app.rs`. The transition
*semantics* (and therefore the existing transition tests, ported to feed
`KeyEvent`) are kept — the Ring-2 gate is the crate's core safety property and
must not regress.

## Architecture (mirrors RexOps)

```
crates/conductor/src/
  main.rs            # flags → dispatch; bare/orchestrate → tui::run on a TTY
  lib.rs             # unchanged exports
  plan/  state.rs  sources.rs  run.rs  report.rs  error.rs  util.rs   # UNCHANGED
  tui/
    mod.rs           # pub run(plan, color) -> io::Result<RunReport>; should_run_interactive()
    app.rs           # App state, Screen, Action, step()/confirm_key()/run_current(),
                     #   RunReport/report_from  (ported from old tui/mod.rs)
    runtime.rs       # Tui setup (TuiOptions{alt screen}), dirty-flag draw/event loop,
                     #   ForegroundRunner impl for the suspend-on-spawn handoff
    render.rs        # ratatui renderers: plan screen + confirm + help overlays
```

### Rendering (`tui/render.rs`) — the RexOps look

One top-level `render(f: &mut Frame, app: &App, theme: Theme)`, matching RexOps's
`ui::render` shape. Layout (vertical):

- **Header pane** — `pane("Conductor", theme)` titled rounded border (the suite
  chrome RexOps uses), a one-line dim subtitle.
- **Situation pane** (only when `plan.situation` is non-empty) —
  `pane("The situation", theme)`, one dim line per situation string.
- **Plan pane** — `pane("The plan", theme)`, one `Line` per step built the way
  RexOps's launcher rows are built:
  - an accent **selection rail** (`▌ `, `theme.selected_rail()`) on the focused
    step, two-space gutter otherwise — identical to `render_launcher_row`;
  - the status glyph (`▸` current, `○` pending, `✓` done, `·` skipped, `✗`
    failed), the step number, the title (`theme.selection()` when focused, else
    `theme.title()`);
  - the ring tag right-aligned, styled by ring (`theme.health(...)` /
    `theme.dim()` — read-only dim, changes-state attention, info dim);
  - a dim command line beneath (`theme.dim()`), and the inline `← annotation`
    in accent when present.
- **Footer / status strip** — the key hints
  (`enter run · s skip · a advance · r rexops · ? help · q quit`) in
  `theme.dim()`, plus the transient `notice` line when set.

Overlays (drawn over the plan frame, RexOps-style, each clears its own area):

- **Confirm** (Ring-2 gate) → `suite_ui::ConfirmModal { title, message }`
  rendered centered. `title` = the step title; `message` = the literal command +
  the "changes suite state" caution. The app still owns the `y`/`s`/anything-else
  key handling — `ConfirmModal` only draws. Its footer already reads
  `y: yes · n / Esc: no`; Conductor maps `y` → run, `s` → skip, any other key →
  back out (the existing safety semantics; `n`/`Esc` decline as before).
- **Help** → `suite_ui::HelpSheet { title: "Keys", rows: &[...] }` with one
  `(key, description)` row per binding, kept next to the real key handling so it
  cannot drift.

Empty plan ("nothing to conduct") → `suite_ui::EmptyState { message, ... }`
centered, replacing the old hand-spaced `healthy_screen`.

No more compact `<80×24` fallback string: ratatui reflows into whatever area the
panes get, so the manual ioctl/compact path is dropped (its job is now the
layout engine's).

### Terminal + runtime (`tui/runtime.rs`)

Modeled on `rexops-tui::{lib.rs, runtime.rs}`:

- `Tui::new(TuiOptions { hide_cursor: true, mouse_capture: false, require_tty:
  false })` enters raw mode + alternate screen and installs the panic-restore
  hook; its `Drop` restores on every exit path.
- A dirty-flag loop: draw when something changed, else just poll input
  (`crossterm::event::poll`/`read`, 100 ms) — copied from RexOps's `run`.
  Conductor has no background snapshots/jobs, so a tick is dirty only after a
  handled keypress; the up-front draw paints the first frame.
- `ForegroundRunner for Tui` (RexOps's exact pattern) services the Ring-1/Ring-2
  step spawns and the `r` rexops handoff via `Tui::suspended(...)`, which leaves
  raw mode + alt screen, runs the child on the real terminal, and re-enters —
  guaranteed even on child failure. This replaces the old `RawMode::suspend` +
  `SuspendSpawner`. The existing `Spawner` trait is satisfied by an adapter that
  forwards to the active `Tui` so `run_step`/`run_current` stay unchanged.

### main.rs

Minimal: keep flag parsing and dispatch. Bare/`orchestrate` on a TTY (and not
`--json`) calls `conductor::tui::run(plan, no_color)` exactly as today, just
backed by the new runtime. `--dump-view` now renders a frame into a
`ratatui::backend::TestBackend` and flattens the buffer to text (the technique
RexOps's screen tests already use) so snapshot tests keep working without a PTY;
`status` / `plan` / `health` / `--json` are unchanged (they never used the TUI).

## Cargo.toml

Add to `[dependencies]`:

```toml
suite-ui  = { workspace = true }
ratatui   = { workspace = true }
crossterm = { workspace = true }
```

`suite-ui`, `ratatui`, `crossterm` are already `[workspace.dependencies]` in the
umbrella root, so Conductor uses `{ workspace = true }` (path dep) — no git pin
(unlike the external `rexops` repo, which must pin a rev). This automatically
tracks the same suite-ui the rest of the workspace builds, so Conductor and the
in-suite TUIs render from one source.

## Testing

- **Ported transition tests** (from old `tui/mod.rs` tests): `q` quits, `a`
  advances, `s` skips+advances, Enter-on-Ring2 opens confirm and spawns nothing,
  confirm-`y` spawns once + marks Done + returns, confirm non-`y` never spawns,
  confirm-`s` skips, a failing spawn marks Failed and stays, `?` toggles help,
  notice clears on next key, exit-code mapping. These drive `step()` with
  synthesized `crossterm::KeyEvent`s instead of `term::Key`. Same assertions,
  same `FakeSpawner`/`ExitSpawner`, same `ENV_TEST_LOCK` PATH discipline.
- **Render tests** (new, RexOps-style): draw each view into a `TestBackend`,
  flatten the buffer, assert the plan shows the steps/commands/ring tags, the
  focused row shows the `▌` rail, the confirm modal shows the command + caution,
  help lists the keys, empty plan shows "nothing to conduct". Replaces the old
  `frame.rs` string-content + 80-column tests (the width math is now ratatui's).
- `cargo build -p conductor`, `cargo test -p conductor`, `cargo clippy -p
  conductor`, and a full `cargo test` at the workspace root must pass.
- **Manual smoke** (the user's acceptance bar): run `conductor` on a real
  terminal, confirm a clean full-screen RexOps-like interface, navigate, open the
  confirm on a changes-state step, open help, quit cleanly (terminal restored).
  This is a human step — I cannot drive a real PTY from here — so I'll report
  build/test results and ask the user to eyeball it (or I can provide a
  TestBackend render dump as a proxy).

## Non-goals (YAGNI)

- No new screens, no command palette, no live background refresh — Conductor has
  no streaming data; adding RexOps's multi-screen scaffolding would be empty
  weight.
- No change to plan rules, state loading, exit codes, or the scriptable output.
- No mouse support.

## Risks

- **Behaviour drift from the rewrite.** Mitigated by porting the existing
  transition tests verbatim (only the key type changes) — the Ring-2 gate is
  guarded by tests that must stay green.
- **`--dump-view` snapshot tests** assert on the old string layout. They are
  rewritten to assert on the flattened `TestBackend` buffer; any integration test
  under `tests/` that greps old exact spacing will be updated to match the new
  content (content assertions, not pixel-exact spacing).
- **MSRV.** suite-ui/ratatui/crossterm already build in this workspace at its
  rust-version, so no MSRV regression for an existing workspace member.
