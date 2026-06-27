use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Desired-state manifest for one owned tool.
pub struct ToolManifest {
    pub schema_version: u16,
    pub kind: ManifestKind,
    pub identity: Identity,
    pub ownership: Ownership,
    pub source: Source,
    pub install: Install,
    pub links: Links,
    pub health: Health,
    pub lifecycle: Lifecycle,
}

impl ToolManifest {
    /// Validate schema and repository-specific invariants for this manifest.
    pub fn validate(&self) -> Result<()> {
        super::validation::validate_manifest(self)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
/// Top-level manifest kind.
pub enum ManifestKind {
    Tool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Stable identity and search metadata for a tool.
pub struct Identity {
    pub id: String,
    pub display_name: String,
    pub summary: String,
    pub kind: ToolKind,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Tool implementation category.
pub enum ToolKind {
    Script,
    Binary,
    Project,
    Service,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Human and project ownership metadata.
pub struct Ownership {
    pub owner: String,
    pub maintainer: String,
    pub project: String,
    pub repo: String,
    pub local_path: String,
    pub criticality: Criticality,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Operational importance of a tool.
pub enum Criticality {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Source language, primary entrypoint, and build strategy.
pub struct Source {
    pub language: String,
    pub primary_file: String,
    pub build: BuildStrategy,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Build workflow needed before an install artifact is available.
pub enum BuildStrategy {
    None,
    Cargo,
    Make,
    Custom,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Install intent for the tool artifact.
pub struct Install {
    pub method: InstallMethod,
    pub artifact_path: String,
    pub target_path: String,
    pub requires_sudo: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Supported install mechanisms recorded by the manifest.
pub enum InstallMethod {
    Symlink,
    Copy,
    Package,
    Manual,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Desired symlink ownership for this tool.
pub struct Links {
    pub managed: bool,
    pub desired: Vec<DesiredLink>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// One desired source-to-target symlink.
pub struct DesiredLink {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Health checks declared for this tool.
pub struct Health {
    pub checks: Vec<HealthCheck>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// One health check declaration from the manifest.
pub struct HealthCheck {
    pub id: String,
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Supported health check implementations.
pub enum HealthCheckType {
    FileExists,
    Executable,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Lifecycle state and review policy for a tool.
pub struct Lifecycle {
    pub state: LifecycleState,
    pub review_after: NaiveDate,
    pub replacement: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Lifecycle state used by ToolFoundry's transition rules.
pub enum LifecycleState {
    Experimental,
    Active,
    Stale,
    Risky,
    Broken,
    Deprecated,
    Archived,
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Experimental => "experimental",
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Risky => "risky",
            Self::Broken => "broken",
            Self::Deprecated => "deprecated",
            Self::Archived => "archived",
        };

        formatter.write_str(value)
    }
}
