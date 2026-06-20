//! Typed errors for the few things that can stop rewind *itself* from running.
//! A file we can't read, a missing capture-set path, an unresolvable owner —
//! none of those are errors. They are data: a [`crate::model::CaptureEntry`]
//! with `unreadable` set, or simply an absent path. These variants are only for
//! "rewind could not even produce a view": the store can't be read/written,
//! there's nowhere to anchor it, the capture set resolved to nothing, or a
//! requested capture id doesn't exist. They map to exit code 3, the same
//! `NoBaseline`/`BadBaseline` split tripwire makes.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors that abort a command before it can produce output (exit code 3).
#[derive(Debug)]
pub enum RewindError {
    /// Neither `$HOME` nor `$XDG_DATA_HOME` resolves, so there's nowhere to
    /// anchor the store.
    NoDataDir,
    /// A read command (`list`, `show`, `diff`, `restore`) needs a store that
    /// doesn't exist yet — nothing has been captured.
    NoStore { path: PathBuf },
    /// A capture manifest exists but couldn't be read or parsed (corrupt, or a
    /// schema version this build doesn't understand).
    BadManifest { path: PathBuf, detail: String },
    /// A capture or object couldn't be written (e.g. unwritable XDG data dir).
    SaveFailed { path: PathBuf, source: io::Error },
    /// The capture set resolved to zero paths (e.g. `--config` pointed at an
    /// empty file). There is nothing to capture.
    EmptySet,
    /// The `--config` file was given but couldn't be read.
    BadConfig { path: PathBuf, detail: String },
    /// A capture id/selector (`show`/`diff`/`restore`) matched no capture, or a
    /// prefix matched more than one.
    UnknownCapture { selector: String },
    /// A capture-vs-live diff was asked for a capture taken from an explicit
    /// `--path` set, but this run gave no `--path`/`--config` to reconstruct it —
    /// so the live side would silently be a *different* set. Refused rather than
    /// comparing the wrong files.
    SetMismatch { selector: String },
    /// A `prune --older-than` duration couldn't be parsed (e.g. `5y`, `abc`).
    BadDuration { spec: String },
}

impl fmt::Display for RewindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewindError::NoDataDir => f.write_str(
                "cannot resolve $XDG_DATA_HOME or $HOME; rewind needs one to store captures",
            ),
            RewindError::NoStore { path } => write!(
                f,
                "no capture store yet at {} — run `rewind capture` to record one",
                path.display()
            ),
            RewindError::BadManifest { path, detail } => {
                write!(f, "capture at {} is unreadable: {detail}", path.display())
            }
            RewindError::SaveFailed { path, source } => {
                write!(f, "could not write {}: {source}", path.display())
            }
            RewindError::EmptySet => f.write_str(
                "capture set is empty — nothing to capture (give --path, a --config, or rely on the built-in set)",
            ),
            RewindError::BadConfig { path, detail } => {
                write!(f, "could not read config {}: {detail}", path.display())
            }
            RewindError::UnknownCapture { selector } => {
                write!(f, "no capture matches '{selector}'")
            }
            RewindError::SetMismatch { selector } => write!(
                f,
                "capture '{selector}' was taken from explicit --path arguments; \
                 re-run `rewind diff {selector}` with the same --path/--config so \
                 the live side compares the same files"
            ),
            RewindError::BadDuration { spec } => write!(
                f,
                "invalid --older-than '{spec}'; use <n><unit> with unit s/m/h/d (e.g. 30d, 12h)"
            ),
        }
    }
}

impl std::error::Error for RewindError {}
