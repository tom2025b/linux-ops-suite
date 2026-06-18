//! Typed errors for the few things that can stop rex-doctor *itself* from
//! running. A failing *check* is never an error — it's a [`crate::model::Check`]
//! with a `Fail` status. These variants are only for "the doctor could not even
//! start" (exit code 3): bad CLI input, or an environment it can't reason about.

use std::fmt;

/// Errors that abort the whole run before (or instead of) producing a report.
#[derive(Debug)]
pub enum DoctorError {
    /// An unknown check id or category was passed to `--only`/`--skip`.
    UnknownSelector { value: String },
    /// Neither `$HOME` nor a usable base dir could be resolved, so the env
    /// checks have nothing to anchor to.
    NoHome,
}

impl fmt::Display for DoctorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DoctorError::UnknownSelector { value } => write!(
                f,
                "unknown check or category '{value}' (run `rex-doctor --list` to see valid ids)"
            ),
            DoctorError::NoHome => {
                f.write_str("cannot resolve $HOME; rex-doctor needs it to locate the suite")
            }
        }
    }
}

impl std::error::Error for DoctorError {}
