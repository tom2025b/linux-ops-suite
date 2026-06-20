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
    fn ring2_step_is_refused_and_never_spawned() {
        let sp = TestSpawner::new(true);
        let step = Step::new(
            "refresh",
            "refresh",
            Some("workstate snapshot".into()),
            Ring::ChangesState,
        );
        assert_eq!(run_step(&step, &sp), RunOutcome::RefusedChangesState);
        assert!(
            sp.calls.borrow().is_empty(),
            "a changes-state step must never reach the spawner"
        );
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
