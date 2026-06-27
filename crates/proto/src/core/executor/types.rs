use std::path::PathBuf;
use std::time::Duration;

use crate::core::checks::Check;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Error,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "Pass"),
            Self::Fail => write!(f, "Fail"),
            Self::Error => write!(f, "Error"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionOptions {
    pub working_dir: Option<PathBuf>,
    pub timeout: Option<Duration>,
}

// Default timeout for an interactive, operator-confirmed `command:` step. Shorter
// than the batch DEFAULT_TIMEOUT: the operator is watching it live, so a hang
// should hand control back in minutes, not ten.
pub const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            working_dir: None,
            timeout: Some(DEFAULT_TIMEOUT),
        }
    }
}

impl ExecutionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub check_id: String,
    pub name: String,
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub full_command: String,
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub timed_out: bool,
    pub error_message: Option<String>,
}

pub type CheckResult = CheckOutcome;

impl CheckOutcome {
    pub(crate) fn new(check: &Check, options: &ExecutionOptions, status: CheckStatus) -> Self {
        Self {
            check_id: check.id.clone(),
            name: check.name.clone(),
            status,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::ZERO,
            full_command: check.command.clone(),
            program: String::new(),
            args: Vec::new(),
            working_dir: options.working_dir.clone(),
            timed_out: false,
            error_message: None,
        }
    }

    pub(crate) fn error(
        check: &Check,
        options: &ExecutionOptions,
        message: impl Into<String>,
    ) -> Self {
        let mut outcome = Self::new(check, options, CheckStatus::Error);
        outcome.error_message = Some(message.into());
        outcome
    }

    pub(crate) fn with_command(mut self, program: &str, args: &[String]) -> Self {
        self.program = program.to_string();
        self.args = args.to_vec();
        self
    }

    pub(crate) fn push_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        match &mut self.error_message {
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&message);
            }
            None => self.error_message = Some(message),
        }
    }

    pub fn passed(&self) -> bool {
        self.status == CheckStatus::Pass
    }

    pub fn failed(&self) -> bool {
        self.status == CheckStatus::Fail
    }

    pub fn errored(&self) -> bool {
        self.status == CheckStatus::Error
    }
}

// -----------------------------------------------------------------------------
// StreamedOutcome — the result of a STREAMING command run (output not captured).
// -----------------------------------------------------------------------------
// The interactive `command:` step runs through the same timeout + process-group
// machinery as a captured check, but inherits the terminal so the operator
// watches output live. There's nothing to capture, so the result is just the
// disposition: did it pass, and (for the operator's message) the exit code, a
// timeout flag, and a spawn/wait error if any.
#[derive(Debug, Clone)]
pub struct StreamedOutcome {
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub timeout: Option<Duration>,
    pub error_message: Option<String>,
}

impl StreamedOutcome {
    pub fn passed(&self) -> bool {
        self.status == CheckStatus::Pass
    }
}
