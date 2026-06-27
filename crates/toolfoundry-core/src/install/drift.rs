use std::{fs, path::Path};

use crate::{
    error::Result,
    install::report::{DriftReport, DriftStatus, LinkDrift, LinkStatus},
    manifest::{DesiredLink, ToolManifest},
    paths::expand_manifest_path,
};

/// Compare a manifest's install intent against the current filesystem state.
pub fn check_install_drift(manifest: &ToolManifest) -> Result<DriftReport> {
    let resolved_artifact_path = expand_manifest_path(&manifest.install.artifact_path)?;
    let resolved_target_path = expand_manifest_path(&manifest.install.target_path)?;

    let mut links = Vec::with_capacity(manifest.links.desired.len());
    for desired in &manifest.links.desired {
        links.push(check_link_drift(desired)?);
    }

    let artifact_exists = resolved_artifact_path.exists();
    let target_exists = resolved_target_path.exists();
    let links_current = links.iter().all(|link| link.status == LinkStatus::Current);
    // Link correctness is what determines drift. The install.target_path field only
    // raises its own TargetMissing status, and only when the desired links are
    // otherwise current, so it can never masquerade as link drift (P2): a fully
    // current link set is never reported as "links_drifted".
    let status = if !artifact_exists {
        DriftStatus::ArtifactMissing
    } else if !links_current {
        DriftStatus::LinksDrifted
    } else if !target_exists {
        DriftStatus::TargetMissing
    } else {
        DriftStatus::Current
    };

    Ok(DriftReport {
        tool_id: manifest.identity.id.clone(),
        status,
        artifact_path: manifest.install.artifact_path.clone(),
        resolved_artifact_path,
        artifact_exists,
        target_path: manifest.install.target_path.clone(),
        resolved_target_path,
        target_exists,
        links,
    })
}

fn check_link_drift(desired: &DesiredLink) -> Result<LinkDrift> {
    let resolved_source = expand_manifest_path(&desired.source)?;
    let resolved_target = expand_manifest_path(&desired.target)?;

    let (status, message) = classify_link(&resolved_source, &resolved_target);

    Ok(LinkDrift {
        source: desired.source.clone(),
        target: desired.target.clone(),
        resolved_source,
        resolved_target,
        status,
        message,
    })
}

fn classify_link(source: &Path, target: &Path) -> (LinkStatus, String) {
    if !source.exists() {
        return (
            LinkStatus::SourceMissing,
            format!("source does not exist at {}", source.display()),
        );
    }

    if !target.exists() && fs::symlink_metadata(target).is_err() {
        return (
            LinkStatus::TargetMissing,
            format!("target does not exist at {}", target.display()),
        );
    }

    match fs::read_link(target) {
        Ok(actual) if paths_equivalent(&actual, source) => (
            LinkStatus::Current,
            format!("target points to source at {}", target.display()),
        ),
        Ok(actual) => (
            LinkStatus::TargetMismatch,
            format!(
                "target points to {}, expected {}",
                actual.display(),
                source.display()
            ),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => (
            LinkStatus::TargetNotSymlink,
            format!("target exists but is not a symlink at {}", target.display()),
        ),
        Err(error) => (
            LinkStatus::TargetMismatch,
            format!("cannot inspect symlink at {}: {error}", target.display()),
        ),
    }
}

fn paths_equivalent(actual: &Path, expected: &Path) -> bool {
    actual == expected || actual.canonicalize().ok() == expected.canonicalize().ok()
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{manifest::load_manifest, test_support::ManifestFixture};

    use super::*;

    #[test]
    fn reports_current_symlink_state() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);

        let manifest_path = workspace.write_manifest(&source, &target);
        let manifest = load_manifest(&manifest_path).expect("manifest should load");
        let report = check_install_drift(&manifest).expect("drift check should run");

        assert!(report.is_current());
        assert_eq!(report.current_link_count(), 1);
    }

    #[test]
    fn reports_target_mismatch() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let other = workspace.path.join("other");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        File::create(&other).expect("other fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&other, &target);

        let manifest_path = workspace.write_manifest(&source, &target);
        let manifest = load_manifest(&manifest_path).expect("manifest should load");
        let report = check_install_drift(&manifest).expect("drift check should run");

        assert_eq!(report.status, DriftStatus::LinksDrifted);
        assert_eq!(report.links[0].status, LinkStatus::TargetMismatch);
    }

    #[test]
    fn current_link_with_dangling_install_target_reports_target_missing_not_drift() {
        // Regression (P2): a fully current desired link must never be reported as
        // LinksDrifted just because install.target_path does not resolve on disk.
        // The artifact and the desired link are current, but install.target_path is
        // itself a dangling symlink (exists() == false); status must be TargetMissing,
        // never LinksDrifted.
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);
        // install.target_path is a dangling symlink: present as a link entry, absent
        // as a resolved file. The desired link (source -> target) stays current.
        let install_target = workspace.path.join("bin").join("install-target");
        create_symlink(&workspace.path.join("nonexistent"), &install_target);

        // Parse without validating: the divergent install.target_path is exactly what
        // validation now forbids (see manifest validation tests), but check_install_drift
        // must still classify it correctly and defensively.
        let manifest: ToolManifest = serde_yaml::from_str(
            &ManifestFixture::with_paths(
                workspace.path.display(),
                source.display(),
                install_target.display(),
            )
            .with_link(source.display(), target.display())
            .yaml(),
        )
        .expect("fixture manifest should parse");
        let report = check_install_drift(&manifest).expect("drift check should run");

        assert_eq!(report.status, DriftStatus::TargetMissing);
        assert_ne!(report.status, DriftStatus::LinksDrifted);
        assert!(!report.target_exists);
        assert_eq!(report.links[0].status, LinkStatus::Current);
        assert_eq!(report.current_link_count(), 1);
    }

    #[test]
    fn reports_missing_artifact_separately_from_link_drift() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("missing-backup-home");
        let target = workspace.path.join("bin").join("backup-home");

        let manifest_path = workspace.write_manifest(&source, &target);
        let manifest = load_manifest(&manifest_path).expect("manifest should load");
        let report = check_install_drift(&manifest).expect("drift check should run");

        assert_eq!(report.status, DriftStatus::ArtifactMissing);
        assert!(!report.artifact_exists);
        assert_eq!(report.links[0].status, LinkStatus::SourceMissing);
    }

    #[cfg(unix)]
    fn create_symlink(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).expect("symlink should be created");
    }

    #[cfg(windows)]
    fn create_symlink(source: &Path, target: &Path) {
        std::os::windows::fs::symlink_file(source, target).expect("symlink should be created");
    }

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("toolfoundry-drift-{nonce}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }

        fn write_manifest(&self, source: &Path, target: &Path) -> PathBuf {
            let manifest_path = self.path.join("tool.yaml");
            let manifest = ManifestFixture::with_paths(
                self.path.display(),
                source.display(),
                target.display(),
            )
            .yaml();

            fs::write(&manifest_path, manifest).expect("fixture manifest should be written");
            manifest_path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
