use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::core::checks::{Check, CheckProfile};

use super::command::{ParsedCommand, parse_command};
use super::types::{CheckOutcome, CheckStatus, ExecutionOptions, StreamedOutcome};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn execute_profile(profile: &CheckProfile, options: &ExecutionOptions) -> Vec<CheckOutcome> {
    profile
        .checks
        .iter()
        .map(|check| execute_check(check, options))
        .collect()
}

pub fn execute_check(check: &Check, options: &ExecutionOptions) -> CheckOutcome {
    let started = Instant::now();
    let parsed = match parse_command(&check.command) {
        Ok(parsed) => parsed,
        Err(message) => return error_outcome(check, options, started, message),
    };

    let child = match spawn_check_process(&parsed, options) {
        Ok(child) => child,
        Err(err) => {
            return error_outcome(
                check,
                options,
                started,
                format!("Failed to execute {:?}: {}", parsed.program, err),
            )
            .with_command(&parsed.program, &parsed.args);
        }
    };

    let run = wait_and_capture(child, options.timeout, started);
    outcome_from_run(check, options, &parsed, run)
}

// -----------------------------------------------------------------------------
// run_streaming — execute a command with output STREAMED to the terminal.
// -----------------------------------------------------------------------------
// Used by the interactive `command:` step. Unlike execute_check (which captures
// stdout/stderr for a batch summary), this inherits the terminal so the operator
// watches the command live — exactly as if they'd typed it. It still runs under
// the SAME timeout + process-group kill machinery, so an interactive command that
// hangs is bounded and its sh -c grandchildren don't survive. Run via `sh -c` so
// the protocol's command string can use the full shell (pipes, &&, quoting). The
// status is derived from the exit code; a spawn failure is reported as an Error.
pub fn run_streaming(command: &str, options: &ExecutionOptions) -> StreamedOutcome {
    let started = Instant::now();

    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    if let Some(dir) = &options.working_dir {
        shell.current_dir(dir);
    }
    configure_child_process(&mut shell);

    let mut child = match shell.spawn() {
        Ok(child) => child,
        Err(err) => {
            return StreamedOutcome {
                status: CheckStatus::Error,
                exit_code: None,
                timed_out: false,
                timeout: options.timeout,
                error_message: Some(format!("could not run command: {err}")),
            };
        }
    };

    let end = wait_for_process(&mut child, options.timeout, started);
    streamed_outcome_from_end(end, options.timeout)
}

fn streamed_outcome_from_end(end: ProcessEnd, timeout: Option<Duration>) -> StreamedOutcome {
    match end {
        ProcessEnd::Exited(status) => StreamedOutcome {
            status: if status.success() {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            exit_code: status.code(),
            timed_out: false,
            timeout,
            error_message: None,
        },
        ProcessEnd::TimedOut {
            timeout: limit,
            termination,
        } => StreamedOutcome {
            status: CheckStatus::Error,
            exit_code: termination.status.and_then(|status| status.code()),
            timed_out: true,
            timeout: Some(limit),
            error_message: termination_error(&termination),
        },
        ProcessEnd::WaitError {
            message,
            termination,
        } => StreamedOutcome {
            status: CheckStatus::Error,
            exit_code: termination.status.and_then(|status| status.code()),
            timed_out: false,
            timeout,
            error_message: Some(match termination_error(&termination) {
                Some(extra) => format!("{message}; {extra}"),
                None => message,
            }),
        },
    }
}

fn termination_error(termination: &ChildTermination) -> Option<String> {
    match (&termination.kill_error, &termination.wait_error) {
        (Some(kill), Some(wait)) => Some(format!("kill failed: {kill}; reap failed: {wait}")),
        (Some(kill), None) => Some(format!("kill failed: {kill}")),
        (None, Some(wait)) => Some(format!("reap failed: {wait}")),
        (None, None) => None,
    }
}

fn spawn_check_process(parsed: &ParsedCommand, options: &ExecutionOptions) -> io::Result<Child> {
    let mut command = Command::new(&parsed.program);
    command
        .args(&parsed.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = &options.working_dir {
        command.current_dir(dir);
    }

    configure_child_process(&mut command);
    command.spawn()
}

#[cfg(unix)]
fn configure_child_process(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_child_process(_command: &mut Command) {}

fn wait_and_capture(mut child: Child, timeout: Option<Duration>, started: Instant) -> CompletedRun {
    let stdout = capture_pipe(child.stdout.take(), "stdout");
    let stderr = capture_pipe(child.stderr.take(), "stderr");
    let process_end = wait_for_process(&mut child, timeout, started);

    CompletedRun {
        process_end,
        stdout: collect_output(stdout),
        stderr: collect_output(stderr),
        duration: started.elapsed(),
    }
}

fn wait_for_process(child: &mut Child, timeout: Option<Duration>, started: Instant) -> ProcessEnd {
    let deadline = timeout.and_then(|limit| started.checked_add(limit).map(|at| (limit, at)));

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ProcessEnd::Exited(status),
            Ok(None) => {}
            Err(err) => {
                return ProcessEnd::WaitError {
                    message: err.to_string(),
                    termination: terminate_child(child),
                };
            }
        }

        if let Some((limit, at)) = deadline {
            let now = Instant::now();
            if now >= at {
                return ProcessEnd::TimedOut {
                    timeout: limit,
                    termination: terminate_child(child),
                };
            }
            thread::sleep(POLL_INTERVAL.min(at.saturating_duration_since(now)));
        } else {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn capture_pipe<R>(reader: Option<R>, label: &'static str) -> OutputCapture
where
    R: Read + Send + 'static,
{
    match reader {
        Some(reader) => OutputCapture::Reader(thread::spawn(move || read_to_end(reader))),
        None => OutputCapture::Missing(label),
    }
}

fn read_to_end(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn collect_output(capture: OutputCapture) -> Result<String, String> {
    match capture {
        OutputCapture::Reader(handle) => match handle.join() {
            Ok(Ok(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Ok(Err(err)) => Err(format!("Failed to capture process output: {err}")),
            Err(_) => Err("Failed to capture process output: reader thread panicked".to_string()),
        },
        OutputCapture::Missing(label) => {
            Err(format!("Failed to capture process {label}: pipe missing"))
        }
    }
}

fn outcome_from_run(
    check: &Check,
    options: &ExecutionOptions,
    parsed: &ParsedCommand,
    run: CompletedRun,
) -> CheckOutcome {
    let mut outcome = CheckOutcome::new(check, options, CheckStatus::Error)
        .with_command(&parsed.program, &parsed.args);
    outcome.duration = run.duration;

    match run.stdout {
        Ok(stdout) => outcome.stdout = stdout,
        Err(message) => outcome.push_error(message),
    }
    match run.stderr {
        Ok(stderr) => outcome.stderr = stderr,
        Err(message) => outcome.push_error(message),
    }

    match run.process_end {
        ProcessEnd::Exited(status) => {
            outcome.exit_code = status.code();
            if outcome.error_message.is_none() {
                outcome.status = if status.success() {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                };
            }
        }
        ProcessEnd::TimedOut {
            timeout,
            termination,
        } => {
            outcome.timed_out = true;
            outcome.exit_code = termination.status.and_then(|status| status.code());
            outcome.push_error(format!("Timed out after {}", format_duration(timeout)));
            add_termination_errors(&mut outcome, termination);
        }
        ProcessEnd::WaitError {
            message,
            termination,
        } => {
            outcome.push_error(format!("Failed while waiting for process: {message}"));
            add_termination_errors(&mut outcome, termination);
        }
    }

    outcome
}

fn error_outcome(
    check: &Check,
    options: &ExecutionOptions,
    started: Instant,
    message: impl Into<String>,
) -> CheckOutcome {
    let mut outcome = CheckOutcome::error(check, options, message);
    outcome.duration = started.elapsed();
    outcome
}

fn add_termination_errors(outcome: &mut CheckOutcome, termination: ChildTermination) {
    if let Some(message) = termination.kill_error {
        outcome.push_error(format!("Failed to kill process: {message}"));
    }

    if let Some(message) = termination.wait_error {
        outcome.push_error(format!("Failed to reap process: {message}"));
    }
}

#[cfg(unix)]
fn terminate_child(child: &mut Child) -> ChildTermination {
    let process_group_id = -(child.id() as libc::pid_t);

    // SAFETY: kill receives a process-group id derived from this live child.
    // It does not dereference pointers or touch Rust-managed memory.
    let kill_error = match unsafe { libc::kill(process_group_id, libc::SIGKILL) } {
        0 => None,
        _ => Some(io::Error::last_os_error().to_string()),
    };

    wait_after_kill(child, kill_error)
}

#[cfg(not(unix))]
fn terminate_child(child: &mut Child) -> ChildTermination {
    let kill_error = child.kill().err().map(|err| err.to_string());
    wait_after_kill(child, kill_error)
}

fn wait_after_kill(child: &mut Child, kill_error: Option<String>) -> ChildTermination {
    let (status, wait_error) = match child.wait() {
        Ok(status) => (Some(status), None),
        Err(err) => (None, Some(err.to_string())),
    };

    ChildTermination {
        status,
        kill_error,
        wait_error,
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

struct CompletedRun {
    process_end: ProcessEnd,
    stdout: Result<String, String>,
    stderr: Result<String, String>,
    duration: Duration,
}

enum OutputCapture {
    Reader(thread::JoinHandle<io::Result<Vec<u8>>>),
    Missing(&'static str),
}

#[derive(Debug)]
enum ProcessEnd {
    Exited(ExitStatus),
    TimedOut {
        timeout: Duration,
        termination: ChildTermination,
    },
    WaitError {
        message: String,
        termination: ChildTermination,
    },
}

#[derive(Debug)]
struct ChildTermination {
    status: Option<ExitStatus>,
    kill_error: Option<String>,
    wait_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, name: &str, command: &str) -> Check {
        Check {
            id: id.to_string(),
            name: name.to_string(),
            command: command.to_string(),
        }
    }

    fn profile(checks: Vec<Check>) -> CheckProfile {
        CheckProfile {
            name: "Test".to_string(),
            description: "Test".to_string(),
            checks,
        }
    }

    #[test]
    fn passing_check_captures_stdout_and_command_details() {
        let outcome = execute_check(
            &check("echo", "Echo", "echo 'hello world'"),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Pass);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.stdout, "hello world\n");
        assert_eq!(outcome.stderr, "");
        assert_eq!(outcome.full_command, "echo 'hello world'");
        assert_eq!(outcome.program, "echo");
        assert_eq!(outcome.args, vec!["hello world"]);
        assert!(!outcome.timed_out);
        assert!(outcome.error_message.is_none());
    }

    #[test]
    fn failing_check_is_not_an_execution_error() {
        let outcome = execute_check(
            &check("false", "False", "false"),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Fail);
        assert_ne!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(outcome.error_message.is_none());
    }

    #[test]
    fn execution_error_is_reported_as_error_outcome() {
        let outcome = execute_check(
            &check("missing", "Missing", "nonexistent_command_xyz"),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Error);
        assert_eq!(outcome.exit_code, None);
        assert_eq!(outcome.program, "nonexistent_command_xyz");
        assert!(
            outcome
                .error_message
                .as_deref()
                .unwrap_or_default()
                .contains("Failed to execute")
        );
    }

    #[test]
    fn profile_continues_after_errors() {
        let results = execute_profile(
            &profile(vec![
                check("missing", "Missing", "nonexistent_command_xyz"),
                check("pass", "Pass", "echo still-ran"),
                check("fail", "Fail", "false"),
            ]),
            &ExecutionOptions::default(),
        );

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].status, CheckStatus::Error);
        assert_eq!(results[1].status, CheckStatus::Pass);
        assert_eq!(results[1].stdout, "still-ran\n");
        assert_eq!(results[2].status, CheckStatus::Fail);
    }

    #[test]
    fn captures_stderr_separately() {
        let outcome = execute_check(
            &check("stderr", "Stderr", "sh -c 'echo out; echo err >&2'"),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Pass);
        assert_eq!(outcome.stdout, "out\n");
        assert_eq!(outcome.stderr, "err\n");
    }

    #[test]
    fn unparsable_command_is_error_outcome() {
        let outcome = execute_check(
            &check("bad-quote", "Bad Quote", "echo 'unterminated"),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Error);
        assert!(outcome.program.is_empty());
        assert!(
            outcome
                .error_message
                .as_deref()
                .unwrap_or_default()
                .contains("unmatched shell quote")
        );
    }

    #[test]
    fn empty_command_is_error_outcome() {
        let outcome = execute_check(
            &check("empty", "Empty", "   "),
            &ExecutionOptions::default(),
        );

        assert_eq!(outcome.status, CheckStatus::Error);
        assert_eq!(outcome.error_message.as_deref(), Some("Command is empty"));
    }

    #[test]
    fn timeout_kills_hanging_check() {
        let outcome = execute_check(
            &check("sleep", "Sleep", "sh -c 'sleep 2'"),
            &ExecutionOptions::default().with_timeout(Duration::from_millis(100)),
        );

        assert_eq!(outcome.status, CheckStatus::Error);
        assert!(outcome.timed_out);
        assert!(
            outcome
                .error_message
                .as_deref()
                .unwrap_or_default()
                .contains("Timed out")
        );
        assert!(outcome.duration < Duration::from_secs(2));
    }

    #[test]
    fn supports_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = execute_check(
            &check("pwd", "Pwd", "pwd"),
            &ExecutionOptions::default().with_working_dir(dir.path()),
        );

        assert_eq!(outcome.status, CheckStatus::Pass);
        assert_eq!(outcome.stdout.trim(), dir.path().to_string_lossy());
        assert_eq!(outcome.working_dir.as_deref(), Some(dir.path()));
    }

    #[test]
    fn profile_api_returns_an_outcome_per_check_in_order() {
        let results = execute_profile(
            &profile(vec![
                check("one", "One", "echo one"),
                check("missing", "Missing", "nonexistent_command_xyz"),
                check("two", "Two", "echo two"),
            ]),
            &ExecutionOptions::default(),
        );

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].check_id, "one");
        assert_eq!(results[1].status, CheckStatus::Error);
        assert_eq!(results[2].check_id, "two");
    }

    #[test]
    fn streaming_run_maps_exit_code_to_status() {
        let pass = run_streaming("true", &ExecutionOptions::default());
        assert_eq!(pass.status, CheckStatus::Pass);
        assert_eq!(pass.exit_code, Some(0));
        assert!(!pass.timed_out);

        let fail = run_streaming("exit 3", &ExecutionOptions::default());
        assert_eq!(fail.status, CheckStatus::Fail);
        assert_eq!(fail.exit_code, Some(3));
        assert!(fail.error_message.is_none());
    }

    #[test]
    fn streaming_run_times_out_a_hanging_command() {
        let outcome = run_streaming(
            "sleep 5",
            &ExecutionOptions::default().with_timeout(Duration::from_millis(100)),
        );

        assert_eq!(outcome.status, CheckStatus::Error);
        assert!(outcome.timed_out);
    }
}
