# Conductor Phase 3 (the driver: Ring-2 + confirm modal + orchestrate) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Conductor's read-only walker into the driver: Enter on a Ring-2 (changes-state) step opens a confirm modal that spawns the sibling only on a deliberate `y`; add the `orchestrate` verb; and make both bare `conductor` and `orchestrate` return meaningful exit codes (1 a step failed, 2 quit unfinished).

**Architecture:** The *gate* lives in the TUI event loop (a new `Screen::Confirm`); `run.rs` is the unchanged spawn *mechanism* (known-binary-only, `$PATH`-checked, fixed argv, no shell) with its Phase-2 Ring-2 refusal removed because the confirm now precedes it. A step gains a `Failed` status; the loop returns a tiny `RunReport` the binary maps to an exit code. No new dependency; rule engine and JSON envelope untouched.

**Tech Stack:** Rust, std only (no new third-party dep), `clap` (already present). Reuses Phase 1/2: `conductor::load_state`, `conductor::plan::build`, `Plan`/`Step`/`Ring`/`StepStatus`, `run::{Spawner, RealSpawner, run_step, RunOutcome}`, `tui::{frame, style, term}`.

## Global Constraints

- No new third-party dependency. std only (terminal bits already hand-rolled in Phase 2).
- Conductor writes ZERO live files with its own code. The only state change is a spawned known sibling, and ONLY after a `y` confirm.
- Only known suite binaries (`SUITE_BINARIES` + `rexops`) are spawnable; argv is whitespace-split from the fixed rule-built command; a finding id stays ONE argv token; NO shell is ever invoked.
- A changes-state step NEVER fires on Enter or on any key other than `y`. A stray Enter cannot trigger a state change. Every confirm has an explicit non-Esc back path (`q`).
- `NO_COLOR` set OR non-TTY ⇒ monochrome. State is carried by word + glyph, never color alone. Every frame width-safe at 80×24.
- Exit codes (BOTH bare `conductor` and `orchestrate`): `0` clean/all-done/nothing-to-conduct, `1` a step that ran exited non-zero, `2` quit with steps still Pending/Skipped (failure outranks unfinished), `3` conductor itself could not run. Non-TTY / `--json` still falls back to `status`.
- Per task: `cargo test -p conductor` + `cargo clippy -p conductor --all-targets -- -D warnings` + `cargo fmt -p conductor -- --check` must pass before commit. Per-task commits. NOTHING committed/pushed without explicit human approval.
- Phase 1 rule semantics and the JSON envelope shape are FROZEN — do not change `plan/rules.rs` or the `report.rs` envelope fields. (Adding a `Failed` glyph arm to `report.rs::status_glyph` is allowed; it is rendering, not envelope shape.)

---

### Task 1: Add `StepStatus::Failed` and wire its glyph into both renderers

**Files:**
- Modify: `crates/conductor/src/plan/mod.rs:36-41` (the `StepStatus` enum)
- Modify: `crates/conductor/src/report.rs:71-77` (`status_glyph`)
- Modify: `crates/conductor/src/tui/frame.rs:18-27` (`glyph`)
- Test: inline `#[cfg(test)] mod tests` in `plan/mod.rs`, `report.rs`, `tui/frame.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `StepStatus::Failed` (a fourth variant; `#[derive(Clone, Copy, PartialEq, Eq, Debug)]` already on the enum). Both `status_glyph` (report) and `glyph` (frame) render it as `'✗'`. Later tasks set this status; the exit-code logic in Task 4/5 reads it.

- [ ] **Step 1: Write the failing test (plan/mod.rs)**

Add to the `#[cfg(test)] mod tests` block in `crates/conductor/src/plan/mod.rs`:

```rust
    #[test]
    fn step_status_has_a_failed_variant() {
        // Failed is distinct from the other three lifecycle states.
        let f = StepStatus::Failed;
        assert_ne!(f, StepStatus::Pending);
        assert_ne!(f, StepStatus::Done);
        assert_ne!(f, StepStatus::Skipped);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p conductor step_status_has_a_failed_variant`
Expected: FAIL — compile error, `no variant named Failed found for enum StepStatus`.

- [ ] **Step 3: Add the variant**

In `crates/conductor/src/plan/mod.rs`, change the `StepStatus` enum (currently lines 36-41) to:

```rust
/// A step's lifecycle. Phase 1 only ever produces `Pending`; `Done`/`Skipped`/
/// `Failed` are driven by the Phase 2/3 TUI. `Failed` means a delegated step
/// actually ran and its sibling exited non-zero — distinct from Skipped (the
/// operator passed on it) and used to drive the guided run's exit code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepStatus {
    Pending,
    Done,
    Skipped,
    Failed,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p conductor step_status_has_a_failed_variant`
Expected: PASS. (The build now fails elsewhere — `status_glyph` and `glyph` are non-exhaustive — fixed in the next steps; that is expected mid-task.)

- [ ] **Step 5: Write the failing tests for the glyph arms**

Add to the `#[cfg(test)] mod tests` block in `crates/conductor/src/report.rs`:

```rust
    #[test]
    fn failed_step_renders_a_cross_glyph() {
        assert_eq!(status_glyph(StepStatus::Failed), '✗');
    }
```

Add to the `#[cfg(test)] mod tests` block in `crates/conductor/src/tui/frame.rs`:

```rust
    #[test]
    fn failed_step_glyph_is_a_cross_when_not_current() {
        assert_eq!(glyph(StepStatus::Failed, false), '✗');
        // the current marker still wins regardless of status
        assert_eq!(glyph(StepStatus::Failed, true), '▸');
    }
```

- [ ] **Step 6: Run the tests to verify they fail to compile**

Run: `cargo test -p conductor 2>&1 | head -30`
Expected: compile error — both `status_glyph` and `glyph` `match` statements are non-exhaustive (`pattern StepStatus::Failed not covered`).

- [ ] **Step 7: Add the `Failed` arm to both glyph matches**

In `crates/conductor/src/report.rs`, change `status_glyph` (lines 71-77) to:

```rust
/// The glyph for a step's status: the one-shot renderer marks every pending step
/// with `○` (the TUI decides the `▸` current marker); `✓` done, `·` skipped,
/// `✗` failed.
fn status_glyph(status: StepStatus) -> char {
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
        StepStatus::Failed => '✗',
    }
}
```

In `crates/conductor/src/tui/frame.rs`, change `glyph` (lines 18-27) to:

```rust
/// The glyph for a step in the interactive view. The current step overrides this
/// with `▸` regardless of status (it is by definition Pending when focused).
fn glyph(status: StepStatus, is_current: bool) -> char {
    if is_current {
        return '▸';
    }
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
        StepStatus::Failed => '✗',
    }
}
```

- [ ] **Step 8: Run the full crate tests + lint + fmt**

Run: `cargo test -p conductor && cargo clippy -p conductor --all-targets -- -D warnings && cargo fmt -p conductor -- --check`
Expected: PASS (all three).

- [ ] **Step 9: Commit (after explicit human approval)**

```bash
git add crates/conductor/src/plan/mod.rs crates/conductor/src/report.rs crates/conductor/src/tui/frame.rs
git commit -m "feat(conductor): add StepStatus::Failed (✗) for delegated steps that error"
```

---

### Task 2: `run.rs` — drop the Ring-2 refusal, add `confirm_command`

**Files:**
- Modify: `crates/conductor/src/run.rs` (`RunOutcome` enum, `run_step`, add `confirm_command`; rewrite the refused test)
- Test: inline `#[cfg(test)] mod tests` in `run.rs`

**Interfaces:**
- Consumes: `StepStatus::Failed` is NOT used here (run.rs returns outcomes, not statuses). Uses `Step`, `Ring`, `is_on_path`, `known_program` (all unchanged).
- Produces:
  - `RunOutcome` WITHOUT the `RefusedChangesState` variant (removed): `Ran(bool)` / `NotAvailable(String)` / `NotRunnable`.
  - `pub fn confirm_command(step: &Step) -> Option<&str>` — the literal command string a Ring-2 confirm should display (the SAME source `run_step` will spawn). Returns `None` for a commandless step.
  - `run_step` now spawns a `ChangesState` step exactly like a `ReadOnly` one (the gate is the TUI's job). Info / commandless / unknown-program / not-on-PATH guards all unchanged.

- [ ] **Step 1: Rewrite the refused test into a "now spawns" test**

In `crates/conductor/src/run.rs`, REPLACE the existing test `ring2_step_is_refused_and_never_spawned` (currently lines 122-136) with:

```rust
    #[test]
    fn ring2_step_now_spawns_after_the_gate() {
        // Phase 3: run.rs no longer refuses a changes-state step — by the time
        // run_step is called the TUI confirm has already happened. The gate is
        // the loop's job (tested in tui/mod.rs); run.rs is just the mechanism.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("workstate");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let new_path = match &orig {
            Some(p) => {
                let mut v = std::ffi::OsString::from(dir.path());
                v.push(":");
                v.push(p);
                v
            }
            None => std::ffi::OsString::from(dir.path()),
        };
        std::env::set_var("PATH", &new_path);

        let sp = TestSpawner::new(true);
        let step = Step::new(
            "refresh",
            "refresh",
            Some("workstate snapshot".into()),
            Ring::ChangesState,
        );
        let outcome = run_step(&step, &sp);
        assert!(matches!(outcome, RunOutcome::Ran(true)));
        let calls = sp.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "workstate");
        assert_eq!(calls[0][1], "snapshot");

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn confirm_command_returns_the_exact_command_that_would_spawn() {
        let step = Step::new(
            "refresh",
            "refresh",
            Some("workstate snapshot".into()),
            Ring::ChangesState,
        );
        assert_eq!(confirm_command(&step), Some("workstate snapshot"));
        let prose = Step::new("note", "note", None, Ring::ChangesState);
        assert_eq!(confirm_command(&prose), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --lib run::tests 2>&1 | head -30`
Expected: FAIL — `confirm_command` undefined, and `ring2_step_now_spawns_after_the_gate` fails because `run_step` still returns `RefusedChangesState` (so `matches!(.., Ran(true))` is false).

- [ ] **Step 3: Remove the `RefusedChangesState` variant**

In `crates/conductor/src/run.rs`, change the `RunOutcome` enum (currently lines 34-44) to:

```rust
/// What happened (or didn't) when asked to run a step.
#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// The step ran; the bool is the child's success().
    Ran(bool),
    /// The step's binary is not on `$PATH`; carries the binary name for a hint.
    NotAvailable(String),
    /// The step has no runnable command (Info, or no command at all).
    NotRunnable,
}
```

- [ ] **Step 4: Drop the Ring-2 refusal in `run_step` and add `confirm_command`**

In `crates/conductor/src/run.rs`, change `run_step` (currently lines 60-84) to remove the first `if step.ring == Ring::ChangesState` block, and add `confirm_command` just above it. The doc comment is updated to reflect Phase 3:

```rust
/// The literal command a Ring-2 confirm should display — the SAME string
/// `run_step` will spawn, so the modal can never advertise a different command
/// than it runs. `None` for a pure-prose step.
pub fn confirm_command(step: &Step) -> Option<&str> {
    step.command.as_deref()
}

/// Run a single step through the spawner, enforcing every safety rule: the
/// program must be a known suite binary and present on `$PATH` before any spawn;
/// Info / commandless steps are not runnable. A Ring-2 step is spawned like any
/// other — the confirm gate that precedes it lives in the TUI, not here.
pub fn run_step(step: &Step, spawner: &dyn Spawner) -> RunOutcome {
    let Some(cmd) = &step.command else {
        return RunOutcome::NotRunnable;
    };
    if step.ring == Ring::Info {
        return RunOutcome::NotRunnable;
    }
    let argv = argv_of(cmd);
    if argv.is_empty() || !known_program(&argv[0]) {
        return RunOutcome::NotRunnable;
    }
    if !is_on_path(&argv[0]) {
        return RunOutcome::NotAvailable(argv[0].clone());
    }
    match spawner.spawn(&argv) {
        Ok(status) => RunOutcome::Ran(status.success()),
        Err(_) => RunOutcome::Ran(false),
    }
}
```

Also update the module-level doc comment at the top of `run.rs` (lines 1-9): replace the Phase-2 paragraph that says "Phase 2 runs ONLY Ring-1 … A Ring-2 step is refused here" with:

```rust
//! The delegated-spawn layer — Conductor's single subprocess choke point.
//!
//! A Ring-2 (state-changing) step is spawned like any other step here; the
//! `y`-confirm gate that must precede it lives in the TUI (`tui/mod.rs`), not in
//! this module. Spawning is direct (`std::process::Command`) with a fixed argv
//! vector and NO shell, so a finding id carried in a step's command can never
//! become a shell metacharacter — it is one argv element. The actual launch sits
//! behind the `Spawner` trait so tests can assert intent ("would spawn X with
//! argv […]") without starting a real process.
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p conductor --lib run::tests`
Expected: PASS (including the unchanged `unknown_program_is_never_spawned`, `info_step_is_not_runnable`, `readonly_step_builds_argv_with_id_as_one_token_and_no_shell`, `known_program_recognizes_suite_bins_only`).

- [ ] **Step 6: Verify the rest of the crate still compiles (the deleted variant)**

Run: `cargo build -p conductor 2>&1 | head -20`
Expected: a compile error in `crates/conductor/src/tui/mod.rs` — `RunOutcome::RefusedChangesState` no longer exists (the arm at mod.rs:129). This is EXPECTED; that arm is removed in Task 4. To keep Task 2 self-contained and committable, apply this minimal stopgap now and finish the real handling in Task 4:

In `crates/conductor/src/tui/mod.rs`, delete the `RunOutcome::RefusedChangesState => { … }` arm (currently lines 129-131) from `run_current`'s `match`. The match over `RunOutcome` becomes exhaustive again with just `Ran` / `NotAvailable` / `NotRunnable`. (The Ring-2 routing is reworked entirely in Task 4; for now Enter on a Ring-2 step will fall through to `Ran`/`NotAvailable` like any step — that intermediate behavior is replaced before this phase ships.)

After deleting the arm, re-run: `cargo build -p conductor`
Expected: PASS.

- [ ] **Step 7: Full crate tests + lint + fmt**

Run: `cargo test -p conductor && cargo clippy -p conductor --all-targets -- -D warnings && cargo fmt -p conductor -- --check`
Expected: PASS. (Note: the Phase-2 test `enter_on_ring2_is_a_noop_with_note_and_no_spawn` in tui/mod.rs will now FAIL because the no-op arm is gone. Mark it `#[ignore = "replaced by the confirm-gate tests in Task 4"]` for this one commit; Task 4 deletes it. If you prefer not to ignore, you may instead move this whole stopgap into Task 4 and keep Task 2 limited to run.rs — but then Task 2 won't build standalone. The `#[ignore]` keeps each task green.)

- [ ] **Step 8: Commit (after explicit human approval)**

```bash
git add crates/conductor/src/run.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): run.rs spawns Ring-2 (gate moves to TUI) + confirm_command"
```

---

### Task 3: `frame.rs` — the confirm modal + scrub the "needs Phase 3" help

**Files:**
- Modify: `crates/conductor/src/tui/frame.rs` (add `confirm_screen`; fix `help_screen`; swap the cosmetic notice strings in two tests)
- Test: inline `#[cfg(test)] mod tests` in `tui/frame.rs`

**Interfaces:**
- Consumes: `Step`, `Ring`, `Style` (all present), `confirm_command` is NOT needed here (the modal takes the `&Step` and reads its command directly via the same field; the loop uses `confirm_command` to decide whether to OPEN the modal).
- Produces: `pub fn confirm_screen(step: &Step, style: &Style) -> String` — the Ring-2 modal. Width-safe at 80, no ANSI with color off.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/conductor/src/tui/frame.rs`:

```rust
    fn ring2_step() -> Step {
        Step::new(
            "refresh-stale-data",
            "refresh stale data",
            Some("workstate snapshot".into()),
            crate::plan::Ring::ChangesState,
        )
    }

    #[test]
    fn confirm_screen_shows_command_caution_and_a_non_esc_back_path() {
        let out = confirm_screen(&ring2_step(), &plain());
        assert!(out.contains("refresh stale data"));
        assert!(out.contains("changes state"));
        assert!(out.contains("workstate snapshot"));
        assert!(out.contains("it changes suite state"));
        // y is the only spawn trigger; q is the explicit non-Esc back path.
        assert!(out.contains("y  run it"));
        assert!(out.contains("s  skip"));
        assert!(out.contains("q  back"));
    }

    #[test]
    fn confirm_screen_is_width_safe_and_color_off_has_no_escapes() {
        let out = confirm_screen(&ring2_step(), &plain());
        assert!(longest_line(&out) <= 80);
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn help_no_longer_mentions_phase_3() {
        let out = help_screen(&plain());
        assert!(!out.contains("Phase 3"));
        assert!(out.contains("changes-state steps ask first"));
    }
```

Also, in this same test module, change the two existing tests that pass the literal `"needs Phase 3 — not run"` notice (currently `plan_screen_renders_notice_line_when_present` at ~line 210-214 and `frames_fit_80_columns_with_color_off` at ~line 234-246) to use a neutral notice string instead — they only assert the notice line renders, the wording is cosmetic:

In `plan_screen_renders_notice_line_when_present`:

```rust
    #[test]
    fn plan_screen_renders_notice_line_when_present() {
        let p = sample_plan();
        let out = plan_screen(&p, 0, Some("a step failed — exit 1"), &plain());
        assert!(out.contains("a step failed — exit 1"));
    }
```

In `frames_fit_80_columns_with_color_off`, replace the `Some("needs Phase 3 — not run")` argument (it appears once) with `Some("a step failed — exit 1")`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --lib tui::frame 2>&1 | head -30`
Expected: FAIL — `confirm_screen` undefined; `help_no_longer_mentions_phase_3` fails (help still says "needs Phase 3").

- [ ] **Step 3: Add `confirm_screen`**

In `crates/conductor/src/tui/frame.rs`, add this function after `compact_plan` (after line 150) and before `help_screen`:

```rust
/// The Ring-2 confirm modal. Shows the step title with its `changes state` tag,
/// the LITERAL command on its own line, a one-line caution, and the explicit
/// `y` / `s` / `q` strip. `y` is the only spawn trigger; `q` (and Esc) backs out
/// — there is no Esc-only path. The command shown is the step's own command,
/// exactly what `run.rs` will spawn.
pub fn confirm_screen(step: &Step, style: &Style) -> String {
    let cmd = step.command.as_deref().unwrap_or("(no command)");
    format!(
        "\n   {mc}▸{rst} {title}   {rc}{tag}{rst}\n\n        this will run:  {dim}{cmd}{rst}\n        {ylw}it changes suite state.{rst}\n\n        y  run it        s  skip        q  back to plan\n",
        mc = style.current_marker(),
        rst = style.rst,
        title = step.title,
        rc = style.ring_color(step.ring),
        tag = step.ring.tag(),
        dim = style.dim,
        cmd = cmd,
        ylw = style.ylw,
    )
}
```

- [ ] **Step 4: Fix `help_screen`**

In `crates/conductor/src/tui/frame.rs`, change `help_screen` (lines 153-159) to drop the "needs Phase 3" caveat:

```rust
/// The help screen: every key with a one-line description.
pub fn help_screen(style: &Style) -> String {
    format!(
        " {b}keys{r}\n   enter  run the current step (read-only runs; changes-state steps ask first)\n   s      skip the current step\n   a      advance focus without running\n   r      hand off to the rexops cockpit\n   ?      toggle this help\n   q      quit\n",
        b = style.bold,
        r = style.rst,
    )
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p conductor --lib tui::frame`
Expected: PASS (the new modal tests, the help test, and the two reworded notice tests).

- [ ] **Step 6: Full crate tests + lint + fmt**

Run: `cargo test -p conductor && cargo clippy -p conductor --all-targets -- -D warnings && cargo fmt -p conductor -- --check`
Expected: PASS. (The Task-2 `#[ignore]`d test stays ignored; it is deleted in Task 4.)

- [ ] **Step 7: Commit (after explicit human approval)**

```bash
git add crates/conductor/src/tui/frame.rs
git commit -m "feat(conductor): tui/frame — Ring-2 confirm modal + scrub Phase-3 help caveat"
```

---

### Task 4: `tui/mod.rs` — the confirm gate, Failed handling, and `RunReport`

**Files:**
- Modify: `crates/conductor/src/tui/mod.rs` (add `Screen::Confirm`; the gate in `step`; uniform outcome handling + Failed in `run_current`; `RunReport`; `run` returns it; new tests; delete the Phase-2 no-op test)
- Test: inline `#[cfg(test)] mod tests` in `tui/mod.rs`

**Interfaces:**
- Consumes: `confirm_command` and `confirm_screen` (Tasks 2/3), `StepStatus::Failed` (Task 1), `run_step`/`RunOutcome` (Task 2).
- Produces:
  - `pub enum Screen { Plan, Help, Confirm }`
  - `pub struct RunReport { pub failed: usize, pub unfinished: usize }` with `pub fn exit_code(&self) -> u8`.
  - `pub fn run(plan: Plan, force_no_color: bool) -> std::io::Result<RunReport>` (was `-> io::Result<()>`).
  - `step(...)` unchanged signature; new behavior: Enter on a `ChangesState` step opens `Screen::Confirm`; Confirm-screen key handling.

- [ ] **Step 1: Write the failing tests**

In `crates/conductor/src/tui/mod.rs`, first DELETE the Phase-2 test `enter_on_ring2_is_a_noop_with_note_and_no_spawn` (currently lines 295-308 — the one marked `#[ignore]` in Task 2). Then add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn enter_on_ring2_opens_confirm_and_spawns_nothing() {
        let mut app = AppState::new(sample()); // step 0 is the Ring2 refresh
        let sp = FakeSpawner::new();
        let action = step(&mut app, Key::Enter, &sp);
        assert_eq!(action, Action::Redraw);
        assert_eq!(app.screen, Screen::Confirm);
        assert_eq!(app.plan.steps[0].status, StepStatus::Pending);
        assert_eq!(app.cursor, 0);
        assert!(sp.calls.borrow().is_empty(), "opening confirm must not spawn");
    }

    #[test]
    fn confirm_y_spawns_once_marks_done_and_returns_to_plan() {
        // Stub `workstate` on PATH so the Ring-2 spawn passes the availability check.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("workstate");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let new_path = match &orig {
            Some(p) => {
                let mut v = std::ffi::OsString::from(dir.path());
                v.push(":");
                v.push(p);
                v
            }
            None => std::ffi::OsString::from(dir.path()),
        };
        std::env::set_var("PATH", &new_path);

        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp); // open confirm
        assert_eq!(app.screen, Screen::Confirm);
        step(&mut app, Key::Char('y'), &sp); // confirm + run
        assert_eq!(app.screen, Screen::Plan);
        assert_eq!(app.plan.steps[0].status, StepStatus::Done);
        assert_eq!(app.cursor, 1, "a successful run advances");
        assert_eq!(sp.calls.borrow().len(), 1);
        assert_eq!(sp.calls.borrow()[0][0], "workstate");

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn confirm_non_y_keys_never_spawn_and_back_out() {
        // q, Esc, Enter, and an arbitrary key all decline — the central safety
        // property: a stray Enter cannot fire a state change.
        for k in [Key::Char('q'), Key::Esc, Key::Enter, Key::Char('z')] {
            let mut app = AppState::new(sample());
            let sp = FakeSpawner::new();
            step(&mut app, Key::Enter, &sp); // open confirm
            assert_eq!(app.screen, Screen::Confirm);
            step(&mut app, k, &sp); // decline
            assert_eq!(app.screen, Screen::Plan, "{k:?} must return to plan");
            assert_eq!(app.plan.steps[0].status, StepStatus::Pending);
            assert_eq!(app.cursor, 0, "{k:?} must not advance");
            assert!(sp.calls.borrow().is_empty(), "{k:?} must not spawn");
        }
    }

    #[test]
    fn confirm_s_skips_and_advances_without_spawning() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp); // open confirm
        step(&mut app, Key::Char('s'), &sp); // skip from the modal
        assert_eq!(app.screen, Screen::Plan);
        assert_eq!(app.plan.steps[0].status, StepStatus::Skipped);
        assert_eq!(app.cursor, 1);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn a_failing_spawn_marks_failed_and_stays_put() {
        // Stub `bulwark` that exits non-zero; the Ring-1 investigate step then fails.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
        std::fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let new_path = match &orig {
            Some(p) => {
                let mut v = std::ffi::OsString::from(dir.path());
                v.push(":");
                v.push(p);
                v
            }
            None => std::ffi::OsString::from(dir.path()),
        };
        std::env::set_var("PATH", &new_path);

        let mut app = AppState::new(sample());
        let last = app.plan.steps.len() - 1; // the Ring-1 investigate step
        app.cursor = last;
        let sp = ExitSpawner { success: false };
        step(&mut app, Key::Enter, &sp);
        assert_eq!(app.plan.steps[last].status, StepStatus::Failed);
        assert_eq!(app.cursor, last, "a failed step does not advance");
        assert!(app.notice.is_some());

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn run_report_maps_statuses_to_exit_codes() {
        // all done -> 0
        assert_eq!(RunReport { failed: 0, unfinished: 0 }.exit_code(), 0);
        // a failure -> 1, and failure beats unfinished
        assert_eq!(RunReport { failed: 1, unfinished: 3 }.exit_code(), 1);
        // unfinished, none failed -> 2
        assert_eq!(RunReport { failed: 0, unfinished: 2 }.exit_code(), 2);
    }

    #[test]
    fn report_from_plan_counts_failed_and_unfinished() {
        let mut app = AppState::new(sample());
        // mark: step0 Failed, step1 Skipped, leave the rest Pending.
        app.plan.steps[0].status = StepStatus::Failed;
        app.plan.steps[1].status = StepStatus::Skipped;
        let r = report_from(&app.plan);
        assert_eq!(r.failed, 1);
        assert!(r.unfinished >= 1, "skipped + any pending count as unfinished");
        assert_eq!(r.exit_code(), 1, "any failure outranks unfinished");
    }
```

Also add this second fake spawner near `FakeSpawner` in the test module (it returns a chosen exit status WITHOUT forking, mirroring run.rs's TestSpawner):

```rust
    /// A spawner that fabricates a chosen exit status without forking.
    struct ExitSpawner {
        success: bool,
    }
    impl Spawner for ExitSpawner {
        fn spawn(&self, _argv: &[String]) -> std::io::Result<ExitStatus> {
            use std::os::unix::process::ExitStatusExt;
            Ok(ExitStatus::from_raw(if self.success { 0 } else { 1 << 8 }))
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --lib tui::tests 2>&1 | head -30`
Expected: FAIL — `Screen::Confirm` undefined, `RunReport` undefined, `report_from` undefined, behavior mismatches.

- [ ] **Step 3: Add `Screen::Confirm` and `RunReport`**

In `crates/conductor/src/tui/mod.rs`, change the `Screen` enum (lines 23-27) to:

```rust
/// Which screen is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Plan,
    Help,
    /// The Ring-2 confirm modal for the cursor step. `y` runs it; anything else
    /// (incl. Enter) backs out without running.
    Confirm,
}
```

Add, just after the `Action` enum (after line 34):

```rust
/// The outcome of a guided run, mapped to a process exit code. `failed` counts
/// steps whose delegated command exited non-zero; `unfinished` counts steps left
/// Pending or Skipped. A failure outranks unfinished.
pub struct RunReport {
    pub failed: usize,
    pub unfinished: usize,
}

impl RunReport {
    pub fn exit_code(&self) -> u8 {
        if self.failed > 0 {
            1
        } else if self.unfinished > 0 {
            2
        } else {
            0
        }
    }
}

/// Tally a plan's final step statuses into a `RunReport`.
pub fn report_from(plan: &Plan) -> RunReport {
    let mut failed = 0;
    let mut unfinished = 0;
    for s in &plan.steps {
        match s.status {
            StepStatus::Failed => failed += 1,
            StepStatus::Pending | StepStatus::Skipped => unfinished += 1,
            StepStatus::Done => {}
        }
    }
    RunReport { failed, unfinished }
}
```

- [ ] **Step 4: Add the confirm branch to `step` and rework `run_current`**

In `crates/conductor/src/tui/mod.rs`, in `step` (lines 66-113): add a `Screen::Confirm` branch BEFORE the `Screen::Help` block, and change the `Key::Enter` arm of the main (Plan) match to open the modal for Ring-2. Replace the body of `step` from the help-block down through the main match with:

```rust
    // Any key clears a stale notice first; specific arms may set a fresh one.
    app.notice = None;

    if app.screen == Screen::Confirm {
        return confirm_key(app, key, spawner);
    }

    if app.screen == Screen::Help {
        // In help, any key returns to the plan (q still quits).
        match key {
            Key::Char('q') | Key::Eof => return Action::Quit,
            _ => {
                app.screen = Screen::Plan;
                return Action::Redraw;
            }
        }
    }

    match key {
        Key::Char('q') | Key::Eof | Key::Esc => Action::Quit,
        Key::Char('?') => {
            app.screen = Screen::Help;
            Action::Redraw
        }
        Key::Char('a') => {
            app.advance();
            Action::Redraw
        }
        Key::Char('s') => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Skipped;
            }
            app.advance();
            Action::Redraw
        }
        Key::Char('r') => {
            // Hand off to the rexops cockpit if present; else a dim note.
            if crate::sources::is_on_path("rexops") {
                let _ = spawner.spawn(&["rexops".to_string(), "tui".to_string()]);
            } else {
                app.notice = Some("rexops is not on PATH".to_string());
            }
            Action::Redraw
        }
        Key::Enter => {
            // A changes-state step never fires on Enter — it opens the confirm.
            // Every other runnable step runs immediately.
            let opens_confirm = app
                .plan
                .steps
                .get(app.cursor)
                .map(|s| s.ring == Ring::ChangesState && crate::run::confirm_command(s).is_some())
                .unwrap_or(false);
            if opens_confirm {
                app.screen = Screen::Confirm;
            } else {
                run_current(app, spawner);
            }
            Action::Redraw
        }
        _ => Action::Redraw,
    }
}

/// Handle a key while the Ring-2 confirm modal is showing. `y` runs the cursor
/// step (the ONLY spawn trigger); `s` skips it; anything else — including a stray
/// Enter — backs out to the plan without running. This is the core safety gate.
fn confirm_key(app: &mut AppState, key: Key, spawner: &dyn Spawner) -> Action {
    match key {
        Key::Char('y') => {
            run_current(app, spawner);
            app.screen = Screen::Plan;
            Action::Redraw
        }
        Key::Char('s') => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Skipped;
            }
            app.advance();
            app.screen = Screen::Plan;
            Action::Redraw
        }
        // q, Esc, Enter, any other key: decline, run nothing, back to the plan.
        _ => {
            app.screen = Screen::Plan;
            Action::Redraw
        }
    }
}
```

Then change `run_current` (currently lines 117-141) to mark `Failed` on a non-zero exit and drop the removed `RefusedChangesState` arm:

```rust
/// Run the focused step (Enter, or `y` in the confirm). On success: mark Done +
/// advance. On a non-zero exit: mark Failed + stay (the operator can retry or
/// skip). Unavailable / Info produce a note and stay put. The Ring-2 gate has
/// already happened by the time this is called for a changes-state step.
fn run_current(app: &mut AppState, spawner: &dyn Spawner) {
    let Some(step_ref) = app.plan.steps.get(app.cursor) else {
        return;
    };
    let ring = step_ref.ring;
    match run_step(step_ref, spawner) {
        RunOutcome::Ran(true) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Done;
            }
            app.advance();
        }
        RunOutcome::Ran(false) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Failed;
            }
            app.notice = Some("that step failed (the tool exited non-zero)".to_string());
        }
        RunOutcome::NotAvailable(bin) => {
            app.notice = Some(format!("{bin} is not on PATH — install it first"));
        }
        RunOutcome::NotRunnable => {
            if ring == Ring::Info {
                app.notice = Some("informational — run the shown command yourself".to_string());
            }
        }
    }
}
```

- [ ] **Step 5: Make `render` paint the modal and `run` return a `RunReport`**

In `crates/conductor/src/tui/mod.rs`, change `render` (lines 173-182) to add the Confirm screen:

```rust
/// Render the current frame for `app` at the current terminal size.
fn render(app: &AppState, style: &crate::tui::style::Style) -> String {
    let (cols, rows) = term_size();
    if app.screen == Screen::Help {
        return crate::tui::frame::help_screen(style);
    }
    if app.screen == Screen::Confirm {
        if let Some(step) = app.plan.steps.get(app.cursor) {
            return crate::tui::frame::confirm_screen(step, style);
        }
    }
    if cols < 80 || rows < 24 {
        return crate::tui::frame::compact_plan(&app.plan, app.cursor, style);
    }
    crate::tui::frame::plan_screen(&app.plan, app.cursor, app.notice.as_deref(), style)
}
```

Change `run` (lines 200-220) to return the report:

```rust
/// Run the interactive TUI to completion. Sets up the panic guard + raw mode,
/// loops painting frames and applying keys until Quit, always restores the
/// terminal on the way out (RawMode's Drop), and returns a `RunReport` tallying
/// what the operator left behind (for the exit code).
pub fn run(plan: Plan, force_no_color: bool) -> std::io::Result<RunReport> {
    term::install_panic_guard();
    let style = crate::tui::style::Style::resolve(force_no_color);
    let mut app = AppState::new(plan);
    let mut raw = RawMode::enter()?;
    let mut stdin = io::stdin();
    loop {
        term::paint(&render(&app, &style))?;
        let key = term::read_key(&mut stdin)?;
        let action = {
            let spawner = SuspendSpawner {
                raw: RefCell::new(&mut raw),
            };
            step(&mut app, key, &spawner)
        };
        if action == Action::Quit {
            break;
        }
    }
    Ok(report_from(&app.plan))
}
```

- [ ] **Step 6: Run the tui tests to verify they pass**

Run: `cargo test -p conductor --lib tui::tests`
Expected: PASS (all new gate/report tests + the surviving Phase-2 transition tests: `q_quits`, `a_advances_focus_without_running`, `s_skips_and_advances`, `enter_on_ring1_spawns_marks_done_and_advances`, `question_toggles_help_and_any_key_returns`, `notice_clears_on_next_key`).

- [ ] **Step 7: Verify `run`'s new return type didn't break `main.rs`**

Run: `cargo build -p conductor 2>&1 | head -20`
Expected: a compile error in `crates/conductor/src/main.rs:104` — `run_bare` does `conductor::tui::run(...).map_err(...)?; Ok(ExitCode::SUCCESS)` and now gets a `RunReport` it ignores; the `?` on a value that is no longer `()` may warn/err on the unused `RunReport`. This is wired properly in Task 5. To keep Task 4 building standalone, apply the minimal change now: in `run_bare`, bind and ignore the report —

```rust
    let _report = conductor::tui::run(plan, cli.no_color)
        .map_err(|e| ConductorError::Tui(e.to_string()))?;
    Ok(ExitCode::SUCCESS)
```

Re-run: `cargo build -p conductor`
Expected: PASS. (Task 5 replaces `_report` with the real exit-code mapping.)

- [ ] **Step 8: Full crate tests + lint + fmt**

Run: `cargo test -p conductor && cargo clippy -p conductor --all-targets -- -D warnings && cargo fmt -p conductor -- --check`
Expected: PASS. No `#[ignore]`d tests remain (the Phase-2 no-op test was deleted in Step 1).

- [ ] **Step 9: Commit (after explicit human approval)**

```bash
git add crates/conductor/src/tui/mod.rs crates/conductor/src/main.rs
git commit -m "feat(conductor): tui — Ring-2 confirm gate, Failed status, RunReport exit codes"
```

---

### Task 5: `main.rs` — the `orchestrate` verb, exit-code wiring, `--dump-view confirm`, docs

**Files:**
- Modify: `crates/conductor/src/main.rs` (add `Orchestrate` to `Cmd`; map `RunReport` to exit code in `run_bare`; `confirm` view in `run_dump_view`)
- Modify: `crates/conductor/tests/cli.rs` (integration tests)
- Modify: `crates/conductor/README.md`
- Modify: `LAST_WORK.md` (repo root)
- Test: `crates/conductor/tests/cli.rs`

**Interfaces:**
- Consumes: `tui::run -> RunReport` (Task 4), `frame::confirm_screen` (Task 3).
- Produces: `conductor orchestrate` (same driver as bare); both bare and orchestrate return 0/1/2/3; `--dump-view confirm` renders the modal.

- [ ] **Step 1: Write the failing integration tests**

Add to `crates/conductor/tests/cli.rs`, reusing the existing harness. EXACT signatures already in that file (verified): `TempRoot::new(tag: &str)`, `t.write(rel, body)`, and `run(root: &TempRoot, args: &[&str]) -> std::process::Output`. NOTE: `run` ALWAYS injects `--data-dir <root>`, `--no-color`, and a stub `PATH` (all 8 suite bins read as present), and is NOT a TTY — so every `conductor` / `orchestrate` invocation here takes the non-TTY `status` fallback path.

```rust
#[test]
fn orchestrate_verb_is_listed_in_help() {
    let t = TempRoot::new("orch-help");
    let out = run(&t, &["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("orchestrate"), "help must list the orchestrate verb");
}

#[test]
fn orchestrate_json_is_non_interactive_and_matches_status() {
    // orchestrate shares the non-TTY fallback: with --json it prints the status
    // envelope and exits 0, never opening a TUI (the test harness is not a TTY).
    let t = TempRoot::new("orch-json");
    let o = run(&t, &["orchestrate", "--json"]);
    let s = run(&t, &["status", "--json"]);
    assert!(o.status.success());
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&s.stdout),
        "orchestrate --json must equal status --json"
    );
}

#[test]
fn dump_view_confirm_renders_the_ring2_modal() {
    // A stale feed yields a Ring-2 refresh step at the top; the confirm view
    // renders its modal deterministically (no PTY).
    let t = TempRoot::new("confirm-dump");
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
    );
    let out = run(&t, &["--dump-view", "confirm"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("this will run:"));
    assert!(text.contains("changes state"));
    assert!(text.contains("y  run it"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --test cli 2>&1 | head -30`
Expected: FAIL — `orchestrate` is not a known subcommand (clap errors / help lacks it); `--dump-view confirm` errors ("needs one of: plan healthy compact help").

- [ ] **Step 3: Add the `Orchestrate` verb**

In `crates/conductor/src/main.rs`, change the `Cmd` enum (lines 52-60) to:

```rust
#[derive(Subcommand)]
enum Cmd {
    /// Print the situation and the ordered plan (same as no subcommand).
    Status,
    /// Print the suite's readiness as conductor sees it (feeds + tools).
    Health,
    /// Print just the ordered steps, no situation prose.
    Plan,
    /// Walk the plan interactively, confirming each changes-state step (the
    /// driver). Same as bare `conductor` on a terminal; falls back to `status`
    /// when not a TTY or with --json.
    Orchestrate,
}
```

And add it to the dispatch `match` in `main` (lines 71-76):

```rust
    let result = match &cli.command {
        None => run_bare(&cli, &style),
        Some(Cmd::Status) => run_status(&cli, &style),
        Some(Cmd::Health) => run_health(&cli, &style),
        Some(Cmd::Plan) => run_plan(&cli, &style),
        Some(Cmd::Orchestrate) => run_bare(&cli, &style),
    };
```

- [ ] **Step 4: Map `RunReport` to the exit code in `run_bare`**

In `crates/conductor/src/main.rs`, change `run_bare` (lines 95-106) to translate the report:

```rust
/// Bare `conductor` (and `orchestrate`): open the interactive TUI on a real
/// terminal; otherwise fall back to the scriptable `status` output so pipes/CI
/// still work. The guided run's outcome becomes the exit code: 1 a step failed,
/// 2 quit with steps still pending/skipped, 0 clean / all done / nothing to do.
fn run_bare(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    if cli.json || !conductor::tui::should_run_interactive() {
        return run_status(cli, style);
    }
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    let report = conductor::tui::run(plan, cli.no_color)
        .map_err(|e| ConductorError::Tui(e.to_string()))?;
    Ok(ExitCode::from(report.exit_code()))
}
```

- [ ] **Step 5: Add the `confirm` dump view**

In `crates/conductor/src/main.rs`, in `run_dump_view` (lines 109-135), add a `"confirm"` arm to the `match view` and update the error hint. The modal targets the first step (index 0), matching how the other views use cursor 0:

```rust
    let frame = match view {
        "plan" => frame::plan_screen(&plan, 0, None, &style),
        "healthy" => frame::healthy_screen(&style),
        "compact" => frame::compact_plan(&plan, 0, &style),
        "help" => frame::help_screen(&style),
        "confirm" => match plan.steps.first() {
            Some(step) => frame::confirm_screen(step, &style),
            None => frame::healthy_screen(&style),
        },
        other => {
            eprintln!(
                "conductor: --dump-view needs one of: plan healthy compact help confirm (got {other})"
            );
            return ExitCode::from(3);
        }
    };
```

- [ ] **Step 6: Run the integration tests to verify they pass**

Run: `cargo test -p conductor --test cli`
Expected: PASS (the three new tests + all existing cli tests).

- [ ] **Step 7: Update the README**

In `crates/conductor/README.md`, in the "Interactive mode (Phase 2)" section, update it to reflect Phase 3. Find the Phase-2 heading and replace that section's body with Phase-3 reality (the implementer should read the current section first and edit in place; the new content is):

```markdown
## Interactive mode

Bare `conductor` (on a terminal) opens the interactive driver — the same thing as
`conductor orchestrate`. It shows the ordered plan and walks you through it:

- `enter` runs the current step. A **read-only** step spawns its sibling
  immediately; a **changes-state** step first opens a confirm showing the exact
  command — it runs only when you press `y` (a stray `enter` never fires a state
  change), with `s` to skip and `q` to back out.
- `s` skip · `a` advance focus · `r` hand off to the rexops cockpit · `?` help ·
  `q` quit.

Conductor still changes nothing with its own code: every state change is a
confirmed spawn of the tool that owns it, with that tool's own safety gate on top.

Exit codes for a guided run (bare `conductor` and `orchestrate`): `0` clean /
all steps done / nothing to conduct, `1` a step that ran failed, `2` you quit
with steps still pending or skipped, `3` conductor itself could not run.

When not a terminal (piped / CI) or with `--json`, both fall back to the
read-only `status` output so scripts keep working.
```

- [ ] **Step 8: Update LAST_WORK.md**

In `LAST_WORK.md` (repo root), add a new top entry (above the Phase-2 entry) summarizing Phase 3. Read the current top of the file first to match its exact heading/format; the new entry's content:

```markdown
## Conductor Phase 3 — the driver (Ring-2 + confirm modal + orchestrate)

Bare `conductor` and the new `conductor orchestrate` verb now DRIVE the plan:
Enter on a changes-state step opens a confirm modal showing the literal command;
it spawns the sibling only on `y` (a stray Enter can never fire a state change),
with `s` skip / `q` back. `run.rs` no longer refuses Ring-2 (the gate moved to
the TUI); every other guard is intact (known-binary-only, $PATH check, fixed
argv, NO shell). A step that runs and exits non-zero is marked Failed (new
StepStatus variant, ✗). The guided run returns a RunReport mapped to exit codes:
0 clean/all-done/nothing-to-conduct, 1 a step failed, 2 quit with pending/skipped
(failure outranks unfinished), 3 can't-run — for BOTH bare and orchestrate.
Non-TTY / --json still falls back to `status`. Conductor still writes zero live
files with its own code. No new dependency; rules + JSON envelope unchanged.
Tests: the full confirm-gate matrix + RunReport mapping + `--dump-view confirm`,
all green; clippy -D warnings + fmt + `cargo build --workspace` clean.
```

- [ ] **Step 9: Full-workspace verification**

Run:
```bash
cargo test -p conductor \
  && cargo clippy -p conductor --all-targets -- -D warnings \
  && cargo fmt -p conductor -- --check \
  && cargo build --workspace
```
Expected: PASS (all four). Then a binary smoke test:
```bash
cargo run -q -p conductor -- --help | grep -q orchestrate && echo "orchestrate OK"
cargo run -q -p conductor -- --dump-view confirm --data-dir /nonexistent 2>/dev/null; echo "confirm dump exit: $?"
```
Expected: prints `orchestrate OK`; the confirm dump on an empty data dir renders the healthy screen (no Ring-2 step ⇒ falls back) and exits 0.

- [ ] **Step 10: Commit (after explicit human approval)**

```bash
git add crates/conductor/src/main.rs crates/conductor/tests/cli.rs crates/conductor/README.md LAST_WORK.md
git commit -m "feat(conductor): orchestrate verb + exit-code wiring + --dump-view confirm + docs"
```

---

## Final verification (after all 5 tasks)

Run the whole gate once more from a clean tree:

```bash
cargo test -p conductor \
  && cargo clippy -p conductor --all-targets -- -D warnings \
  && cargo fmt -p conductor -- --check \
  && cargo build --workspace
```

All must pass. Then confirm the safety invariants by inspection:
- `grep -rn "RefusedChangesState" crates/conductor/` → no matches (fully removed).
- No `std::process::Command::new` with a shell (`sh -c`) anywhere in `run.rs`.
- The confirm modal's command comes from the step's own `command` field
  (`confirm_command`), the same source `run_step` spawns.

## Definition of done (mirrors the spec)

- [ ] Enter on a Ring-2 step opens the confirm modal and runs nothing; only `y`
      spawns; `q`/Esc/Enter/other keys back out without running.
- [ ] A confirmed Ring-2 step spawns the known binary (foreground, fixed argv, no
      shell) and, on success, marks `Done` + advances; on non-zero exit marks
      `Failed` + stays. Ring-1 follows the same Done/Failed rule.
- [ ] `conductor orchestrate` opens the same TUI as bare `conductor`; non-TTY /
      `--json` falls back to `status`; the verb is listed in `--help`.
- [ ] Exit codes 0/1/2/3 as specified (failure outranks unfinished) for BOTH
      bare `conductor` and `orchestrate`.
- [ ] `--dump-view confirm` renders the modal; help no longer says "needs
      Phase 3"; every frame width-safe and NO_COLOR-legible.
- [ ] Conductor writes zero live files; only known binaries spawnable; no shell;
      a finding id stays one argv token. All re-tested.
- [ ] Tests green; clippy `-D warnings` clean; `cargo fmt --check` clean;
      `cargo build --workspace` clean.
- [ ] README + LAST_WORK.md updated. Nothing committed/pushed without explicit
      human approval.
