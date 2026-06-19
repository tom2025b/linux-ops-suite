# Conductor Phase 2 — TUI + Ring-1 (implementation design)

Status: approved 2026-06-19. Builds on Phase 1 (Ring-0, read-only, merged and
verified green). The authoritative product design is `CONDUCTOR_DESIGN.md`
(repo root) — this document is the *implementation* design for Phase 2 only.

## Scope

Bare `conductor` opens an interactive TUI that renders the plan and lets the
operator run the **read-only (Ring 1)** steps, walking down the list. Conductor
still **cannot change state**: Ring-2 steps render (tag + literal command) but
selecting one is a **no-op-with-note** until Phase 3. No writes by Conductor's
own code, ever.

Explicitly out of scope (Phase 3): any Ring-2 execution, the `y`-confirm modal,
`conductor orchestrate`, exit codes 1/2, `--auto-readonly`, user config /
runbook files. Phase 1 rule semantics and the JSON envelope shape are frozen.

## Decisions locked before implementation

1. **TUI base: hand-rolled, dependency-free — ported from pulse.** Conductor's
   `Cargo.toml` has neither `suite-ui` nor `thomas-tui`, and pulse (the suite's
   real TUI) drives the terminal directly with zero TUI crates. Phase 2 follows
   that precedent and the "no new 3rd-party dep" house rule. The handoff prose
   mentioning thomas-tui/suite-ui is superseded by this decision.
2. **Phase 2 only**, sequenced before Phase 3 (which sits on top of it).

## Modules

```text
src/tui/mod.rs    interactive app: the event loop, AppState, navigation, step-
                  status transitions, the Ring-2 no-op note, the rexops handoff.
                  Calls term.rs for I/O and frame.rs for rendering. No rendering
                  logic of its own beyond choosing which frame to paint.
src/tui/term.rs   dependency-free terminal driver, ported ~verbatim from
                  pulse/src/tui.rs: RawMode{enter,restore,suspend},
                  install_panic_guard, Key, read_key, paint. The terminal is
                  ALWAYS restored (Drop + panic hook). suspend() hands the real
                  terminal to a foreground child and takes it back even on error.
src/tui/frame.rs  PURE String-returning renderers (NO I/O): the plan screen, the
                  healthy "nothing to conduct" screen, the compact <80×24
                  fallback, the dim help/hint strip, and the inline Ring-2
                  "needs Phase 3" note. This is what the snapshot tests hit.
src/tui/style.rs  TUI Style resolver: same shape as report::Style (empty strings
                  when color off, so call sites interpolate unconditionally) plus
                  the ▸ current-step cyan marker. Kept beside the frame code so
                  the one-shot (report.rs) and interactive renderers stay
                  independent — a change to one cannot affect the other.
src/run.rs        the Ring-1 spawn choke point. A Spawner trait (tests assert
                  intent without launching); RealSpawner spawns a known binary
                  with a fixed argv via std::process — NO shell. A $PATH check
                  before spawning. Ring-2 is refused here as defence in depth.
src/main.rs       bare `conductor` on a TTY -> tui::run(); non-TTY bare ->
                  print_status (stays scriptable, CI-safe). Hidden
                  --dump-view <name> for deterministic snapshot tests.
                  status/health/plan unchanged.
src/lib.rs        add `pub mod tui;` and `pub mod run;`.
```

Module-boundary intent (the testability story): `term.rs` only touches the
terminal; `frame.rs` only turns model -> String; `style.rs` only resolves
colors; `run.rs` is the only module that spawns a subprocess; `mod.rs` only
wires events to state and chooses a frame. A change to how a step renders cannot
change what runs; a new key binding cannot change how spawning works.

## State machine (`tui/mod.rs`)

```text
AppState { plan: Plan, cursor: usize, screen: Screen, notice: Option<String> }
Screen  = Plan | Help

- The healthy plan (Plan::is_empty()) renders the "nothing to conduct" frame.
- Step lifecycle lives on plan.steps[i].status (Pending/Done/Skipped — already
  defined in Phase 1). `cursor` is the ▸ current step.
- `notice` is a transient one-line message (e.g. the Ring-2 note, a missing-bin
  hint, the rexops-absent line); cleared on the next key.
- There is NO separate "running" screen: running a step suspends the entire TUI
  via term::RawMode::suspend, the child owns the real terminal, then the TUI
  resumes and repaints.
```

Key bindings (from CONDUCTOR_DESIGN.md "Navigation"):

```text
Enter  run the current step:
         Ring::ReadOnly     -> spawn (suspend/run/resume), mark Done, advance
         Ring::ChangesState -> NO-OP; set notice "needs Phase 3 — not run"
         Ring::Info         -> NO-OP (a fix hint; nothing to spawn)
         binary not on PATH -> notice with the install hint; stay on the step
s      skip current step -> Skipped, advance (skipped is noted, not lost)
a      advance focus without running
r      hand off to `rexops tui` (suspend + spawn); dim notice if rexops absent
?      toggle Help (the full hint strip)
q      quit — ALWAYS works. Esc also quits, but is never the only path.
```

When every step is Done/Skipped, the footer shows "plan complete"; `q` leaves.
No exit code 1/2 is emitted (reserved for Phase 3). The TUI exits 0 on a clean
quit, 3 only if Conductor itself could not start (e.g. no data dir) — identical
to Phase 1.

## `run.rs` — the spawn choke point (Ring-1 only)

```text
trait Spawner { fn spawn(&self, argv: &[String]) -> io::Result<ExitStatus>; }

RealSpawner:
  - argv is derived by splitting Step.command on ASCII whitespace. The command
    is a fixed string the rules built; a finding id is already a single token in
    it, so it stays one argv element — never interpolated, never shell-expanded.
  - Command::new(argv[0]).args(&argv[1..]).status() — direct exec, NO shell.
  - Guard: argv[0] must be one of the known suite binaries (SUITE_BINARIES /
    "rexops"); an unknown program is refused, not spawned. Asserted by test.
  - $PATH availability is checked (same probe sources.rs uses) before spawning;
    absent -> a "not available, install with …" outcome, no spawn.

Ring-2 refusal: run.rs will not spawn a ChangesState step in Phase 2 — it
returns a "would change state; not run in Phase 2" outcome. A test asserts a
ChangesState step is never handed to Spawner::spawn. This is defence in depth on
top of the TUI already routing Ring-2 to a no-op.

TestSpawner records the argv it was asked to run, so tests assert "would spawn
`pulse`" / "would spawn `bulwark show <id>`" with zero real processes.
```

## Testing strategy (mirrors Phase 1)

- **Deterministic frame render:** a hidden `--dump-view <plan|healthy|compact|
  help>` path renders a fixed synthetic `Plan` to a `String`, enabling
  snapshot/`contains` assertions with no PTY. Built early (it is the backbone of
  TUI testing here).
- **Width invariant (`frame.rs`):** no rendered line exceeds the viewport at
  80×24 and at a compact size — copied from pulse's assertion.
- **`run.rs`:** the $PATH check; a Ring-1 step builds the correct argv vector
  (no shell); a Ring-2 step is never spawned; a finding id stays one argv token;
  argv[0] is a known binary.
- **`term.rs`:** the `read_key` decoder tests come with the port (they already
  exist in pulse — plain keys, Enter/Backspace, lone Esc vs swallowed CSI, EOF,
  multibyte UTF-8, raw/cooked flag inverses).
- **PATH gotcha:** any "nothing to conduct" / wiring-free frame test neutralizes
  `$PATH` with the stub-bin-dir trick from `tests/cli.rs` (stub executables for
  all 8 `SUITE_BINARIES` on PATH). Do not clear PATH (that makes all 8 missing —
  the opposite failure).
- **Color off:** assert no ANSI escapes when color is forced off, as report.rs
  does.

## Task order (TDD, per-task commits)

```text
T1  src/tui/term.rs   port pulse's driver + its read_key/flags tests (green)
T2  src/tui/style.rs  Style resolver + ▸/glyph/severity colors (unit tests)
T3  src/tui/frame.rs  pure renderers: plan / healthy / compact / help
                      (snapshot tests + width invariant)
T4  src/run.rs        Spawner trait + RealSpawner + $PATH + argv + Ring-2 refusal
                      (intent tests; no real process)
T5  src/tui/mod.rs    event loop + navigation (Enter/s/a/r/?/q) + status
                      transitions + Ring-2 no-op note + rexops handoff
T6  src/main.rs       bare->TUI on TTY / non-TTY->status + --dump-view; keep
                      status/health/plan; integration tests via --dump-view;
                      README + LAST_WORK.md; full-workspace clippy -D warnings,
                      fmt --check, build
```

Each task: failing test → minimal impl → green → `cargo test -p conductor` +
`cargo clippy -p conductor --all-targets -- -D warnings` +
`cargo fmt -p conductor -- --check`, then a per-task commit (with explicit human
approval before any commit/push).

## Invariants held

- Conductor writes **zero** live files; executes **zero** Ring-2 commands.
- **No new third-party dependency** (term driver is hand-rolled; std only).
- `NO_COLOR` + non-TTY ⇒ monochrome; state carried by word + glyph, never color
  alone. Compact (<80×24) fallback cannot clip.
- No Escape-only flow; `q` always quits.
- Commands shown in the UI are exactly what `run.rs` would spawn — no hidden
  actions, no command string assembled from feed content beyond a single id
  token.
- Exit codes 0 (ok) / 3 (can't run) only; 1/2 stay reserved for Phase 3.

## Definition of done

- [ ] Bare `conductor` (on a TTY) opens the TUI; non-TTY falls back to status.
- [ ] Plan, healthy, and compact-fallback frames render; width-safe; NO_COLOR
      legible.
- [ ] Ring-1 steps spawn (foreground, argv, no shell) and mark `Done`; Ring-2
      steps render with tag + command but are a no-op-with-note; Info steps don't
      spawn.
- [ ] `r` hands off to `rexops tui` (graceful no-op if absent); `q` always quits;
      no Escape-only flow.
- [ ] Tests: snapshot frames + `run.rs` intent tests + width invariant, all
      green; clippy `-D warnings` clean; `cargo fmt --check` clean;
      `cargo build --workspace` clean.
- [ ] README + LAST_WORK.md updated. No Ring-2, no writes, no orchestrate.
- [ ] Nothing committed/pushed to a shared branch without explicit human
      approval.
