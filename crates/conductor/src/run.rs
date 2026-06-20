//! The delegated-spawn layer — Conductor's single subprocess choke point.
//!
//! A Ring-2 (state-changing) step is spawned like any other step here; the
//! `y`-confirm gate that must precede it lives in the TUI (`tui/mod.rs`), not in
//! this module. Spawning is direct (`std::process::Command`) with a fixed argv
//! vector and NO shell, so a finding id carried in a step's command can never
//! become a shell metacharacter — it is one argv element. The actual launch sits
//! behind the `Spawner` trait so tests can assert intent ("would spawn X with
//! argv […]") without starting a real process.

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
        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .status()
    }
}

/// What happened (or didn't) when asked to run a step.
#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// The step ran; the bool is the child's success().
    Ran(bool),
    /// The step's binary is not on `$PATH`; carries the binary name for a hint.
    NotAvailable(String),
    /// The step has no runnable command (Info, or no command at all).
    NotRunnable,
    /// The spawn itself failed — the child never produced an exit status. With
    /// the real [`crate::tui::SuspendSpawner`] this is almost always a failure to
    /// suspend or re-enter the terminal around the child, which leaves the
    /// display in a bad state; it is NOT the same as a tool exiting non-zero, so
    /// callers must report it differently. Carries the error text.
    SpawnError(String),
}

/// Is `name` a binary Conductor is allowed to spawn? Only the known suite tools
/// (plus rexops, already in SUITE_BINARIES). Never spawn anything else.
pub fn known_program(name: &str) -> bool {
    SUITE_BINARIES.contains(&name)
}

/// Split a step's command string into an argv vector. Tokens are separated by
/// ASCII whitespace, except inside a single-quoted run `'…'`, which is taken
/// verbatim as one token (quotes stripped). This is the exact inverse of
/// [`crate::plan::quote_arg`]: a finding id or job title that contains spaces is
/// emitted single-quoted by the rules, so it round-trips back to ONE argv
/// element here — the modal's displayed command and the spawned argv are
/// guaranteed to be the same list. No other shell syntax is interpreted (no
/// double quotes, no escapes, no globbing): the command is never a user shell
/// line, only the suite's own fixed verbs plus a quoted id.
fn argv_of(cmd: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut have_token = false; // distinguishes "" (a real empty arg) from no arg
    for ch in cmd.chars() {
        match ch {
            '\'' => {
                in_quote = !in_quote;
                have_token = true; // quotes always start a token, even if empty
            }
            c if c.is_ascii_whitespace() && !in_quote => {
                if have_token {
                    argv.push(std::mem::take(&mut cur));
                    have_token = false;
                }
            }
            c => {
                cur.push(c);
                have_token = true;
            }
        }
    }
    if have_token {
        argv.push(cur);
    }
    argv
}

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
        // The child never ran (or the terminal suspend/re-enter around it
        // failed). Surface it as its own outcome, not a silent `Ran(false)` that
        // masquerades as "the tool exited non-zero".
        Err(e) => RunOutcome::SpawnError(e.to_string()),
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
            TestSpawner {
                calls: RefCell::new(Vec::new()),
                success,
            }
        }
    }

    impl Spawner for TestSpawner {
        fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push(argv.to_vec());
            // Fabricate an ExitStatus without forking at all — no real process,
            // no dependency on a binary being on PATH. `success` picks the code.
            use std::os::unix::process::ExitStatusExt;
            Ok(ExitStatus::from_raw(if self.success { 0 } else { 1 << 8 }))
        }
    }

    fn ro(cmd: &str) -> Step {
        Step::new("inv", "investigate", Some(cmd.to_string()), Ring::ReadOnly)
    }

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

    /// A spawner that always fails the launch — models the SuspendSpawner
    /// failing to suspend/re-enter the terminal around the child.
    struct FailSpawner;
    impl Spawner for FailSpawner {
        fn spawn(&self, _argv: &[String]) -> std::io::Result<ExitStatus> {
            Err(std::io::Error::other("suspend failed"))
        }
    }

    #[test]
    fn spawn_failure_is_its_own_outcome_not_ran_false() {
        // M3 regression: a failed launch must NOT collapse to Ran(false) (which
        // reads as "the tool exited non-zero"); it is a distinct SpawnError so
        // the TUI can warn that the terminal state is suspect.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
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

        let outcome = run_step(&ro("bulwark show x.sh"), &FailSpawner);
        assert!(
            matches!(outcome, RunOutcome::SpawnError(ref m) if m.contains("suspend failed")),
            "got {outcome:?}"
        );

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn info_step_is_not_runnable() {
        let sp = TestSpawner::new(true);
        let step = Step::new(
            "wiring",
            "install rewind",
            Some("cargo install rewind".into()),
            Ring::Info,
        );
        assert_eq!(run_step(&step, &sp), RunOutcome::NotRunnable);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn readonly_step_builds_argv_with_id_as_one_token_and_no_shell() {
        // Put a stub `bulwark` on PATH so the availability check passes without
        // depending on the host having the suite installed.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Prepend the stub dir to PATH (not replace it) so other tests that
        // probe existing binaries like `sh` still see a full PATH while this
        // test holds the mutex.
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

    #[test]
    fn quoted_id_with_spaces_stays_one_argv_token_and_matches_display() {
        // M1 regression: a finding/title with a space must NOT fragment into
        // extra argv elements, and the spawned argv must match the displayed
        // command exactly. The rules quote such a value via plan::quote_arg, so
        // the command string here is `proto show 'Nightly Backup'`.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("proto");
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

        let cmd = format!("proto show {}", crate::plan::quote_arg("Nightly Backup"));
        assert_eq!(cmd, "proto show 'Nightly Backup'");
        let step = Step::new("rev", "review", Some(cmd.clone()), Ring::ReadOnly);

        // What the confirm modal would display is exactly `cmd`…
        assert_eq!(confirm_command(&step), Some(cmd.as_str()));
        // …and what gets spawned is ["proto","show","Nightly Backup"] — the id
        // is one token, not two.
        let sp = TestSpawner::new(true);
        let outcome = run_step(&step, &sp);
        assert!(matches!(outcome, RunOutcome::Ran(true)));
        let calls = sp.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["proto", "show", "Nightly Backup"]);

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn argv_of_round_trips_quote_arg() {
        // Direct unit coverage of the split/quote inverse pair, no PATH needed.
        use crate::plan::quote_arg;
        for raw in ["x.sh", "deploy prod.sh", "two  spaces", "", "a'b"] {
            let cmd = format!("bulwark show {}", quote_arg(raw));
            let argv = argv_of(&cmd);
            assert_eq!(argv[0], "bulwark");
            assert_eq!(argv[1], "show");
            // The id is always exactly one trailing token (quotes stripped a '\'').
            assert_eq!(argv.len(), 3, "cmd={cmd:?} argv={argv:?}");
        }
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
