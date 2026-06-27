// error — the single typed error for scriptvault-core (via `thiserror`), so a
// frontend can map each variant to a message and branch on the kind of failure
// rather than parse strings. `anyhow` is for the binary only.
//
// What is deliberately NOT an error: the degrade-don't-fail paths (missing/empty
// config, malformed sidecar, missing/corrupt state, a single unreadable file)
// warn and continue. Only caller-actionable failures (a malformed *user* config,
// a state *write* failure, our own broken embedded defaults) surface here.

use std::path::PathBuf;

use thiserror::Error;

/// The crate-wide `Result`: call sites read `crate::Result<T>`.
pub type Result<T> = std::result::Result<T, ScriptVaultError>;

/// Every way a core operation can fail.
#[derive(Debug, Error)]
pub enum ScriptVaultError {
    /// The user's `config.yaml` exists but could not be read from disk.
    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The user's `config.yaml` is present but malformed — fatal for that file
    /// (we name the path and reason rather than silently ignore overrides).
    #[error("malformed config file {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    /// Our OWN embedded `config/default.yaml` failed to parse — a ScriptVault bug.
    #[error("failed to parse embedded default.yaml (this is a ScriptVault bug): {0}")]
    DefaultConfigParse(#[source] serde_yaml::Error),

    /// Valid YAML but semantically invalid (no roots, blank ignore, empty editor).
    #[error("invalid configuration: {0}")]
    ConfigInvalid(String),

    /// A configured glob ignore pattern could not be compiled.
    #[error("invalid ignore pattern {pattern:?}: {source}")]
    BadIgnorePattern {
        pattern: String,
        #[source]
        source: globset::Error,
    },

    /// Persisting user state to disk failed. `context` says which step.
    #[error("{context}: {source}")]
    StateIo {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// Serializing state to JSON failed (typed so `save` has no `unwrap`).
    #[error("failed to serialize state: {0}")]
    StateSerialize(#[source] serde_json::Error),

    /// A script action (open in editor / run) failed.
    #[error("{context}")]
    Action {
        context: String,
        #[source]
        source: Option<std::io::Error>,
    },

    /// A candidate could not be read as UTF-8 text (e.g. a binary in `~/bin`).
    /// The parser returns this so `parse_all` can warn-and-skip it.
    #[error("cannot read as text: {path}: {source}")]
    NotReadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ScriptVaultError {
    /// Build a `StateIo` from a context string + io error.
    pub(crate) fn state_io(context: impl Into<String>, source: std::io::Error) -> Self {
        ScriptVaultError::StateIo {
            context: context.into(),
            source,
        }
    }

    /// Build an `Action` we describe ourselves (no OS source) — e.g. a non-zero
    /// exit status or an empty editor command.
    pub(crate) fn action(context: impl Into<String>) -> Self {
        ScriptVaultError::Action {
            context: context.into(),
            source: None,
        }
    }

    /// Build an `Action` wrapping an io error (e.g. a failed `Command::status()`).
    pub(crate) fn action_io(context: impl Into<String>, source: std::io::Error) -> Self {
        ScriptVaultError::Action {
            context: context.into(),
            source: Some(source),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_path_and_reason_for_config_parse() {
        // A frontend prints `{err}`; the message must name the file and the cause
        // so the user can find and fix it.
        let yaml_err = serde_yaml::from_str::<crate::Config>("roots: [unterminated")
            .err()
            .map(ScriptVaultError::DefaultConfigParse)
            .unwrap();
        let msg = yaml_err.to_string();
        assert!(msg.contains("ScriptVault bug"), "got: {msg}");
    }

    #[test]
    fn config_invalid_carries_explanation() {
        let e = ScriptVaultError::ConfigInvalid("no scan roots configured".into());
        assert_eq!(
            e.to_string(),
            "invalid configuration: no scan roots configured"
        );
    }

    #[test]
    fn source_chain_is_preserved() {
        use std::error::Error;
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = ScriptVaultError::state_io("failed to write state file /x/state.json", io);
        // `source()` exposes the underlying io error so `{:#}` and loggers see it.
        assert!(e.source().is_some(), "StateIo must expose its io source");
        assert!(e.to_string().contains("/x/state.json"));
    }
}
