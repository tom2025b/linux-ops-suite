//! Typed errors for the few things that can stop portman *itself* from running.
//! A socket we can't fully resolve is never an error — it's a [`crate::Listener`]
//! with `None` links in its [`crate::model::Owner`]. These variants are only for
//! "portman could not even produce a view": the kernel socket tables are
//! unreadable, or the baseline can't be read/written.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors that abort a command before it can produce output (exit code 3).
#[derive(Debug)]
pub enum PortmanError {
    /// `/proc/net/*` couldn't be read at all — portman has no socket source.
    /// (On Linux this effectively never happens; it's here so a non-Linux or
    /// sandboxed `/proc` fails loudly instead of silently reporting nothing.)
    NoProc { source: io::Error },
    /// `portman diff` was asked to compare against a baseline that doesn't
    /// exist yet.
    NoBaseline { path: PathBuf },
    /// The baseline file exists but couldn't be read or parsed.
    BadBaseline { path: PathBuf, detail: String },
    /// The baseline couldn't be written (e.g. unwritable XDG data dir).
    SaveFailed { path: PathBuf, source: io::Error },
    /// Neither `$HOME` nor `$XDG_DATA_HOME` resolves, so there's nowhere to
    /// anchor the baseline.
    NoDataDir,
}

impl fmt::Display for PortmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortmanError::NoProc { source } => {
                write!(f, "cannot read /proc/net (is this Linux?): {source}")
            }
            PortmanError::NoBaseline { path } => write!(
                f,
                "no baseline yet at {} — run `portman baseline` to record one",
                path.display()
            ),
            PortmanError::BadBaseline { path, detail } => {
                write!(f, "baseline at {} is unreadable: {detail}", path.display())
            }
            PortmanError::SaveFailed { path, source } => {
                write!(f, "could not write baseline {}: {source}", path.display())
            }
            PortmanError::NoDataDir => f.write_str(
                "cannot resolve $XDG_DATA_HOME or $HOME; portman needs one to store the baseline",
            ),
        }
    }
}

impl std::error::Error for PortmanError {}
