//! Optional sidecar metadata for discovered scripts.
//!
//! Sidecars are best-effort enrichment. A missing, unreadable, or malformed
//! sidecar must not prevent Bulwark from inventorying the script itself. A
//! malformed one is *reported* (via [`SidecarOutcome::Malformed`], which the
//! engine turns into a scan warning) rather than silently discarded, so a typo
//! in a user's annotation surfaces instead of looking like it took effect.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Optional structured metadata loaded from a file next to the script.
///
/// Supported naming conventions:
/// - `script.sh.bulwark.yaml`
/// - `script.bulwark.yaml` for extensionless scripts
///
/// Every field is optional: a sidecar that sets only `risk` (and omits `tags`,
/// `description`, etc.) is valid. `#[serde(default)]` on each field is what makes
/// that true — without it, a sidecar omitting `tags` would fail to deserialize
/// and be silently dropped, so a user's `risk:`/`category:`/`owner:` override
/// would never take effect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidecarMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// Overrides the rule-engine risk for this script when set (applied in
    /// `engine::collect_classified_inventory`). Must be one of
    /// `low|medium|high|critical`.
    pub risk: Option<String>,
    /// Overrides the rule-engine category for this script when set.
    pub category: Option<String>,
    /// Overrides the rule-engine owner for this script when set.
    pub owner: Option<String>,
}

/// The result of looking for a script's sidecar.
///
/// We distinguish "no sidecar" from "a sidecar file exists but could not be
/// parsed" so the caller can surface the malformed case as a warning instead of
/// silently behaving as if the user never wrote one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SidecarOutcome {
    /// No sidecar file was found next to the script.
    None,
    /// A sidecar was found and parsed successfully.
    Loaded(SidecarMetadata),
    /// A sidecar file exists but is unreadable or malformed. Carries the path
    /// and a short reason for a user-facing warning.
    Malformed { path: PathBuf, reason: String },
}

/// Load sidecar metadata next to `script_path`.
///
/// Returns [`SidecarOutcome`] so a present-but-broken sidecar can be reported
/// rather than vanishing. Only the *first existing* candidate path is
/// considered: if it fails to parse we surface that error rather than silently
/// trying the fallback and pretending the malformed file isn't there.
pub(super) fn load_sidecar(script_path: &Path) -> SidecarOutcome {
    let file_name = script_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    let primary = script_path.with_file_name(format!("{file_name}.bulwark.yaml"));
    let fallback = script_path.with_file_name(format!(
        "{}.bulwark.yaml",
        Path::new(file_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(file_name)
    ));

    for candidate in [primary, fallback] {
        if !candidate.exists() {
            continue;
        }
        return match std::fs::read_to_string(&candidate) {
            Err(e) => SidecarOutcome::Malformed {
                path: candidate,
                reason: format!("could not read sidecar: {e}"),
            },
            Ok(yaml) => match serde_yaml::from_str::<SidecarMetadata>(&yaml) {
                Ok(metadata) => SidecarOutcome::Loaded(metadata),
                Err(e) => SidecarOutcome::Malformed {
                    path: candidate,
                    reason: format!("invalid sidecar YAML: {e}"),
                },
            },
        };
    }

    SidecarOutcome::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn loaded(outcome: SidecarOutcome) -> SidecarMetadata {
        match outcome {
            SidecarOutcome::Loaded(m) => m,
            other => panic!("expected a loaded sidecar, got {other:?}"),
        }
    }

    #[test]
    fn loads_primary_sidecar_when_valid() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("tool.sh");
        let sidecar = dir.path().join("tool.sh.bulwark.yaml");

        fs::write(&script, "#!/bin/bash\n").unwrap();
        fs::write(
            sidecar,
            "description: Test tool\ntags:\n  - local\nrisk: low\ncategory: script\nowner: user\n",
        )
        .unwrap();

        let metadata = loaded(load_sidecar(&script));
        assert_eq!(metadata.description.as_deref(), Some("Test tool"));
        assert_eq!(metadata.tags, vec!["local"]);
    }

    #[test]
    fn loads_partial_sidecar_that_omits_tags() {
        // Regression: `tags` lacked #[serde(default)], so a sidecar that set only
        // risk/category/owner (the common override case) failed to deserialize and
        // was silently dropped — meaning overrides never applied. A partial
        // sidecar must now load cleanly.
        let dir = tempdir().unwrap();
        let script = dir.path().join("tool.sh");
        let sidecar = dir.path().join("tool.sh.bulwark.yaml");

        fs::write(&script, "#!/bin/bash\n").unwrap();
        fs::write(sidecar, "risk: high\ncategory: audited\n").unwrap();

        let metadata = loaded(load_sidecar(&script));
        assert_eq!(metadata.risk.as_deref(), Some("high"));
        assert_eq!(metadata.category.as_deref(), Some("audited"));
        assert!(metadata.tags.is_empty());
        assert_eq!(metadata.owner, None);
    }

    #[test]
    fn malformed_sidecar_is_reported_not_silently_dropped() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("tool.sh");
        let sidecar = dir.path().join("tool.sh.bulwark.yaml");

        fs::write(&script, "#!/bin/bash\n").unwrap();
        fs::write(&sidecar, "description: [unterminated").unwrap();

        match load_sidecar(&script) {
            SidecarOutcome::Malformed { path, reason } => {
                assert_eq!(path, sidecar);
                assert!(reason.contains("invalid sidecar YAML"));
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn absent_sidecar_is_none() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("tool.sh");
        fs::write(&script, "#!/bin/bash\n").unwrap();
        assert_eq!(load_sidecar(&script), SidecarOutcome::None);
    }
}
