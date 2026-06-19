# Conductor Phase 2 (TUI + Ring-1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make bare `conductor` open an interactive TUI that renders the plan and runs the read-only (Ring-1) steps, while Ring-2 steps render but are a no-op-with-note.

**Architecture:** Hand-rolled, dependency-free terminal driver ported from `pulse/src/tui.rs` (raw mode + suspend-for-child + key reader + panic guard). Pure String frame renderers (snapshot-testable, no PTY). A single subprocess choke point (`run.rs`) behind a `Spawner` trait that refuses Ring-2. The event loop wires keys to step-status transitions and chooses which frame to paint.

**Tech Stack:** Rust, std only (no new third-party dep), `clap` (already present), `serde`/`serde_json`/`chrono` (already present). Reuses Phase 1: `conductor::load_state`, `conductor::plan::build`, `Plan`/`Step`/`Ring`/`StepStatus`.

## Global Constraints

- No new third-party dependency. Terminal driver is hand-rolled std + `extern "C"` termios (copy pulse). Bar: std only.
- Conductor writes ZERO live files and executes ZERO Ring-2 commands in its own code. Ever.
- No `r-conductor` wrapper, no shell alias. Bare binary named `conductor`.
- `NO_COLOR` set OR non-TTY ⇒ monochrome. State is carried by word + glyph, never color alone.
- Compact (<80×24) fallback must not clip; no rendered line may exceed the viewport.
- No Escape-only flow; `q` always quits.
- The command shown for a step is EXACTLY what `run.rs` would spawn. Never assemble a command from feed content beyond passing an id as a single argv token. No shell.
- Exit codes: `0` ok, `3` conductor itself could not run. `1`/`2` are RESERVED for Phase 3 — do not emit them.
- Per task: `cargo test -p conductor` + `cargo clippy -p conductor --all-targets -- -D warnings` + `cargo fmt -p conductor -- --check` must pass before commit. Per-task commits. NOTHING committed/pushed without explicit human approval.
- Phase 1 rule semantics and the JSON envelope shape are FROZEN — do not change `plan/rules.rs` or `report.rs` envelope fields.

---

### Task 1: Port the dependency-free terminal driver (`tui/term.rs`)

**Files:**
- Create: `crates/conductor/src/tui/term.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod tui;`)
- Create: `crates/conductor/src/tui/mod.rs` (temporary minimal: `pub mod term;` — fleshed out in Task 5)
- Test: inline `#[cfg(test)] mod tests` in `term.rs`

**Interfaces:**
- Consumes: nothing (std only).
- Produces:
  - `pub struct RawMode` with `pub fn enter() -> std::io::Result<RawMode>`, `pub fn suspend<T>(&mut self, body: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T>`, and `Drop` that restores the terminal.
  - `pub fn install_panic_guard()`
  - `pub enum Key { Char(char), Enter, Esc, Backspace, Eof, Other }`
  - `pub fn read_key(input: &mut impl std::io::Read) -> std::io::Result<Key>`
  - `pub fn paint(frame: &str) -> std::io::Result<()>`

- [ ] **Step 1: Create the module wiring**

Create `crates/conductor/src/tui/mod.rs` with exactly:

```rust
//! Conductor's interactive TUI. Dependency-free, modeled on pulse: a hand-rolled
//! terminal driver (`term`), pure frame renderers (`frame`), a color resolver
//! (`style`), and the event loop (this module, added in a later task).

pub mod term;
```

Add to `crates/conductor/src/lib.rs` after the existing `pub mod state;` line:

```rust
pub mod tui;
```

- [ ] **Step 2: Write `term.rs` by porting pulse's driver**

Create `crates/conductor/src/tui/term.rs` with the full contents of `crates/pulse/src/tui.rs` (read that file and copy it verbatim), with one doc-comment edit: change the opening line `//! Minimal, dependency-free terminal driver for Pulse's interactive mode.` to `//! Minimal, dependency-free terminal driver for Conductor's interactive mode.` Keep everything else — `RawMode`, `install_panic_guard`, `Key`, `read_key`, `decode_utf8`, `paint`, the `extern "C"` termios block, and the entire `#[cfg(test)] mod tests` — identical. The driver and its tests are proven; do not rewrite them.

- [ ] **Step 3: Run the ported tests to verify they pass**

Run: `cargo test -p conductor term::tests 2>&1 | tail -20`
Expected: the ported tests pass — `decodes_plain_keys`, `decodes_enter_and_backspace`, `lone_escape_is_esc`, `arrow_key_sequence_is_swallowed_as_other_not_esc_plus_letters`, `ctrl_d_is_eof`, `decodes_multibyte_utf8`, `raw_and_cooked_flags_are_inverses_on_the_lflags` all `ok`.

- [ ] **Step 4: Lint and format**

Run: `cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -15`
Expected: no warnings. (If clippy flags `paint`/`suspend` as unused at this stage, add `#[allow(dead_code)]` on the specific item with a `// wired up in Task 5` comment — they are consumed by the event loop later.)

Run: `cargo fmt -p conductor -- --check`
Expected: no output (already formatted, since it is copied from formatted pulse).

- [ ] **Step 5: Commit**

```bash
git add crates/conductor/src/tui/term.rs crates/conductor/src/tui/mod.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): tui/term — port pulse's dependency-free terminal driver

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: TUI color resolver (`tui/style.rs`)

**Files:**
- Create: `crates/conductor/src/tui/style.rs`
- Modify: `crates/conductor/src/tui/mod.rs` (add `pub mod style;`)
- Test: inline `#[cfg(test)] mod tests` in `style.rs`

**Interfaces:**
- Consumes: `conductor::plan::Ring`, `conductor::state::Severity`, `conductor::util::stdout_is_tty`.
- Produces:
  - `pub struct Style { pub bold, pub dim, pub red, pub grn, pub ylw, pub cyn, pub rst: &'static str }`
  - `pub fn resolve(force_off: bool) -> Style`
  - `pub fn ring_color(&self, ring: Ring) -> &'static str`
  - `pub fn severity_color(&self, sev: Severity) -> &'static str`
  - `pub fn current_marker(&self) -> &'static str` (cyan `▸` focus marker, empty-colored when off)

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/tui/style.rs`:

```rust
//! Color resolver for the interactive TUI. Same discipline as report::Style and
//! pulse: color on iff stdout is a TTY and NO_COLOR is unset (or forced off);
//! every field is an empty string when off, so call sites interpolate
//! unconditionally and the frame reads identically with color stripped. State is
//! always carried by word and glyph — color is only ever a bonus.

use crate::plan::Ring;
use crate::state::Severity;
use crate::util;

/// Resolved ANSI styling. Empty strings when color is off.
pub struct Style {
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub grn: &'static str,
    pub ylw: &'static str,
    pub cyn: &'static str,
    pub rst: &'static str,
}

impl Style {
    /// Resolve styling. `force_off` (e.g. `--no-color`) wins; otherwise color is
    /// on only for a real TTY without `NO_COLOR`.
    pub fn resolve(force_off: bool) -> Self {
        let on = !force_off && util::stdout_is_tty() && std::env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                red: "\u{1b}[31m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Style {
                bold: "",
                dim: "",
                red: "",
                grn: "",
                ylw: "",
                cyn: "",
                rst: "",
            }
        }
    }

    /// Amber for a state-changing ring; dim for read-only/info.
    pub fn ring_color(&self, ring: Ring) -> &'static str {
        match ring {
            Ring::ChangesState => self.ylw,
            Ring::ReadOnly | Ring::Info => self.dim,
        }
    }

    /// Red for critical/high; amber for medium; dim for low.
    pub fn severity_color(&self, sev: Severity) -> &'static str {
        match sev {
            Severity::Critical | Severity::High => self.red,
            Severity::Medium => self.ylw,
            Severity::Low => self.dim,
        }
    }

    /// The cyan focus color used for the current-step `▸` marker.
    pub fn current_marker(&self) -> &'static str {
        self.cyn
    }

    #[cfg(test)]
    fn plain() -> Self {
        Self::resolve(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_off_emits_no_escapes() {
        let s = Style::plain();
        assert_eq!(s.bold, "");
        assert_eq!(s.cyn, "");
        assert_eq!(s.ring_color(Ring::ChangesState), "");
        assert_eq!(s.severity_color(Severity::Critical), "");
        assert_eq!(s.current_marker(), "");
    }

    #[test]
    fn ring_color_maps_state_change_to_amber_when_on() {
        // Build an "on" style directly to test the mapping without a TTY.
        let on = Style {
            bold: "B", dim: "D", red: "R", grn: "G", ylw: "Y", cyn: "C", rst: "0",
        };
        assert_eq!(on.ring_color(Ring::ChangesState), "Y");
        assert_eq!(on.ring_color(Ring::ReadOnly), "D");
        assert_eq!(on.ring_color(Ring::Info), "D");
    }

    #[test]
    fn severity_color_buckets_match_design() {
        let on = Style {
            bold: "B", dim: "D", red: "R", grn: "G", ylw: "Y", cyn: "C", rst: "0",
        };
        assert_eq!(on.severity_color(Severity::Critical), "R");
        assert_eq!(on.severity_color(Severity::High), "R");
        assert_eq!(on.severity_color(Severity::Medium), "Y");
        assert_eq!(on.severity_color(Severity::Low), "D");
    }
}
```

Add to `crates/conductor/src/tui/mod.rs`:

```rust
pub mod style;
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p conductor style::tests 2>&1 | tail -15`
Expected: `forced_off_emits_no_escapes`, `ring_color_maps_state_change_to_amber_when_on`, `severity_color_buckets_match_design` all `ok`.

Note: `Severity` must derive `Clone, Copy` for `severity_color(sev)` by value to be ergonomic. Verify with `grep -n "pub enum Severity" -A1 crates/conductor/src/state.rs`. If it is not `Copy`, change the param to `&Severity` and match on `*sev` (do NOT add a derive to the frozen Phase-1 type unless it already has one).

- [ ] **Step 3: Lint and format**

Run: `cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -15`
Expected: no warnings. (clippy may warn `current_marker`/`severity_color` unused until Task 3 — add `#[allow(dead_code)] // used by frame.rs in Task 3` on the specific method if so.)

Run: `cargo fmt -p conductor -- --check`
Expected: no output. If it reformats, run `cargo fmt -p conductor` and re-check.

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/tui/style.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): tui/style — color resolver (ring/severity/focus), legible with color off

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Pure frame renderers (`tui/frame.rs`)

**Files:**
- Create: `crates/conductor/src/tui/frame.rs`
- Modify: `crates/conductor/src/tui/mod.rs` (add `pub mod frame;`)
- Test: inline `#[cfg(test)] mod tests` in `frame.rs`

**Interfaces:**
- Consumes: `conductor::plan::{Plan, Step, Ring, StepStatus}`, `tui::style::Style`.
- Produces (all pure, no I/O):
  - `pub fn plan_screen(plan: &Plan, cursor: usize, notice: Option<&str>, style: &Style) -> String`
  - `pub fn healthy_screen(style: &Style) -> String`
  - `pub fn compact_plan(plan: &Plan, cursor: usize, style: &Style) -> String`
  - `pub fn help_screen(style: &Style) -> String`
  - `pub const HINT: &str` (the one-line key strip)

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/tui/frame.rs`:

```rust
//! Pure frame renderers for the interactive TUI: model in, String out, no I/O.
//! Everything the event loop paints is built here, so every screen is
//! snapshot-testable without a PTY. Layout mirrors CONDUCTOR_DESIGN.md ("The
//! plan" and "All clear"). The current step is `▸`, pending `○`, done `✓`,
//! skipped `·`; the right-edge tag is the ring word; the command is shown
//! verbatim under each step. Color (via `Style`) is always optional — the words
//! and glyphs carry every distinction.

use crate::plan::{Plan, Ring, Step, StepStatus};
use crate::tui::style::Style;

/// The one-line key-hint strip shown at the foot of the plan screen.
pub const HINT: &str =
    "enter  run step    s  skip    a  advance    r  rexops    ?  help    q  quit";

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
    }
}

/// Render one step block: the marker line (glyph, number, title, optional ring
/// tag, optional correlation annotation) then the dim command line.
fn render_step(out: &mut String, n: usize, step: &Step, is_current: bool, style: &Style) {
    let g = glyph(step.status, is_current);
    let marker_color = if is_current { style.current_marker() } else { "" };
    let marker_rst = if is_current { style.rst } else { "" };
    out.push_str(&format!(
        "  {mc}{g}{mr} {n}  {title}",
        mc = marker_color,
        g = g,
        mr = marker_rst,
        n = n,
        title = step.title,
    ));
    if let Some(note) = &step.annotation {
        out.push_str(&format!("  {}← {}{}", style.cyn, note, style.rst));
    }
    // The ring tag rides at the end of the title line (right-edge in the design;
    // kept inline here so it never clips at narrow widths).
    out.push_str(&format!(
        "  {rc}{tag}{rst}",
        rc = style.ring_color(step.ring),
        tag = step.ring.tag(),
        rst = style.rst,
    ));
    out.push('\n');
    if let Some(cmd) = &step.command {
        out.push_str(&format!("       {dim}{cmd}{rst}\n", dim = style.dim, cmd = cmd, rst = style.rst));
    }
}

/// The healthy "nothing to conduct" screen — Conductor's voice for an empty plan.
pub fn healthy_screen(style: &Style) -> String {
    format!(
        "\n\n\n                             {g}nothing to conduct{r}\n\n                          the suite is healthy and\n                           every feed is current\n",
        g = style.grn,
        r = style.rst,
    )
}

/// The full plan screen: situation, the ordered steps with the current one
/// marked, an optional transient notice line, and the key-hint strip.
pub fn plan_screen(plan: &Plan, cursor: usize, notice: Option<&str>, style: &Style) -> String {
    if plan.is_empty() {
        return healthy_screen(style);
    }
    let mut out = String::new();
    out.push_str(&format!(" {b}conductor{r}\n\n", b = style.bold, r = style.rst));
    if !plan.situation.is_empty() {
        out.push_str(&format!("   {b}the situation{r}\n", b = style.bold, r = style.rst));
        for line in &plan.situation {
            out.push_str(&format!("   {line}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "   {b}the plan{r}   {n} steps\n",
        b = style.bold,
        r = style.rst,
        n = plan.steps.len()
    ));
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, i == cursor, style);
    }
    out.push('\n');
    if let Some(msg) = notice {
        out.push_str(&format!(" {dim}{msg}{rst}\n", dim = style.dim, msg = msg, rst = style.rst));
    }
    out.push_str(&format!(" {dim}{HINT}{rst}\n", dim = style.dim, HINT = HINT, rst = style.rst));
    out
}

/// The compact (<80×24) fallback: a plain unpadded list that cannot clip — title
/// line then command line per step, with a one-letter current marker.
pub fn compact_plan(plan: &Plan, cursor: usize, style: &Style) -> String {
    if plan.is_empty() {
        return format!("{g}nothing to conduct{r}\n", g = style.grn, r = style.rst);
    }
    let mut out = String::new();
    for (i, step) in plan.steps.iter().enumerate() {
        let g = glyph(step.status, i == cursor);
        out.push_str(&format!("{g} {n} {t} [{tag}]\n", g = g, n = i + 1, t = step.title, tag = step.ring.tag()));
        if let Some(cmd) = &step.command {
            out.push_str(&format!("    {cmd}\n", cmd = cmd));
        }
    }
    out.push_str("enter run  s skip  a next  q quit\n");
    out
}

/// The help screen: every key with a one-line description.
pub fn help_screen(style: &Style) -> String {
    format!(
        " {b}keys{r}\n   enter  run the current step (read-only runs; changes-state needs Phase 3)\n   s      skip the current step\n   a      advance focus without running\n   r      hand off to the rexops cockpit\n   ?      toggle this help\n   q      quit\n",
        b = style.bold,
        r = style.rst,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};

    fn plain() -> Style {
        Style::resolve(true)
    }

    /// A non-trivial plan: a stale-feed refresh (Ring 2), a safety capture
    /// (Ring 2), and an investigate step (Ring 1) — exercises every ring + the
    /// situation block.
    fn sample_plan() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "AWS key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    fn longest_line(frame: &str) -> usize {
        frame.lines().map(|l| l.chars().count()).max().unwrap_or(0)
    }

    #[test]
    fn plan_screen_shows_steps_commands_and_ring_tags() {
        let p = sample_plan();
        let out = plan_screen(&p, 0, None, &plain());
        assert!(out.contains("the plan"));
        assert!(out.contains("workstate snapshot"));
        assert!(out.contains("changes state"));
        assert!(out.contains("bulwark show deploy-prod.sh"));
        assert!(out.contains("read-only"));
        // current marker on step 1
        assert!(out.contains("▸ 1"));
        // pending marker on a later step
        assert!(out.contains("○ "));
        assert!(out.contains(HINT));
    }

    #[test]
    fn plan_screen_renders_notice_line_when_present() {
        let p = sample_plan();
        let out = plan_screen(&p, 0, Some("needs Phase 3 — not run"), &plain());
        assert!(out.contains("needs Phase 3 — not run"));
    }

    #[test]
    fn healthy_screen_speaks_conductors_voice() {
        let out = healthy_screen(&plain());
        assert!(out.contains("nothing to conduct"));
        assert!(out.contains("the suite is healthy"));
        assert!(!out.contains("the plan"));
    }

    #[test]
    fn no_color_frames_have_no_escapes() {
        let p = sample_plan();
        assert!(!plan_screen(&p, 0, Some("x"), &plain()).contains('\u{1b}'));
        assert!(!healthy_screen(&plain()).contains('\u{1b}'));
        assert!(!compact_plan(&p, 0, &plain()).contains('\u{1b}'));
        assert!(!help_screen(&plain()).contains('\u{1b}'));
    }

    #[test]
    fn frames_fit_80_columns_with_color_off() {
        let p = sample_plan();
        assert!(longest_line(&plan_screen(&p, 0, Some("needs Phase 3 — not run"), &plain())) <= 80);
        assert!(longest_line(&healthy_screen(&plain())) <= 80);
        assert!(longest_line(&help_screen(&plain())) <= 80);
    }

    #[test]
    fn compact_plan_is_narrow_and_lists_every_step() {
        let p = sample_plan();
        let out = compact_plan(&p, 1, &plain());
        // current marker now on step 2
        assert!(out.contains("▸ 2"));
        assert!(out.contains("workstate snapshot"));
        assert!(longest_line(&out) <= 60, "compact must stay narrow: {out}");
    }
}
```

Add to `crates/conductor/src/tui/mod.rs`:

```rust
pub mod frame;
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p conductor frame::tests 2>&1 | tail -25`
Expected: all six frame tests `ok`. If `frames_fit_80_columns_with_color_off` fails on the situation line (e.g. a long sentence from the rules), shorten the indentation in `plan_screen` (situation uses `   ` = 3 spaces) — do NOT change rule text. If a command line itself exceeds 80 at 80-col, that is acceptable only in the full screen (the compact fallback is the narrow guarantee); adjust the test to assert the structural lines if a real rule command is inherently long, and document why.

- [ ] **Step 3: Lint and format**

Run: `cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -15`
Expected: no warnings. (clippy commonly flags the `format!` with named args where positional is simpler, and unused `Ring` import if a path is unused — fix as flagged.)

Run: `cargo fmt -p conductor -- --check`
Expected: no output. Run `cargo fmt -p conductor` if it reformats, then re-check.

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/tui/frame.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): tui/frame — pure plan/healthy/compact/help renderers (snapshot + width tested)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: The Ring-1 spawn choke point (`run.rs`)

**Files:**
- Create: `crates/conductor/src/run.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod run;`)
- Modify: `crates/conductor/src/sources.rs` (promote `which` to `pub fn is_on_path`)
- Test: inline `#[cfg(test)] mod tests` in `run.rs`

**Interfaces:**
- Consumes: `conductor::plan::{Step, Ring}`, `conductor::sources::{is_on_path, SUITE_BINARIES}`.
- Produces:
  - `pub trait Spawner { fn spawn(&self, argv: &[String]) -> std::io::Result<std::process::ExitStatus>; }`
  - `pub struct RealSpawner;` implementing `Spawner` (direct exec, no shell).
  - `pub enum RunOutcome { Ran(bool /* success */), NotAvailable(String /* bin name */), RefusedChangesState, NotRunnable }`
  - `pub fn run_step(step: &Step, spawner: &dyn Spawner) -> RunOutcome`
  - `pub fn known_program(name: &str) -> bool`

- [ ] **Step 1: Promote the PATH probe in `sources.rs`**

In `crates/conductor/src/sources.rs`, change the private `which` to public and rename for clarity. Replace:

```rust
/// Whether `name` resolves to an executable on `$PATH`. An in-process `which(1)`:
/// scan `$PATH` entries for an executable file, no fork.
fn which(name: &str) -> bool {
```

with:

```rust
/// Whether `name` resolves to an executable on `$PATH`. An in-process `which(1)`:
/// scan `$PATH` entries for an executable file, no fork. Public so `run.rs` can
/// gate a spawn on availability with the same probe `read_binaries` uses.
pub fn is_on_path(name: &str) -> bool {
```

Then update the one caller inside `read_binaries` (line ~156): change `present: which(name),` to `present: is_on_path(name),`. Also update the two test references in `sources.rs` (`assert!(which("sh"));` and `assert!(!which("definitely-not-a-real-binary-xyzzy"));`) to `is_on_path(...)`.

- [ ] **Step 2: Write the failing tests**

Create `crates/conductor/src/run.rs`:

```rust
//! The delegated-spawn layer — Conductor's single subprocess choke point.
//!
//! Phase 2 runs ONLY Ring-1 (read-only) steps. A Ring-2 (state-changing) step is
//! refused here as defence in depth, on top of the TUI routing it to a no-op.
//! Spawning is direct (`std::process::Command`) with a fixed argv vector and NO
//! shell, so a finding id carried in a step's command can never become a shell
//! metacharacter — it is one argv element. The actual launch sits behind the
//! `Spawner` trait so tests can assert intent ("would spawn X with argv […]")
//! without starting a real process.

use std::process::ExitStatus;

use crate::plan::{Ring, Step};
use crate::sources::{is_on_path, SUITE_BINARIES};

/// Abstracts the actual process launch so tests don't fork.
pub trait Spawner {
    fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus>;
}

/// The real launcher: direct exec of a known binary, inheriting the terminal. No
/// shell is invoked.
pub struct RealSpawner;

impl Spawner for RealSpawner {
    fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
        std::process::Command::new(&argv[0]).args(&argv[1..]).status()
    }
}

/// What happened (or didn't) when asked to run a step.
#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// The step ran; the bool is the child's success().
    Ran(bool),
    /// The step's binary is not on `$PATH`; carries the binary name for a hint.
    NotAvailable(String),
    /// A Ring-2 step was asked to run in Phase 2 — refused.
    RefusedChangesState,
    /// The step has no runnable command (Info, or no command at all).
    NotRunnable,
}

/// Is `name` a binary Conductor is allowed to spawn? Only the known suite tools
/// (plus rexops, already in SUITE_BINARIES). Never spawn anything else.
pub fn known_program(name: &str) -> bool {
    SUITE_BINARIES.contains(&name)
}

/// Split a step's command into an argv vector on ASCII whitespace. The command
/// is a fixed string the rules built; an id within it is already a single token,
/// so it stays one argv element here — never interpolated, never shell-split
/// beyond plain whitespace.
fn argv_of(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(|s| s.to_string()).collect()
}

/// Run a single step through the spawner, enforcing every Phase-2 safety rule:
/// Ring-2 is refused; Info / commandless steps are not runnable; the program
/// must be a known suite binary and present on `$PATH` before any spawn.
pub fn run_step(step: &Step, spawner: &dyn Spawner) -> RunOutcome {
    if step.ring == Ring::ChangesState {
        return RunOutcome::RefusedChangesState;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Step;
    use std::cell::RefCell;

    /// Records argv it is asked to spawn; never launches anything. `success`
    /// controls the simulated child result.
    struct TestSpawner {
        calls: RefCell<Vec<Vec<String>>>,
        success: bool,
    }

    impl TestSpawner {
        fn new(success: bool) -> Self {
            TestSpawner { calls: RefCell::new(Vec::new()), success }
        }
    }

    impl Spawner for TestSpawner {
        fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push(argv.to_vec());
            // Fabricate an ExitStatus without a real process, portably, by
            // running a trivially-succeeding/failing builtin.
            let code = if self.success { "true" } else { "false" };
            std::process::Command::new(code).status()
        }
    }

    fn ro(cmd: &str) -> Step {
        Step::new("inv", "investigate", Some(cmd.to_string()), Ring::ReadOnly)
    }

    #[test]
    fn ring2_step_is_refused_and_never_spawned() {
        let sp = TestSpawner::new(true);
        let step = Step::new("refresh", "refresh", Some("workstate snapshot".into()), Ring::ChangesState);
        assert_eq!(run_step(&step, &sp), RunOutcome::RefusedChangesState);
        assert!(sp.calls.borrow().is_empty(), "a changes-state step must never reach the spawner");
    }

    #[test]
    fn info_step_is_not_runnable() {
        let sp = TestSpawner::new(true);
        let step = Step::new("wiring", "install rewind", Some("cargo install rewind".into()), Ring::Info);
        assert_eq!(run_step(&step, &sp), RunOutcome::NotRunnable);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn readonly_step_builds_argv_with_id_as_one_token_and_no_shell() {
        // A finding id with shell-significant chars must stay a single argv token.
        let sp = TestSpawner::new(true);
        let step = ro("bulwark show deploy-prod.sh;rm -rf");
        let outcome = run_step(&step, &sp);
        // argv[0] = bulwark (known), but the third token is the whole id incl ';'.
        let calls = sp.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "bulwark");
        assert_eq!(calls[0][1], "show");
        assert_eq!(calls[0][2], "deploy-prod.sh;rm");
        // (whitespace split only — the ';' never triggers a shell because there
        // is no shell; this asserts plain tokenization, not interpretation.)
        assert!(matches!(outcome, RunOutcome::Ran(true)));
    }

    #[test]
    fn unknown_program_is_never_spawned() {
        let sp = TestSpawner::new(true);
        let step = ro("evil-tool --do-bad");
        assert_eq!(run_step(&step, &sp), RunOutcome::NotRunnable);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn known_program_recognizes_suite_bins_only() {
        assert!(known_program("pulse"));
        assert!(known_program("bulwark"));
        assert!(known_program("rexops"));
        assert!(!known_program("rm"));
        assert!(!known_program("bash"));
    }
}
```

Add to `crates/conductor/src/lib.rs` (after `pub mod report;`):

```rust
pub mod run;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p conductor run::tests 2>&1 | tail -25`
Expected: `ring2_step_is_refused_and_never_spawned`, `info_step_is_not_runnable`, `readonly_step_builds_argv_with_id_as_one_token_and_no_shell`, `unknown_program_is_never_spawned`, `known_program_recognizes_suite_bins_only` all `ok`.

Note: the `readonly_*` test depends on `bulwark` being a known program (it is, via SUITE_BINARIES) but NOT on it being installed — but `run_step` calls `is_on_path("bulwark")` before spawning. On a box where `bulwark` is absent, that test would return `NotAvailable`. To keep the test hermetic, the test must put a stub `bulwark` on `$PATH`. Replace the body of `readonly_step_builds_argv_with_id_as_one_token_and_no_shell` to set up a temp bin dir first (mirrors `tests/cli.rs`):

```rust
    #[test]
    fn readonly_step_builds_argv_with_id_as_one_token_and_no_shell() {
        // Put a stub `bulwark` on PATH so the availability check passes without
        // depending on the host having the suite installed.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let orig = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());

        let sp = TestSpawner::new(true);
        let step = ro("bulwark show deploy-prod.sh;rm -rf");
        let outcome = run_step(&step, &sp);
        let calls = sp.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "bulwark");
        assert_eq!(calls[0][1], "show");
        assert_eq!(calls[0][2], "deploy-prod.sh;rm");
        assert!(matches!(outcome, RunOutcome::Ran(true)));

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
```

`tempfile` is already a dev-dependency (see `crates/conductor/Cargo.toml`). Re-run the tests; all five pass.

- [ ] **Step 4: Lint and format**

Run: `cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -15`
Expected: no warnings. (clippy may suggest `&argv[0]` → `argv.first()`; the `argv.is_empty()` guard already precedes indexing, so a direct `&argv[0]` is fine — if clippy insists, restructure with `let Some(prog) = argv.first() else { return NotRunnable };`.)

Run: `cargo fmt -p conductor -- --check`
Expected: no output. Run `cargo fmt -p conductor` if needed.

- [ ] **Step 5: Commit**

```bash
git add crates/conductor/src/run.rs crates/conductor/src/lib.rs crates/conductor/src/sources.rs
git commit -m "feat(conductor): run.rs — Ring-1 spawn choke point (Spawner trait, no shell, Ring-2 refused)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: The event loop and navigation (`tui/mod.rs`)

**Files:**
- Modify: `crates/conductor/src/tui/mod.rs` (add the app: `AppState`, the loop, navigation)
- Test: inline `#[cfg(test)] mod tests` in `tui/mod.rs` (pure transition tests — no PTY)

**Interfaces:**
- Consumes: `tui::term::{RawMode, Key, read_key, paint, install_panic_guard}`, `tui::frame`, `tui::style::Style`, `run::{run_step, RealSpawner, RunOutcome, Spawner}`, `plan::{Plan, StepStatus, Ring}`, `sources::is_on_path`.
- Produces:
  - `pub struct AppState { pub plan: Plan, pub cursor: usize, pub screen: Screen, pub notice: Option<String> }`
  - `pub enum Screen { Plan, Help }`
  - `pub enum Action { Redraw, Quit }`
  - `pub fn step(app: &mut AppState, key: Key, spawner: &dyn Spawner) -> Action` (pure-ish: mutates state, calls spawner for Ring-1; returns whether to keep looping). The blocking spawn-with-suspend is performed by the caller via a closure passed in — see Step 1.
  - `pub fn run(plan: Plan, force_no_color: bool) -> std::io::Result<()>` (the real loop)

  To keep `step` unit-testable without a terminal, the actual suspend/spawn is injected: `step` calls `spawner.spawn` directly (the `RealSpawner` used by `run` wraps the suspend); tests pass a `TestSpawner`. The terminal suspend lives in a `SuspendSpawner` adapter inside `run`.

- [ ] **Step 1: Write the failing transition tests**

Append to `crates/conductor/src/tui/mod.rs` (after the `pub mod` lines):

```rust
//! The interactive app: state, navigation, and the event loop. Rendering is
//! delegated to `frame`, terminal I/O to `term`, spawning to `run`. This module
//! only maps keys to state transitions and chooses which frame to paint — so the
//! transitions are unit-testable with a fake spawner and no PTY.

use std::io::{self, IsTerminal};

use crate::plan::{Plan, Ring, StepStatus};
use crate::run::{run_step, RealSpawner, RunOutcome, Spawner};
use crate::tui::term::{self, Key, RawMode};

/// Which screen is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Plan,
    Help,
}

/// Whether the loop should repaint and continue, or exit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Redraw,
    Quit,
}

/// All interactive state. The plan's per-step `status` carries Done/Skipped;
/// `cursor` is the focused (▸) step; `notice` is a transient one-liner cleared on
/// the next keypress.
pub struct AppState {
    pub plan: Plan,
    pub cursor: usize,
    pub screen: Screen,
    pub notice: Option<String>,
}

impl AppState {
    pub fn new(plan: Plan) -> Self {
        AppState { plan, cursor: 0, screen: Screen::Plan, notice: None }
    }

    fn advance(&mut self) {
        if self.cursor + 1 < self.plan.steps.len() {
            self.cursor += 1;
        }
    }
}

/// Apply one key to the state, using `spawner` for any Ring-1 run. Pure with
/// respect to the terminal — it never touches stdin/stdout — so it is fully
/// unit-testable. The real loop wraps `spawner` so a spawn suspends the TUI.
pub fn step(app: &mut AppState, key: Key, spawner: &dyn Spawner) -> Action {
    // Any key clears a stale notice first; specific arms may set a fresh one.
    app.notice = None;

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
            run_current(app, spawner);
            Action::Redraw
        }
        _ => Action::Redraw,
    }
}

/// Run the focused step (Enter). Ring-1 spawns + marks Done + advances; Ring-2
/// is a no-op with a note; Info / unavailable produce a note and stay put.
fn run_current(app: &mut AppState, spawner: &dyn Spawner) {
    let Some(step_ref) = app.plan.steps.get(app.cursor) else {
        return;
    };
    let ring = step_ref.ring;
    match run_step(step_ref, spawner) {
        RunOutcome::Ran(_) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Done;
            }
            app.advance();
        }
        RunOutcome::RefusedChangesState => {
            app.notice = Some("this step changes state — needs Phase 3, not run".to_string());
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

Append the test module to `crates/conductor/src/tui/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{self, Step};
    use crate::run::Spawner;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use std::cell::RefCell;
    use std::process::ExitStatus;

    struct FakeSpawner {
        calls: RefCell<Vec<Vec<String>>>,
    }
    impl FakeSpawner {
        fn new() -> Self {
            FakeSpawner { calls: RefCell::new(Vec::new()) }
        }
    }
    impl Spawner for FakeSpawner {
        fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push(argv.to_vec());
            std::process::Command::new("true").status()
        }
    }

    /// Plan: refresh (Ring2) → capture (Ring2) → investigate (Ring1).
    fn sample() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    #[test]
    fn q_quits() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        assert_eq!(step(&mut app, Key::Char('q'), &sp), Action::Quit);
    }

    #[test]
    fn a_advances_focus_without_running() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('a'), &sp);
        assert_eq!(app.cursor, 1);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn s_skips_and_advances() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('s'), &sp);
        assert_eq!(app.plan.steps[0].status, StepStatus::Skipped);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn enter_on_ring2_is_a_noop_with_note_and_no_spawn() {
        let mut app = AppState::new(sample()); // step 0 is the Ring2 refresh
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp);
        assert_eq!(app.plan.steps[0].status, StepStatus::Pending, "ring2 must not be marked done");
        assert_eq!(app.cursor, 0, "ring2 must not advance");
        assert!(sp.calls.borrow().is_empty(), "ring2 must never spawn");
        assert!(app.notice.as_deref().unwrap().contains("needs Phase 3"));
    }

    #[test]
    fn enter_on_ring1_spawns_marks_done_and_advances() {
        // Move focus to the Ring1 investigate step (last step), with a stub on PATH.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let orig = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());

        let mut app = AppState::new(sample());
        let last = app.plan.steps.len() - 1;
        app.cursor = last;
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp);
        assert_eq!(app.plan.steps[last].status, StepStatus::Done);
        assert_eq!(sp.calls.borrow().len(), 1);
        assert_eq!(sp.calls.borrow()[0][0], "bulwark");

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn question_toggles_help_and_any_key_returns() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('?'), &sp);
        assert_eq!(app.screen, Screen::Help);
        step(&mut app, Key::Char('x'), &sp);
        assert_eq!(app.screen, Screen::Plan);
    }

    #[test]
    fn notice_clears_on_next_key() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp); // sets the ring2 notice
        assert!(app.notice.is_some());
        step(&mut app, Key::Char('a'), &sp); // any key clears it
        assert!(app.notice.is_none());
    }
}
```

- [ ] **Step 2: Write the real event loop**

Append the loop to `crates/conductor/src/tui/mod.rs` (before the `#[cfg(test)]`):

```rust
/// Terminal width/height via `ioctl(TIOCGWINSZ)`; falls back to 80×24 if it
/// can't be read (e.g. piped). Used to pick the compact fallback under 80×24.
fn term_size() -> (u16, u16) {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    const TIOCGWINSZ: u64 = 0x5413; // Linux
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: ioctl fills a correctly-sized Winsize we own.
    let rc = unsafe { ioctl(1, TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        (ws.ws_col, ws.ws_row)
    } else {
        (80, 24)
    }
}

/// Render the current frame for `app` at the current terminal size.
fn render(app: &AppState, style: &crate::tui::style::Style) -> String {
    let (cols, rows) = term_size();
    if app.screen == Screen::Help {
        return crate::tui::frame::help_screen(style);
    }
    if cols < 80 || rows < 24 {
        return crate::tui::frame::compact_plan(&app.plan, app.cursor, style);
    }
    crate::tui::frame::plan_screen(&app.plan, app.cursor, app.notice.as_deref(), style)
}

/// A spawner that suspends the TUI for the duration of the child, handing it the
/// real terminal, then resumes. Wraps the raw-mode guard.
struct SuspendSpawner<'a> {
    raw: RefCell<&'a mut RawMode>,
}

impl Spawner for SuspendSpawner<'_> {
    fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
        let mut raw = self.raw.borrow_mut();
        raw.suspend(|| RealSpawner.spawn(argv))
    }
}

use std::cell::RefCell;
use std::process::ExitStatus;

/// Run the interactive TUI to completion. Sets up the panic guard + raw mode,
/// loops painting frames and applying keys until Quit, and always restores the
/// terminal on the way out (RawMode's Drop).
pub fn run(plan: Plan, force_no_color: bool) -> std::io::Result<()> {
    term::install_panic_guard();
    let style = crate::tui::style::Style::resolve(force_no_color);
    let mut app = AppState::new(plan);
    let mut raw = RawMode::enter()?;
    let mut stdin = io::stdin();
    loop {
        term::paint(&render(&app, &style))?;
        let key = term::read_key(&mut stdin)?;
        let spawner = SuspendSpawner { raw: RefCell::new(&mut raw) };
        let action = step(&mut app, key, &spawner);
        drop(spawner);
        if action == Action::Quit {
            break;
        }
    }
    Ok(())
}

/// True when the bare invocation should open the interactive TUI: stdout is a
/// real terminal. A non-TTY bare invocation stays scriptable (prints status).
pub fn should_run_interactive() -> bool {
    io::stdout().is_terminal()
}
```

- [ ] **Step 3: Run the transition tests to verify they pass**

Run: `cargo test -p conductor tui::tests 2>&1 | tail -30`
Expected: all eight transition tests `ok` (`q_quits`, `a_advances_focus_without_running`, `s_skips_and_advances`, `enter_on_ring2_is_a_noop_with_note_and_no_spawn`, `enter_on_ring1_spawns_marks_done_and_advances`, `question_toggles_help_and_any_key_returns`, `notice_clears_on_next_key`).

- [ ] **Step 4: Lint and format**

Run: `cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings. Likely fixes: the variadic `ioctl` extern is fine; if clippy flags `SuspendSpawner`'s lifetime or the `RefCell<&mut>` pattern, keep it (it is the minimal way to share `&mut RawMode` with the trait object for one call). Move the two `use` lines (`std::cell::RefCell`, `std::process::ExitStatus`) to the top of the file with the other imports if clippy/fmt prefers; ensure no duplicate-import warning with the test module.

Run: `cargo fmt -p conductor -- --check`
Expected: no output. Run `cargo fmt -p conductor` if needed.

- [ ] **Step 5: Commit**

```bash
git add crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): tui event loop + navigation — Ring-1 runs, Ring-2 no-op-with-note, rexops handoff

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Wire bare `conductor` to the TUI + `--dump-view`; docs; full-workspace gate

**Files:**
- Modify: `crates/conductor/src/main.rs` (bare→TUI on a TTY; `--dump-view`; keep status/health/plan)
- Modify: `crates/conductor/README.md` (Phase 2 section)
- Modify: `LAST_WORK.md` (repo root — new top entry)
- Test: extend `crates/conductor/tests/cli.rs` (integration via `--dump-view`)

**Interfaces:**
- Consumes: `conductor::tui::{self, should_run_interactive}`, `conductor::tui::frame`, `conductor::tui::style`, plus the existing Phase-1 `report`, `load_state`, `plan`.
- Produces: no new public library API; only CLI behavior.

- [ ] **Step 1: Add the bare-TUI gate + `--dump-view` to `main.rs`**

In `crates/conductor/src/main.rs`, add a hidden `dump_view` flag to the `Cli` struct (after the `data_dir` field):

```rust
    /// Render one TUI frame once and exit (no event loop): plan | healthy |
    /// compact | help. For deterministic snapshot tests; hidden from help.
    #[arg(long, value_name = "VIEW", global = true, hide = true)]
    dump_view: Option<String>,
```

Replace the `main` dispatch block so bare invocation opens the TUI on a TTY, `--dump-view` short-circuits, and the explicit subcommands are unchanged:

```rust
fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    // Deterministic frame dump for tests: build the real plan, render one frame.
    if let Some(view) = &cli.dump_view {
        return run_dump_view(&cli, view);
    }

    let result = match &cli.command {
        None => run_bare(&cli, &style),
        Some(Cmd::Status) => run_status(&cli, &style),
        Some(Cmd::Health) => run_health(&cli, &style),
        Some(Cmd::Plan) => run_plan(&cli, &style),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("conductor: {err}");
            ExitCode::from(3)
        }
    }
}

/// Bare `conductor`: open the interactive TUI on a real terminal; otherwise fall
/// back to the scriptable `status` output so pipes/CI still work.
fn run_bare(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    if cli.json || !conductor::tui::should_run_interactive() {
        return run_status(cli, style);
    }
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    conductor::tui::run(plan, cli.no_color).map_err(|e| ConductorError::Tui(e.to_string()))?;
    Ok(ExitCode::SUCCESS)
}

/// Render exactly one TUI frame (no event loop) and exit 0 — the test backbone.
fn run_dump_view(cli: &Cli, view: &str) -> ExitCode {
    let dir = match data_dir(cli) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("conductor: {e}");
            return ExitCode::from(3);
        }
    };
    let state = load_state(&dir);
    let plan = plan::build(&state);
    let style = conductor::tui::style::Style::resolve(true); // dumps are monochrome
    use conductor::tui::frame;
    let frame = match view {
        "plan" => frame::plan_screen(&plan, 0, None, &style),
        "healthy" => frame::healthy_screen(&style),
        "compact" => frame::compact_plan(&plan, 0, &style),
        "help" => frame::help_screen(&style),
        other => {
            eprintln!("conductor: --dump-view needs one of: plan healthy compact help (got {other})");
            return ExitCode::from(3);
        }
    };
    print!("{frame}");
    ExitCode::SUCCESS
}
```

- [ ] **Step 2: Add the `Tui` error variant**

In `crates/conductor/src/error.rs`, add a variant to `ConductorError` (read the file first to match its exact `thiserror` style). Add:

```rust
    /// The interactive TUI failed to run (terminal I/O error).
    #[error("interactive mode failed: {0}")]
    Tui(String),
```

- [ ] **Step 3: Write the failing integration tests**

Append to `crates/conductor/tests/cli.rs` (reuse its existing stub-bin-dir helper — read the file to find the helper name; the plan assumes a helper like `stub_path()` or an inline pattern that creates a `bin/` of all 8 `SUITE_BINARIES` and returns the dir). If the helper exists, use it; otherwise add this local helper at the top of the test module:

```rust
/// Create a dir of stub executables for every suite binary and return it, so the
/// $PATH probe sees all 8 as present (neutralizes host-installed bins). Mirrors
/// the Phase-1 cli.rs trick.
fn all_bins_stub_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "pulse", "rewind", "tripwire", "portman", "bulwark", "workstate", "proto", "rexops",
    ] {
        let p = dir.path().join(name);
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}
```

Then add the tests (use `assert_cmd`/`Command` exactly as the existing tests do — match their style; the snippet below uses `std::process::Command` with `CARGO_BIN_EXE_conductor`):

```rust
#[test]
fn dump_view_plan_renders_steps_for_a_stale_feed_state() {
    let data = tempfile::tempdir().unwrap();
    // a stale workstate feed → a refresh step in the plan
    let feed = data.path().join("rexops/feeds/workstate.snapshot.json");
    std::fs::create_dir_all(feed.parent().unwrap()).unwrap();
    std::fs::write(&feed, r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_conductor"))
        .args(["--data-dir", data.path().to_str().unwrap(), "--dump-view", "plan"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("the plan"), "frame:\n{s}");
    assert!(s.contains("workstate snapshot"));
    assert!(s.contains("changes state"));
}

#[test]
fn dump_view_healthy_says_nothing_to_conduct_when_all_clear() {
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path()).unwrap(); // empty: no feeds, no findings
    let bins = all_bins_stub_dir();
    // PATH = only the stub dir, so all 8 bins are "present" → no wiring steps.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_conductor"))
        .args(["--data-dir", data.path().to_str().unwrap(), "--dump-view", "healthy"])
        .env("PATH", bins.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("nothing to conduct"), "frame:\n{s}");
}

#[test]
fn bare_non_tty_falls_back_to_status_not_the_tui() {
    // Output is captured (not a TTY), so bare conductor must print status text.
    let data = tempfile::tempdir().unwrap();
    let feed = data.path().join("rexops/feeds/workstate.snapshot.json");
    std::fs::create_dir_all(feed.parent().unwrap()).unwrap();
    std::fs::write(&feed, r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_conductor"))
        .args(["--data-dir", data.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    // status output contains the plan heading; the TUI would have needed a TTY.
    assert!(s.contains("the plan") || s.contains("nothing to conduct"), "frame:\n{s}");
}
```

- [ ] **Step 4: Run the integration tests to verify they pass**

Run: `cargo test -p conductor --test cli 2>&1 | tail -25`
Expected: the three new tests pass alongside the existing 7. If `dump_view_healthy_*` still shows steps, the host PATH leaked — confirm the `.env("PATH", bins.path())` is present and that no feed files exist under `data`.

- [ ] **Step 5: Update the README**

In `crates/conductor/README.md`, add a Phase 2 section after the existing usage section (read the file first to match its tone). Add:

```markdown
## Interactive mode (Phase 2)

Run `conductor` with no arguments in a terminal to open the interactive plan:

    conductor

It shows the situation and the ordered steps, with the current step marked `▸`.

    enter  run step    s  skip    a  advance    r  rexops    ?  help    q  quit

- `enter` runs the current step. **Read-only** steps spawn the tool, hand over
  the terminal, and mark the step `✓`. A **changes-state** step is shown with its
  command and `changes state` tag but is **not run** in Phase 2 (it needs the
  Phase 3 driver) — Conductor says so and leaves it pending.
- `s` skips, `a` moves focus, `r` hands off to the RexOps cockpit, `q` quits.

Conductor still writes nothing itself and runs no state-changing command. Piped
or non-interactive (`conductor | cat`, CI), bare `conductor` prints `status`
instead, so scripts keep working.
```

- [ ] **Step 6: Update LAST_WORK.md**

Read `LAST_WORK.md` at the repo root, then add a new top entry (above the Phase 1 entry) summarizing: Phase 2 delivered — interactive TUI (hand-rolled, no new deps), Ring-1 read-only spawning, Ring-2 renders but is a no-op-with-note, `--dump-view` snapshot tests, bare `conductor`→TUI on a TTY / status when piped; still zero writes, zero Ring-2; branch `conductor-phase2`; all conductor tests + workspace build green; NOT pushed (awaiting human approval). Match the file's existing entry format.

- [ ] **Step 7: Full-workspace verification**

Run each and confirm clean:

```bash
cargo test -p conductor 2>&1 | tail -15
cargo clippy -p conductor --all-targets -- -D warnings 2>&1 | tail -15
cargo fmt -p conductor -- --check
cargo build --workspace 2>&1 | tail -15
```

Expected: all conductor tests pass; no clippy warnings; fmt clean (no output); workspace builds. If `cargo build --workspace` surfaces an unrelated sibling-crate issue, note it but do not fix out-of-scope crates — confirm `cargo build -p conductor` is clean at minimum.

- [ ] **Step 8: Commit**

```bash
git add crates/conductor/src/main.rs crates/conductor/src/error.rs crates/conductor/tests/cli.rs crates/conductor/README.md LAST_WORK.md
git commit -m "feat(conductor): bare conductor opens the TUI (status when piped) + --dump-view; Phase 2 docs

Phase 2 complete: interactive TUI + Ring-1 read-only spawning. Ring-2 renders
but is a no-op-with-note (Phase 3). No new deps, zero writes, zero Ring-2.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Scope (bare→TUI, Ring-1 runs, Ring-2 no-op-note) → Tasks 5, 6. ✓
- TUI base hand-rolled/no deps → Task 1 (port pulse term). ✓
- Modules tui/{mod,term,frame,style}, run.rs, main.rs, lib.rs → Tasks 1–6 cover each. ✓
- State machine (AppState, Screen, notice, keys) → Task 5. ✓
- run.rs Spawner trait, no shell, $PATH check, Ring-2 refused, id one token → Task 4. ✓
- Testing: --dump-view, width invariant, run.rs intent, term key tests, PATH stub trick, color-off → Tasks 1/3/4/6. ✓
- Invariants (no writes, no Ring-2, no dep, NO_COLOR legible, compact no-clip, no Esc-only, exit 0/3) → enforced across Global Constraints + Task 3 width tests + Task 4 refusal + Task 5 `q`/Esc both quit. ✓
- DoD checklist (spec) → maps to Task 6 Step 7 + the per-task tests. ✓

**Placeholder scan:** No TBD/TODO; every code step has real code; commands have expected output. The one "match the file's format" instructions (README tone, LAST_WORK entry, error.rs thiserror style) require reading an existing file first — flagged in-step with the exact content to add. ✓

**Type consistency:** `Spawner::spawn(&self, argv:&[String]) -> io::Result<ExitStatus>` identical in Task 4 (def), Task 5 (SuspendSpawner/FakeSpawner), Task 6 (RealSpawner via run). `RunOutcome` variants identical in Task 4 (def) and Task 5 (match). `Style` fields identical in Tasks 2/3. `frame::{plan_screen,healthy_screen,compact_plan,help_screen}` signatures identical in Tasks 3/5/6. `is_on_path` (Task 4) used in Tasks 4/5. `AppState`/`Screen`/`Action`/`step` consistent Task 5↔tests. ✓

**Note for the implementer (Esc):** the spec says "no Escape-only flow." Esc quits in `step` (Task 5) but `q` also always quits — Esc is never the *only* path to anything, satisfying the constraint. Verify this holds if you add any sub-screen later.
