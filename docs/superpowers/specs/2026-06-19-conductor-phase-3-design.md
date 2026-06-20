# Conductor Phase 3 — the driver: Ring-2 + confirm modal + orchestrate (implementation design)

Status: drafted 2026-06-19. Builds on Phase 2 (TUI + Ring-1, merged and verified
green at `602c992`). The authoritative product design is `CONDUCTOR_DESIGN.md`
(repo root) — this document is the *implementation* design for Phase 3 only.

## Scope

Turn the read-only walker into the **driver**. Three things land, all gated:

1. **Ring-2 execution** — `run.rs` stops refusing `ChangesState` steps and, on an
   explicit confirm, spawns the state-changing sibling in the foreground (same
   no-shell, fixed-argv, known-binary, `$PATH`-checked path Ring-1 already uses).
2. **The confirm modal** — a Ring-2 step never fires on Enter. Enter on a Ring-2
   step opens a confirm screen showing the *literal* command and a one-line
   "it changes state" caution; the spawn happens only on a deliberate `y`. An
   explicit non-Esc back path (`q`) and a `s` (skip) are offered, per the suite's
   keyboard constraint.
3. **`conductor orchestrate`** — the explicit CLI verb for the driver. It opens
   the *same* TUI as bare `conductor` (it is the named entry to the same driver),
   and **wires exit codes 1 and 2**, which bare `conductor` now also returns.

Out of scope (Future, explicitly not Phase 3): `--auto-readonly` (run Ring-0/1
unattended, Ring-2 always stops), user-authored runbook files / config-driven
plans. Phase 1 rule semantics and the JSON envelope shape stay frozen. No new
third-party dependency.

## Decisions locked before implementation

These are the open questions resolved with the user on 2026-06-19:

1. **Exit codes apply to BOTH bare `conductor` and `orchestrate`.** They share
   one driver, so both return the full set. The bare TUI is no longer always-0.

   ```text
   0  clean quit / nothing to conduct / every step completed (Done or Skipped
      with none failed) / operator quit with no pending work
   1  a delegated step the run actually executed exited non-zero (a step failed)
   2  the operator quit with one or more steps still PENDING or SKIPPED and none
      failed (a guided run left unfinished)
   3  conductor itself could not run (no data dir, etc.) — unchanged
   ```

   Precedence when the run ends: **1 wins over 2** (a real failure outranks
   "left unfinished"). All-Done with no failure ⇒ 0. Empty plan ⇒ 0.

2. **`orchestrate` is the same driver as bare `conductor`, including the non-TTY
   fallback.** With no TTY it prints `status` and exits 0, exactly like bare
   `conductor` (consistency over a hard error — the user's call). The only
   difference from bare is that `orchestrate` is an explicit, discoverable verb;
   the code path is shared, not duplicated.

3. **A confirmed Ring-2 step whose tool exits non-zero is NOT marked Done.**
   (Determined by decision 1: "1 = a step failed" is only meaningful if a failed
   step is tracked as failed.) The step takes a new `Failed` status, a notice
   names the failure, and the cursor stays on the step so the operator can retry
   (`Enter` again) or `s` to skip. A failed step drives the exit-1 outcome.
   Ring-1 adopts the same rule for consistency (a read-only step that exits
   non-zero is `Failed`, not `Done`) — this is a deliberate, small change from
   Phase 2, where Ring-1 marked Done regardless of exit status.

## What changes, file by file

```text
src/plan/mod.rs   StepStatus gains a `Failed` variant. (Pending/Done/Skipped ->
                  +Failed.) The enum is the single source of per-step lifecycle;
                  the driver, the renderers, and the exit-code logic all read it.
                  No rule change — build() still only emits Pending. plan_id /
                  ordering unchanged. NOTE: `StepStatus` matches are exhaustive in
                  TWO renderers — `report.rs::status_glyph` (one-shot text) and
                  `tui/frame.rs::glyph` (TUI) — so BOTH must add a `Failed` arm or
                  the build breaks. Failed renders as '✗' (amber) in both.

src/run.rs        Remove the Ring-2 refusal. run_step() no longer special-cases
                  ChangesState — by the time it is called the confirm has already
                  happened (the *gate* lives in the TUI, not here; run.rs is the
                  mechanism). run.rs keeps every other guard verbatim: known
                  binary only, $PATH check, fixed argv, NO shell. RunOutcome
                  loses `RefusedChangesState` (no longer reachable) and keeps
                  Ran(bool)/NotAvailable/NotRunnable. A new public helper
                  `confirm_command(step) -> Option<&str>` (the literal command a
                  Ring-2 confirm should display) keeps the modal text derived
                  from the same source run.rs spawns, so the shown command and
                  the run command can never drift.

src/tui/mod.rs    The event loop becomes the driver:
                  - Screen gains `Confirm` (the Ring-2 modal). AppState already
                    carries plan + cursor + notice; no new field needed — the
                    confirm always targets the cursor step.
                  - Enter on a ChangesState step -> Screen::Confirm (does NOT
                    spawn). Enter on Ring-1 -> spawn now (unchanged path).
                  - In Confirm: `y` -> run the cursor step via run_step, apply
                    the outcome (Done+advance / Failed+stay / NotAvailable note),
                    return to Plan; `s` -> Skipped+advance, return to Plan;
                    `q`/Esc -> back to Plan, run nothing. NO other key fires it.
                  - run_current() applies outcomes uniformly for Ring-1 and the
                    confirmed Ring-2: Ran(true)->Done+advance, Ran(false)->Failed
                    (stay), NotAvailable->notice (stay), NotRunnable->Info note.
                  - run() returns a `RunReport` (see below) instead of (); the
                    loop computes it from the final plan on Quit.

src/main.rs       - Add `Orchestrate` to the Cmd enum; it dispatches to the same
                    `run_bare` path (shared driver). Bare `conductor` unchanged
                    in behavior except it now propagates the driver's exit code.
                  - tui::run(...) now returns a RunReport; run_bare maps it to an
                    ExitCode: 1 if any step Failed, else 2 if any step Pending or
                    Skipped, else 0. Non-TTY / --json still -> status (exit per
                    health/status as today). --dump-view unchanged, plus a new
                    `confirm` view for snapshot-testing the modal.

src/tui/frame.rs  Add `confirm_screen(step, &Style) -> String`: the modal — the
                  step title with the `changes state` tag, the literal command on
                  its own line, the one-line caution, and the
                  `y run it   s skip   q back` strip. Amber on the tag/caution,
                  words carry it with color off. The existing help text drops the
                  "needs Phase 3" caveat; the hint strip is unchanged.

src/error.rs      No change (Tui(String) already exists from Phase 2).
src/lib.rs        No new module; `RunReport` lives in tui. (No env-test plumbing
                  beyond the existing ENV_TEST_LOCK.)
```

### `RunReport` — how the loop reports an outcome

```text
// in tui/mod.rs — a tiny value the loop returns and main maps to an exit code.
pub struct RunReport { pub failed: usize, pub unfinished: usize }
// failed     = count of steps with status Failed
// unfinished = count of steps still Pending or Skipped (only counts toward
//              exit 2 when failed == 0)
impl RunReport { pub fn exit_code(&self) -> u8 {
    if self.failed > 0 { 1 } else if self.unfinished > 0 { 2 } else { 0 } } }
```

The loop builds it from `app.plan.steps` at the moment of Quit, so it reflects
exactly what the operator left behind. An empty plan yields {0,0} ⇒ 0.

## The confirm modal (UX, from CONDUCTOR_DESIGN.md "Running a step")

```text
   ▸ 1  refresh stale data                              changes state

        this will run:  workstate snapshot
        it changes suite state.

        y  run it        s  skip        q  back to plan
```

- Reached ONLY by Enter on a `ChangesState` step. Any non-`y` key does not spawn.
- `y` is the sole spawn trigger — a stray Enter cannot fire a state change
  (Enter inside Confirm is treated as "no", same as any other non-`y` key, and
  returns to the plan without running). This is the central safety property and
  gets an explicit test.
- `q` (and Esc) is the non-Esc-only back path. `s` skips from within the modal.
- The command shown is `run.rs::confirm_command(step)` — the same string run.rs
  will spawn, so the modal can never advertise a different command than it runs.
- The spawned tool's own gate still applies on top (defence in depth: e.g.
  `rewind restore` is itself dry-run-first). Conductor confirms once; the tool
  decides what is safe.

## Safety invariants (unchanged from the product design; re-asserted)

- Conductor still writes **zero** live files with its own code. The most it does
  is spawn a known sibling, now including Ring-2 — but only after a `y` confirm.
- **No command is ever assembled from feed content beyond a single id token.**
  argv is whitespace-split from the fixed rule-built command; an id is one token.
  No shell is invoked (`Command`, direct exec). Re-tested in Phase 3.
- **Only known suite binaries** (SUITE_BINARIES + rexops) are spawnable; unknown
  programs are refused in run.rs. Re-tested.
- No `--yes-to-all`, no end-to-end unattended run. Each Ring-2 step is confirmed
  individually; the modal is per-step.
- No Escape-only flow anywhere: `q` always backs out of the modal and always
  quits the app.

## Testing strategy (mirrors Phase 1/2; heaviest scrutiny per the design)

`run.rs`:
- A `ChangesState` step IS now spawned by `run_step` (the refusal is gone) — and
  produces `Ran(bool)` like any other; assert via TestSpawner it reaches the
  spawner with the right argv. (The *gate* is the TUI's job, tested there.)
- All Phase-2 guards still hold: unknown program never spawned; finding id stays
  one argv token (no shell); `$PATH`-absent ⇒ NotAvailable, no spawn.
- `confirm_command(step)` returns the same string the step would spawn.

`tui/mod.rs` (the gate — densest new coverage):
- Enter on a Ring-2 step opens Confirm and spawns NOTHING.
- In Confirm: `y` spawns exactly once and (success) marks Done + advances.
- In Confirm: `q`, Esc, Enter, and an arbitrary key each spawn NOTHING and
  return to Plan (the "stray Enter can't fire a state change" property).
- In Confirm: `s` marks Skipped + advances, spawns nothing.
- A spawned step that exits non-zero ⇒ status Failed, cursor stays, notice set
  (both Ring-1 and confirmed Ring-2).
- `RunReport.exit_code()`: all-Done ⇒ 0; any Failed ⇒ 1; some Pending/Skipped &
  none failed ⇒ 2; failed beats unfinished. Unit-tested directly on RunReport
  plus one end-to-end via the step() loop.
- All existing Phase-2 transition tests keep passing (q quits, a advances, s
  skips, ? help, notice clears, Ring-1 spawns) — Ring-1's mark is now Done only
  on success.

`frame.rs`:
- `confirm_screen` contains the literal command, the `changes state` tag, the
  caution, and the `y`/`s`/`q` strip; width-safe at 80×24; no ANSI with color
  off. Added to the no-color and width-invariant sweep tests.

`tests/cli.rs` (integration, via the binary):
- `--dump-view confirm` renders the modal deterministically (no PTY).
- `conductor orchestrate --json` (non-TTY) behaves like `status --json`
  (same envelope), proving the shared non-TTY fallback and that orchestrate
  doesn't require a terminal.
- `conductor orchestrate` exists as a verb (help lists it).

Exit codes: the existing 0/3 integration assertions stay; new unit coverage for
1/2 lives on `RunReport` (deterministic, no PTY needed to prove the mapping).

## Task order (TDD, per-task commits)

```text
T1  plan/mod.rs   add StepStatus::Failed. Wire the '✗' (amber) arm into BOTH
       + glyphs   exhaustive matches: report.rs::status_glyph and
                  tui/frame.rs::glyph (the build won't compile until both have it).
                  Tests: the new variant; each renderer emits '✗' for Failed.
                  Smallest first; everything below leans on it.
T2  run.rs        drop the Ring-2 refusal (run_step no longer special-cases
                  ChangesState); add confirm_command(); keep every other guard
                  verbatim. Remove the RunOutcome::RefusedChangesState variant and
                  REWRITE its test (`ring2_step_is_refused...`) into
                  `ring2_step_now_spawns_after_the_gate...` asserting it reaches
                  the spawner with the right argv. Tests: guards intact;
                  confirm_command matches the spawn string.
T3  frame.rs      confirm_screen() + drop the "needs Phase 3" caveat from the help
                  text (replace with the real changes-state-confirms wording). The
                  two frame tests that pass a "needs Phase 3 — not run" notice
                  string are cosmetic — swap the literal for a neutral notice
                  (e.g. "a step failed"); they only assert the notice line renders.
                  Tests: modal content, width, no-color. (Pure; no loop wiring.)
T4  tui/mod.rs    Screen::Confirm + the gate + uniform outcome handling + Failed
                  status + RunReport + run() returns it. The Phase-2 test
                  `enter_on_ring2_is_a_noop_with_note...` is REPLACED by the gate
                  matrix (Enter opens Confirm + spawns nothing; y spawns once;
                  q/Esc/Enter/other back out; s skips). Also: the
                  `RunOutcome::RefusedChangesState` arm in run_current() is
                  deleted. Tests: the full gate matrix + RunReport mapping +
                  Phase-2 transitions still green (Ring-1 now Done only on
                  success). The heart of the phase; heaviest tests.
T5  main.rs       Orchestrate verb -> shared run_bare; map RunReport to exit code;
                  --dump-view confirm. Integration tests (orchestrate verb +
                  --json fallback + dump-view confirm). README + LAST_WORK.md.
                  Full-workspace: clippy -D warnings, fmt --check, build.
```

Each task: failing test → minimal impl → green → `cargo test -p conductor` +
`cargo clippy -p conductor --all-targets -- -D warnings` +
`cargo fmt -p conductor -- --check`, then a per-task commit (with explicit human
approval before any commit/push, per the standing rule).

## Definition of done

- [ ] Enter on a Ring-2 step opens the confirm modal and runs nothing; only `y`
      spawns; `q`/Esc/Enter/other keys back out without running.
- [ ] A confirmed Ring-2 step spawns the known binary (foreground, fixed argv, no
      shell) and, on success, marks `Done` + advances; on non-zero exit marks
      `Failed` + stays. Ring-1 follows the same Done/Failed rule.
- [ ] `conductor orchestrate` opens the same TUI as bare `conductor`; non-TTY /
      `--json` falls back to `status`; the verb is listed in `--help`.
- [ ] Exit codes: 0 clean/all-done/empty, 1 a step failed, 2 quit with
      pending/skipped (failure outranks unfinished), 3 can't-run — for BOTH bare
      `conductor` and `orchestrate`.
- [ ] `--dump-view confirm` renders the modal; help no longer says "needs
      Phase 3"; every frame width-safe and NO_COLOR-legible.
- [ ] Conductor writes zero live files; only known binaries spawnable; no shell;
      a finding id stays one argv token. All re-tested.
- [ ] Tests green; clippy `-D warnings` clean; `cargo fmt --check` clean;
      `cargo build --workspace` clean.
- [ ] README + LAST_WORK.md updated. Nothing committed/pushed without explicit
      human approval.
