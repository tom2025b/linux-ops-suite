//! Execute check profiles and capture detailed outcomes for each check.

mod command;
mod runner;
mod types;

pub use runner::{execute_check, execute_profile, run_streaming};
pub use types::{
    CheckOutcome, CheckResult, CheckStatus, ExecutionOptions, INTERACTIVE_TIMEOUT, StreamedOutcome,
};
