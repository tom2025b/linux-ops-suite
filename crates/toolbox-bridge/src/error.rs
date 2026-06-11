use thiserror::Error;

/// Typed errors for every failure the bridge can hit, in pipeline order.
///
/// Each variant's message is the full operator-facing diagnosis: what went
/// wrong, where, and (where there is one) the obvious fix. `main` prints these
/// verbatim, so the wording here IS the CLI's error UX.
#[derive(Debug, Error)]
pub enum BridgeError {
    /// The snapshot file does not exist. Distinct from `Io`: a snapshot that
    /// has not been compiled yet is the expected first-run state, and the fix
    /// (`run workstate`) is known.
    #[error("workstate snapshot not found at {0}\nrun `workstate` first to compile one")]
    SnapshotNotFound(String),

    /// The snapshot exists but could not be read (permissions, etc.).
    #[error("could not read workstate snapshot {path}: {source}")]
    SnapshotIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// The snapshot is not valid JSON, or is JSON that does not match the
    /// Workstate `Snapshot` contract.
    #[error("workstate snapshot {path} is malformed: {reason}")]
    SnapshotParse { path: String, reason: String },

    /// The snapshot declares a schema version this bridge does not understand.
    /// Per CONTRACT_RULES.md, consumers validate `schema_version` first and
    /// refuse on a major they don't know.
    #[error("workstate snapshot declares schema_version {found:?}; this toolbox-bridge understands {supported}")]
    UnsupportedSchema { found: Option<i64>, supported: u32 },

    /// The findings section carries no usable Bulwark data (Missing, Failed,
    /// or UnsupportedVersion). The snapshot itself was fine — Bulwark's feed
    /// into Workstate is what's absent or broken.
    #[error("no Bulwark findings to bridge: snapshot findings section is {status}\nrun `bulwark workstate-feed` and then `workstate` to refresh it")]
    FindingsUnavailable { status: String },

    /// The output feed could not be written.
    #[error("could not write sidecar feed {path}: {source}")]
    FeedWrite {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Neither `$XDG_DATA_HOME` nor `$HOME` is set, so a default path cannot
    /// be resolved. Only possible when the caller passed no explicit path.
    #[error("cannot resolve the default {what} path: neither $XDG_DATA_HOME nor $HOME is set\npass an explicit path instead")]
    NoDefaultPath { what: &'static str },
}
