use std::{fs, path::Path};

use serde::Serialize;

use crate::{
    error::{Result, ToolFoundryError},
    install::{
        check_install_drift, plan_install,
        report::{DriftReport, InstallAction, InstallPlanStatus, LinkDrift, LinkStatus},
    },
    manifest::ToolManifest,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Result of applying a safe, non-blocked install plan.
pub struct InstallApplyReport {
    pub tool_id: String,
    /// Number of planned actions for the applied (non-blocked) plan. The `actions`
    /// list is the plan that was executed; this counts those planned actions, not a
    /// separate tally of filesystem syscalls.
    pub planned_count: usize,
    pub actions: Vec<InstallAction>,
    pub final_drift: DriftReport,
}

/// Apply a ready install plan and return the resulting filesystem drift state.
pub fn apply_install(manifest: &ToolManifest) -> Result<InstallApplyReport> {
    let plan = plan_install(manifest)?;
    if plan.status == InstallPlanStatus::Blocked {
        return Err(ToolFoundryError::InstallBlocked {
            tool_id: plan.tool_id,
        });
    }

    let before = check_install_drift(manifest)?;
    for link in &before.links {
        apply_link(link)?;
    }

    let final_drift = check_install_drift(manifest)?;

    Ok(InstallApplyReport {
        tool_id: plan.tool_id,
        planned_count: plan.actions.len(),
        actions: plan.actions,
        final_drift,
    })
}

fn apply_link(link: &LinkDrift) -> Result<()> {
    match link.status {
        LinkStatus::Current => Ok(()),
        LinkStatus::TargetMissing => create_symlink_with_parent(link),
        LinkStatus::TargetMismatch => replace_symlink(link),
        // SourceMissing and TargetNotSymlink are ManualIntervention cases that the
        // planner marks Blocked, so apply_install rejects them before reaching this
        // loop. Fail closed if that contract is ever broken rather than silently
        // skipping a link the caller believes was handled.
        LinkStatus::SourceMissing | LinkStatus::TargetNotSymlink => {
            Err(ToolFoundryError::InstallApplyInvariant {
                link_status: format!("{:?}", link.status),
            })
        }
    }
}

fn create_symlink_with_parent(link: &LinkDrift) -> Result<()> {
    if let Some(parent) = link.resolved_target.parent() {
        fs::create_dir_all(parent).map_err(|source| ToolFoundryError::InstallWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    create_symlink(&link.resolved_source, &link.resolved_target)
}

fn replace_symlink(link: &LinkDrift) -> Result<()> {
    // Repoint atomically: stage the new symlink under a temporary name in the same
    // directory, then rename it over the target. `rename(2)` is atomic on the same
    // filesystem, so a crash leaves either the old or the new link in place, never a
    // missing target. This upholds the "guarded install" contract that apply must not
    // leave a managed link half-removed.
    let target = &link.resolved_target;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = parent.join(format!(".{file_name}.toolfoundry-tmp"));

    // Clear any leftover staging entry from a prior interrupted apply so create_symlink
    // does not fail with AlreadyExists.
    if staging.symlink_metadata().is_ok() {
        fs::remove_file(&staging).map_err(|source| ToolFoundryError::InstallWrite {
            path: staging.clone(),
            source,
        })?;
    }

    create_symlink(&link.resolved_source, &staging)?;

    fs::rename(&staging, target).map_err(|source| {
        // Best-effort cleanup so a failed rename does not strand the staging link.
        let _ = fs::remove_file(&staging);
        ToolFoundryError::InstallWrite {
            path: target.clone(),
            source,
        }
    })
}

#[cfg(unix)]
fn create_symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).map_err(|source| ToolFoundryError::InstallWrite {
        path: target.to_path_buf(),
        source,
    })
}

#[cfg(windows)]
fn create_symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::windows::fs::symlink_file(source, target).map_err(|source| {
        ToolFoundryError::InstallWrite {
            path: target.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        error::ToolFoundryError,
        install::{apply_install, check_install_drift},
        manifest::{ToolManifest, load_manifest},
        test_support::ManifestFixture,
    };

    #[test]
    fn creates_missing_parent_and_symlink() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");

        let manifest = workspace.load_manifest(&source, &target);
        let report = apply_install(&manifest).expect("install should apply");
        let drift = check_install_drift(&manifest).expect("drift should load");

        assert_eq!(report.planned_count, 2);
        assert!(drift.is_current());
    }

    #[test]
    fn leaves_current_install_as_noop() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);

        let manifest = workspace.load_manifest(&source, &target);
        let report = apply_install(&manifest).expect("install should apply");

        assert_eq!(report.planned_count, 0);
        assert!(report.final_drift.is_current());
    }

    #[test]
    fn repoints_mismatched_symlink_atomically() {
        // End-to-end TargetMismatch path: a managed link points at the wrong source.
        // apply must repoint it to the declared source and leave drift current, with
        // no leftover staging entry from the atomic rename.
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let wrong = workspace.path.join("wrong-source");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        File::create(&wrong).expect("wrong source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&wrong, &target);

        let manifest = workspace.load_manifest(&source, &target);
        let report = apply_install(&manifest).expect("install should apply");
        let drift = check_install_drift(&manifest).expect("drift should load");

        assert!(drift.is_current());
        assert_eq!(
            fs::read_link(&target).expect("target should be a symlink"),
            source
        );
        // The atomic staging entry must not survive a successful apply.
        let staging = target
            .parent()
            .expect("target parent")
            .join(".backup-home.toolfoundry-tmp");
        assert!(
            staging.symlink_metadata().is_err(),
            "staging symlink should be renamed away, not left behind"
        );
        assert_eq!(report.planned_count, 1);
    }

    #[test]
    fn refuses_blocked_install_plan() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        File::create(&target).expect("target fixture should be created");

        let manifest = workspace.load_manifest(&source, &target);
        let error = apply_install(&manifest).expect_err("blocked install should fail");

        assert!(matches!(error, ToolFoundryError::InstallBlocked { .. }));
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
            let path = std::env::temp_dir().join(format!("toolfoundry-apply-{nonce}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }

        fn load_manifest(&self, source: &Path, target: &Path) -> ToolManifest {
            let manifest_path = self.write_manifest(source, target);
            load_manifest(&manifest_path).expect("fixture manifest should load")
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
