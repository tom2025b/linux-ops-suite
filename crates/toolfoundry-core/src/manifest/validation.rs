use std::collections::HashSet;

use crate::{
    error::{Result, ToolFoundryError},
    manifest::{InstallMethod, Links, ToolManifest},
};

const SUPPORTED_SCHEMA_VERSION: u16 = 1;

pub fn validate_manifest(manifest: &ToolManifest) -> Result<()> {
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return invalid(format!(
            "unsupported schema_version {}; expected {}",
            manifest.schema_version, SUPPORTED_SCHEMA_VERSION
        ));
    }

    validate_identity(manifest)?;
    validate_ownership(manifest)?;
    validate_source(manifest)?;
    validate_install(manifest)?;
    validate_links(&manifest.links)?;
    validate_health(manifest)?;
    validate_lifecycle(manifest)?;

    Ok(())
}

fn validate_identity(manifest: &ToolManifest) -> Result<()> {
    validate_required("identity.id", &manifest.identity.id)?;
    validate_id("identity.id", &manifest.identity.id)?;
    validate_required("identity.display_name", &manifest.identity.display_name)?;
    validate_required("identity.summary", &manifest.identity.summary)?;
    validate_nonempty_list("identity.tags", &manifest.identity.tags)?;

    for tag in &manifest.identity.tags {
        validate_id("identity.tags[]", tag)?;
    }

    Ok(())
}

fn validate_ownership(manifest: &ToolManifest) -> Result<()> {
    validate_required("ownership.owner", &manifest.ownership.owner)?;
    validate_required("ownership.maintainer", &manifest.ownership.maintainer)?;
    validate_required("ownership.project", &manifest.ownership.project)?;
    validate_required("ownership.repo", &manifest.ownership.repo)?;
    validate_required("ownership.local_path", &manifest.ownership.local_path)?;

    Ok(())
}

fn validate_source(manifest: &ToolManifest) -> Result<()> {
    validate_required("source.language", &manifest.source.language)?;
    validate_required("source.primary_file", &manifest.source.primary_file)?;

    Ok(())
}

fn validate_install(manifest: &ToolManifest) -> Result<()> {
    validate_required("install.artifact_path", &manifest.install.artifact_path)?;
    validate_required("install.target_path", &manifest.install.target_path)?;

    // For symlink installs the install paths are redundant with links.desired. Require
    // them to refer to a declared link so the two cannot silently diverge (the root
    // cause behind the drift-status bug): artifact_path must be some link's source and
    // target_path must be some link's target. Other install methods (copy/package/
    // manual) do not manage symlinks, so the cross-check does not apply.
    if manifest.install.method == InstallMethod::Symlink {
        let artifact = &manifest.install.artifact_path;
        if !manifest
            .links
            .desired
            .iter()
            .any(|link| &link.source == artifact)
        {
            return invalid(format!(
                "install.artifact_path {artifact} must match a links.desired[].source for symlink installs"
            ));
        }

        let target = &manifest.install.target_path;
        if !manifest
            .links
            .desired
            .iter()
            .any(|link| &link.target == target)
        {
            return invalid(format!(
                "install.target_path {target} must match a links.desired[].target for symlink installs"
            ));
        }
    }

    Ok(())
}

fn validate_links(links: &Links) -> Result<()> {
    if links.managed {
        validate_nonempty_list("links.desired", &links.desired)?;
    }

    let mut seen = HashSet::new();
    for desired in &links.desired {
        validate_required("links.desired[].source", &desired.source)?;
        validate_required("links.desired[].target", &desired.target)?;

        if !seen.insert((&desired.source, &desired.target)) {
            return invalid(format!(
                "duplicate desired link from {} to {}",
                desired.source, desired.target
            ));
        }
    }

    Ok(())
}

fn validate_health(manifest: &ToolManifest) -> Result<()> {
    validate_nonempty_list("health.checks", &manifest.health.checks)?;

    let mut seen = HashSet::new();
    for check in &manifest.health.checks {
        validate_required("health.checks[].id", &check.id)?;
        validate_id("health.checks[].id", &check.id)?;
        validate_required("health.checks[].path", &check.path)?;

        if !seen.insert(&check.id) {
            return invalid(format!("duplicate health check id {}", check.id));
        }
    }

    Ok(())
}

fn validate_lifecycle(manifest: &ToolManifest) -> Result<()> {
    if let Some(replacement) = &manifest.lifecycle.replacement {
        validate_required("lifecycle.replacement", replacement)?;
    }

    Ok(())
}

fn validate_required(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return invalid(format!("{field} must not be empty"));
    }

    Ok(())
}

fn validate_nonempty_list<T>(field: &str, value: &[T]) -> Result<()> {
    if value.is_empty() {
        return invalid(format!("{field} must contain at least one item"));
    }

    Ok(())
}

fn validate_id(field: &str, id: &str) -> Result<()> {
    let valid = id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && id.starts_with(|ch: char| ch.is_ascii_lowercase())
        && id.ends_with(|ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit());

    if !valid {
        return invalid(format!(
            "{field} must be kebab-case ASCII, such as backup-home"
        ));
    }

    Ok(())
}

fn invalid<T>(message: String) -> Result<T> {
    Err(ToolFoundryError::ManifestValidation(message))
}

#[cfg(test)]
mod tests {
    use crate::test_support::ManifestFixture;

    use super::*;

    fn valid_manifest() -> ToolManifest {
        serde_yaml::from_str(&ManifestFixture::default_paths().yaml())
            .expect("valid manifest fixture should parse")
    }

    #[test]
    fn accepts_valid_manifest() {
        assert!(validate_manifest(&valid_manifest()).is_ok());
    }

    #[test]
    fn rejects_legacy_integrations_block() {
        let yaml = format!(
            "{}integrations:\n  legacy:\n    enabled: true\n",
            ManifestFixture::default_paths().yaml()
        );

        assert!(serde_yaml::from_str::<ToolManifest>(&yaml).is_err());
    }

    #[test]
    fn rejects_invalid_id() {
        let mut manifest = valid_manifest();
        manifest.identity.id = "Backup Home".to_string();

        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn rejects_duplicate_health_check_ids() {
        let mut manifest = valid_manifest();
        manifest.health.checks[1].id = "target-exists".to_string();

        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn accepts_single_character_id() {
        // Parity with the published JSON schema, which permits a single lowercase
        // letter (`^[a-z]([a-z0-9-]*[a-z0-9])?$`). Previously Rust accepted it while
        // the schema rejected it.
        let mut manifest = valid_manifest();
        manifest.identity.id = "x".to_string();

        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn rejects_install_artifact_path_not_backed_by_a_link() {
        // Root-cause guard: a symlink install's artifact_path must match a declared
        // link source so install paths and links cannot silently diverge.
        let mut manifest = valid_manifest();
        manifest.install.artifact_path = "~/somewhere/else".to_string();

        let error = validate_manifest(&manifest).expect_err("divergent artifact_path should fail");
        assert!(
            error.to_string().contains("artifact_path"),
            "message: {error}"
        );
    }

    #[test]
    fn rejects_install_target_path_not_backed_by_a_link() {
        // The Case G divergence from review: target_path resolves to a path that no
        // desired link manages. This is now a validation error rather than a confusing
        // drift report.
        let mut manifest = valid_manifest();
        manifest.install.target_path = "~/.local/bin/never-linked".to_string();

        let error = validate_manifest(&manifest).expect_err("divergent target_path should fail");
        assert!(
            error.to_string().contains("target_path"),
            "message: {error}"
        );
    }
}
