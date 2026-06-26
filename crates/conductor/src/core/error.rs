//! One error type for the whole binary.

use std::fmt;

/// Anything that can go wrong while running a command.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    NotFound(String),
    /// The canonical snapshot was present but unreadable — malformed, or a schema
    /// version this build doesn't understand (surfaced from workstate-schema's loader).
    Snapshot(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "invalid JSON: {e}"),
            Error::NotFound(what) => write!(f, "not found: {what}"),
            Error::Snapshot(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
