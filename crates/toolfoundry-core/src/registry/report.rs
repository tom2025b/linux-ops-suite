use std::path::PathBuf;

use chrono::NaiveDate;
use serde::Serialize;

use crate::manifest::{Criticality, ToolKind, ToolManifest};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Sorted catalog of manifest summaries.
pub struct ManifestCatalog {
    pub manifest_count: usize,
    pub manifests: Vec<ManifestSummary>,
}

impl ManifestCatalog {
    /// Create a catalog and sort summaries by id and path.
    pub fn new(mut manifests: Vec<ManifestSummary>) -> Self {
        manifests.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.path.cmp(&right.path))
        });

        Self {
            manifest_count: manifests.len(),
            manifests,
        }
    }

    /// Return true when the catalog has no manifests.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Compact manifest metadata used by catalog and TUI surfaces.
pub struct ManifestSummary {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub owner: String,
    pub project: String,
    pub criticality: String,
    pub lifecycle_state: String,
    pub review_after: NaiveDate,
    pub tags: Vec<String>,
    pub path: PathBuf,
}

impl ManifestSummary {
    /// Project a parsed manifest into catalog summary metadata.
    pub fn from_manifest(path: PathBuf, manifest: ToolManifest) -> Self {
        Self {
            id: manifest.identity.id,
            display_name: manifest.identity.display_name,
            kind: tool_kind(&manifest.identity.kind).to_string(),
            owner: manifest.ownership.owner,
            project: manifest.ownership.project,
            criticality: criticality(&manifest.ownership.criticality).to_string(),
            lifecycle_state: manifest.lifecycle.state.to_string(),
            review_after: manifest.lifecycle.review_after,
            tags: manifest.identity.tags,
            path,
        }
    }
}

fn tool_kind(kind: &ToolKind) -> &'static str {
    match kind {
        ToolKind::Script => "script",
        ToolKind::Binary => "binary",
        ToolKind::Project => "project",
        ToolKind::Service => "service",
    }
}

fn criticality(criticality: &Criticality) -> &'static str {
    match criticality {
        Criticality::Low => "low",
        Criticality::Medium => "medium",
        Criticality::High => "high",
    }
}
