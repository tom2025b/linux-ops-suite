use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
/// Typed errors returned by the ToolFoundry core crate.
pub enum ToolFoundryError {
    #[error("failed to read config at {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config YAML at {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to serialize config YAML at {path}: {source}")]
    ConfigSerialize {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("config already exists at {path}; use --force to overwrite it")]
    ConfigExists { path: PathBuf },

    #[error("failed to write config at {path}: {source}")]
    ConfigWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read manifest at {path}: {source}")]
    ManifestRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest YAML at {path}: {source}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to read registry directory at {path}: {source}")]
    RegistryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("install plan for {tool_id} is blocked; resolve manual actions before applying")]
    InstallBlocked { tool_id: String },

    #[error(
        "internal error: apply reached a manual-intervention link ({link_status}); the plan should have blocked it"
    )]
    InstallApplyInvariant { link_status: String },

    #[error("failed to write install target at {path}: {source}")]
    InstallWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manifest validation failed: {0}")]
    ManifestValidation(String),

    #[error(
        "duplicate tool id {id:?} found in manifests {first_path:?} and {second_path:?}; ids must be unique across the registry"
    )]
    DuplicateToolId {
        id: String,
        first_path: String,
        second_path: String,
    },

    #[error("failed to expand path {path}: {reason}")]
    PathExpansion { path: String, reason: String },
}

/// Core result type using `ToolFoundryError`.
pub type Result<T> = std::result::Result<T, ToolFoundryError>;
