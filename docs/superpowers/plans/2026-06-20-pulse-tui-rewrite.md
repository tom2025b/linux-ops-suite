# Pulse TUI Rewrite — Moving to suite-ui + ratatui

**Date:** 2026-06-20. **Status:** executed (T1–T10) on branch
`worktree-pulse-suite-ui-migration`; see `LAST_WORK.md` for the commit list.

## Why

Pulse was a deliberately no-ratatui binary: a hand-rolled ANSI **string**
renderer (`main.rs render`/`app.rs frame`/`panel`/`clip_ansi`) blitted by a
libc `termios` driver (`tui.rs RawMode`/`paint`/`read_key` + an `extern "C"`
block). suite-ui is 100% ratatui, so adopting it is a rendering-**engine** port
(String-of-ANSI → ratatui `Frame`), not a widget swap. The pure navigation state
machine in `app.rs` (`App`/`View`/`handle`) is reused verbatim; only the impure
render + IO half changes.

## Architecture (the shape that makes the next tool's migration easy)

- `app.rs` — UNCHANGED logic: `App`, `View`, `handle(Key)`, `toggle`, `dump`,
  `search_hits`. Pure, fully unit-tested.
- `view.rs` (new) — the only chrome file: `draw(f: &mut Frame, &App)` dispatches
  per `View`, composed from suite-ui widgets. No state, no IO.
- the loop (`App::run`) — `Tui` guard + `terminal().draw(|f| view::draw(f,&app))`
  + `tui::read_event()` (crossterm) + `cockpit::open(&mut tui)` via
  `Tui::suspended`.
- input adapter (`tui.rs`) — crossterm `KeyEvent` → the existing `Key` enum, so
  the state machine and its tests are untouched.

## suite-ui components used

`Tui`/`TuiOptions` (guard), `Theme`/`ThemeChoice`/`ColorChoice`,
`truncate_path`/`truncate_desc`, `pane_titled`, `KeyHints`, `SeverityBadge`,
`HealthStrip`, `HelpSheet`, `SearchBar`, `EmptyState`. A `suite_severity` shim
maps Pulse's domain `Severity` onto `suite_ui::Severity` at the draw boundary.

## Tasks (each: red → green → clippy -D warnings + fmt → commit; nav tests green throughout)

- **T1** add deps (suite-ui, ratatui, crossterm, +insta dev).
- **T2** swap `RawMode` → `suite_ui::Tui` behind a monochrome Paragraph bridge;
  crossterm input adapter; cockpit via `Tui::suspended`.
- **T3** confirm the crossterm adapter is the sole production input; test mapping.
- **T4** resolve `suite_ui::Theme`; parse `--theme`/`--color`.
- **T5** `view.rs` + the default verdict screen in real ratatui; insta snapshots.
- **T6** port drill-downs (Attention/Feeds/Details/Help/Search) to suite-ui.
- **T7** cover the transient status overlay from the draw side.
- **T8/T9** re-point `--dump-view`/`--state` to a headless `TestBackend` render;
  convert the nav tests off the legacy string `frame()`.
- **T10** delete the legacy string renderer + the termios driver.
- **T11** (optional) lift the loop/input into a reusable template — SKIPPED as
  YAGNI until a second tool is migrated.
- **T12** final gate (workspace test/clippy/fmt) + docs (this file,
  PULSE_DESIGN.md note, LAST_WORK.md). Manual real-terminal smoke is recommended
  before merge and is not unit-testable.

## Challenges / tradeoffs

- Rendering-model flip is the bulk of the work; all string-clipping dies.
- Binary-size / "tiny no-ratatui" ethos change — accepted to gain shared chrome.
- Cockpit suspend parity (the one place a regression strands the terminal):
  `Tui::suspended` matches the old guarantee; verify manually.
- Calm-glance aesthetic: the default screen stays bare (no heavy pane).
- Two `Severity` enums — keep the domain one, convert at the draw boundary.
- Intentional restyles: `[CRIT]` badge, glyph health strip, Help-as-overlay.

## Testing

Navigation state-machine tests (pure `handle`) are the regression net and stay
green every step. Rendering is verified against the real ratatui draw via
`TestBackend`: content assertions per view, width-safety + tiny-size sweeps, and
`insta` geometry snapshots (glyphs/layout; colour stays covered by Theme's own
style assertions).
