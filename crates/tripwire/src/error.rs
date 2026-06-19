//! Typed errors for the few things that can stop tripwire *itself* from running.
//! A file we can't read, a missing watched path, an unresolvable owner — none of
//! those are errors. They are data: an [`crate::model::Entry`] with `unreadable`
//! set, or simply an absent path that the diff reports as `removed`. These
//! variants are only for "tripwire could not even produce a view": the baseline
//! can't be read/written, there's nowhere to anchor it, or the watch set
//! resolved to nothing to look at.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors that abort a command before it can produce output (exit code 3).
#[derive(Debug)]
pub enum TripwireError {
    /// `tripwire diff`/`verify` was asked to compare against a baseline that
    /// doesn't exist yet.
    NoBaseline { path: PathBuf },
    /// The baseline file exists but couldn't be read or parsed (corrupt, or a
    /// schema version this build doesn't understand).
    BadBaseline { path: PathBuf, detail: String },
    /// The baseline couldn't be written (e.g. unwritable XDG data dir).
    SaveFailed { path: PathBuf, source: io::Error },
    /// Neither `$HOME` nor `$XDG_DATA_HOME` resolves, so there's nowhere to
    /// anchor the baseline.
    NoDataDir,
    /// The watch set resolved to zero paths (e.g. `--config` pointed at an empty
    /// file). There is nothing to scan, so the command can't produce a view.
    EmptyWatchSet,
    /// The `--config` file was given but couldn't be read.
    BadConfig { path: PathBuf, detail: String },
}

impl fmt::Display for TripwireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TripwireError::NoBaseline { path } => write!(
                f,
                "no baseline yet at {} — run `tripwire baseline` to record one",
                path.display()
            ),
            TripwireError::BadBaseline { path, detail } => {
                write!(f, "baseline at {} is unreadable: {detail}", path.display())
            }
            TripwireError::SaveFailed { path, source } => {
                write!(f, "could not write baseline {}: {source}", path.display())
            }
            TripwireError::NoDataDir => f.write_str(
                "cannot resolve $XDG_DATA_HOME or $HOME; tripwire needs one to store the baseline",
            ),
            TripwireError::EmptyWatchSet => f.write_str(
                "watch set is empty — nothing to watch (give --path, a --config, or rely on the built-in set)",
            ),
            TripwireError::BadConfig { path, detail } => {
                write!(f, "could not read config {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for TripwireError {}
