//! Typed errors for the few things that can stop conductor *itself* from
//! running. A file we can't read, a malformed feed, a missing binary — none of
//! those are errors. They are data: a source that resolves to "unavailable",
//! which narrows the plan. These variants are only for "conductor could not even
//! produce a view": no data dir to anchor reads. They map to exit code 3, the
//! same `NoDataDir` rewind makes.

use std::fmt;

/// Errors that abort a command before it can produce output (exit code 3).
#[derive(Debug)]
pub enum ConductorError {
    /// Neither `$XDG_DATA_HOME` nor `$HOME` resolves, so there's nowhere to read
    /// the suite's contract files from.
    NoDataDir,
}

impl fmt::Display for ConductorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConductorError::NoDataDir => f.write_str(
                "cannot resolve $XDG_DATA_HOME or $HOME; conductor needs one to read suite state",
            ),
        }
    }
}

impl std::error::Error for ConductorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_dir_displays_a_helpful_message() {
        let msg = ConductorError::NoDataDir.to_string();
        assert!(msg.contains("conductor needs one"));
    }
}
