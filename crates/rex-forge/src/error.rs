//! Typed errors. Map cleanly to CLI exit codes and TUI status flashes.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("unknown component: {0}")]
    UnknownComponent(String),
    #[error("component `{component}` does not apply to base `{base}`")]
    BaseMismatch { component: String, base: String },
    #[error("components `{a}` and `{b}` conflict")]
    Conflict { a: String, b: String },
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failed to render `{file}`: {reason}")]
    Template { file: String, reason: String },
    #[error("target `{target}` has no anchor `{anchor}`")]
    MissingAnchor { target: String, anchor: String },
}

#[derive(Debug, Error)]
pub enum WriteError {
    #[error("`{0}` is not empty — re-run with --force to overwrite")]
    TargetNotEmpty(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("git error: {0}")]
    Git(String),
}

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Write(#[from] WriteError),
}

impl ForgeError {
    pub fn exit_code(&self) -> i32 {
        match self {
            ForgeError::Resolve(_) => 2,
            ForgeError::Render(_) => 3,
            ForgeError::Write(_) => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_conflict_message_names_both() {
        let e = ResolveError::Conflict { a: "anyhow".into(), b: "thiserror".into() };
        let msg = e.to_string();
        assert!(msg.contains("anyhow") && msg.contains("thiserror"));
    }

    #[test]
    fn forge_error_maps_to_nonzero_exit_code() {
        let e = ForgeError::from(WriteError::TargetNotEmpty("./x".into()));
        assert_ne!(e.exit_code(), 0);
    }

    #[test]
    fn write_error_converts_into_forge_error() {
        let e: ForgeError = WriteError::Io("disk full".into()).into();
        assert!(matches!(e, ForgeError::Write(_)));
    }
}
