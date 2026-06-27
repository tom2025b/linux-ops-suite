//! Centralized error handling for the entire Bulwark library.
//!
//! Design goals (Bulwark Architect rules):
//! - All library fallible operations return `Result<T, BulwarkError>`.
//! - Use thiserror for ergonomic, typed errors with good Display messages.
//! - Keep variants specific enough for callers to handle differently when needed,
//!   but not so granular that we have 50 variants in the MVP.
//! - The binary/CLI layer (`crates/bulwark/src/main.rs`) is allowed to use `anyhow` for ergonomic
//!   error handling and to add nice context for end users. The library itself
//!   always uses `BulwarkError` (thiserror) so all errors are typed and
//!   machine-inspectable.
//!
//! This file is intentionally small and focused. It will grow new variants as we
//! add scanner, rules, and reporting layers — that is expected and correct.

use std::path::PathBuf;

use thiserror::Error;

/// The single error type used by all Bulwark library code.
///
/// Every public function in `bulwark::*` that can fail returns `Result<_, BulwarkError>`.
/// This gives callers a consistent, machine-readable way to understand what went wrong.
#[derive(Error, Debug)]
pub enum BulwarkError {
    /// I/O error while reading a file, walking a directory, etc.
    /// We wrap the underlying `std::io::Error` so callers can still inspect kind if needed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// YAML parsing or serialization error.
    /// Comes from the `serde_yaml_bw` crate (the maintained `serde_yaml` fork,
    /// imported under the `serde_yaml` name in Cargo.toml).
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// JSON serialization error (when rendering `--json` reports).
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Configuration-specific problems (missing required fields, invalid values,
    /// conflicting settings, etc.). We use a structured message so we can give
    /// precise, actionable feedback to the user via `bulwark config-check`.
    #[error("configuration error: {message}")]
    Config { message: String },

    /// A path that was expected to be a directory was something else (or didn't exist).
    #[error("not a directory: {path}")]
    NotADirectory { path: PathBuf },

    /// Generic "something is wrong with this path" for cases where we want to
    /// surface the path prominently (we always show full paths per guard rails).
    #[error("path error: {path}: {message}")]
    Path { path: PathBuf, message: String },

    /// Rule definition or rule engine error (bad YAML, invalid match spec, etc.).
    #[error("rule error: {message}")]
    Rule { message: String },
}

// Convenience constructors so call sites stay readable and short.

impl BulwarkError {
    /// Create a configuration error with a clear message.
    pub fn config<S: Into<String>>(message: S) -> Self {
        BulwarkError::Config {
            message: message.into(),
        }
    }

    /// Create a "not a directory" error for a given path.
    pub fn not_a_directory(path: impl Into<PathBuf>) -> Self {
        BulwarkError::NotADirectory { path: path.into() }
    }

    /// Create a rule error with a clear message.
    pub fn rule<S: Into<String>>(message: S) -> Self {
        BulwarkError::Rule {
            message: message.into(),
        }
    }
}
