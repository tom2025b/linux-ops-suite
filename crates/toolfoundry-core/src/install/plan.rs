use std::path::Path;

use crate::{
    error::Result,
    install::{
        check_install_drift,
        report::{InstallAction, InstallActionKind, InstallPlan, LinkDrift, LinkStatus},
    },
    manifest::ToolManifest,
};

/// Build a dry-run install plan from a manifest and current drift report.
pub fn plan_install(manifest: &ToolManifest) -> Result<InstallPlan> {
    let drift = check_install_drift(manifest)?;
    let mut actions = Vec::new();

    if !drift.artifact_exists {
        actions.push(InstallAction {
            kind: InstallActionKind::ManualIntervention,
            source: Some(drift.artifact_path.clone()),
            target: drift.target_path.clone(),
            message: format!(
                "install artifact is missing at {}; build or restore it before installing",
                drift.artifact_path
            ),
        });
    }

    if manifest.install.requires_sudo && !drift.is_current() {
        actions.push(InstallAction {
            kind: InstallActionKind::ManualIntervention,
            source: Some(drift.artifact_path.clone()),
            target: drift.target_path.clone(),
            message:
                "install requires sudo; run the installation manually with elevated privileges"
                    .to_string(),
        });

        return Ok(InstallPlan::new(drift.tool_id, actions));
    }

    for link in &drift.links {
        actions.extend(plan_link(link));
    }

    Ok(InstallPlan::new(drift.tool_id, actions))
}

fn plan_link(link: &LinkDrift) -> Vec<InstallAction> {
    match link.status {
        LinkStatus::Current => Vec::new(),
        LinkStatus::SourceMissing => vec![manual_action(
            link,
            "source is missing; cannot create desired symlink",
        )],
        LinkStatus::TargetMissing => plan_missing_target(link),
        LinkStatus::TargetMismatch => vec![InstallAction {
            kind: InstallActionKind::ReplaceSymlink,
            source: Some(link.source.clone()),
            target: link.target.clone(),
            message: format!(
                "replace symlink at {} so it points to {}",
                link.target, link.source
            ),
        }],
        LinkStatus::TargetNotSymlink => vec![manual_action(
            link,
            "target exists but is not a symlink; manual review required before replacement",
        )],
    }
}

fn plan_missing_target(link: &LinkDrift) -> Vec<InstallAction> {
    let mut actions = Vec::new();

    if let Some(parent) = Path::new(&link.resolved_target).parent()
        && !parent.exists()
    {
        actions.push(InstallAction {
            kind: InstallActionKind::CreateParentDirectory,
            source: None,
            target: parent.display().to_string(),
            message: format!("create parent directory {}", parent.display()),
        });
    }

    actions.push(InstallAction {
        kind: InstallActionKind::CreateSymlink,
        source: Some(link.source.clone()),
        target: link.target.clone(),
        message: format!("create symlink {} -> {}", link.target, link.source),
    });

    actions
}

fn manual_action(link: &LinkDrift, message: &str) -> InstallAction {
    InstallAction {
        kind: InstallActionKind::ManualIntervention,
        source: Some(link.source.clone()),
        target: link.target.clone(),
        message: message.to_string(),
    }
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
    use crate::install::report::{InstallActionKind, InstallPlanStatus};

    #[test]
    fn current_install_has_no_actions() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);

        let manifest = workspace.load_manifest(&source, &target);
        let plan = plan_install(&manifest).expect("install plan should be created");

        assert_eq!(plan.status, InstallPlanStatus::Noop);
        assert!(plan.actions.is_empty());
        assert!(plan.dry_run);
    }

    #[test]
    fn missing_target_plans_directory_and_symlink_creation() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("missing-bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");

        let manifest = workspace.load_manifest(&source, &target);
        let plan = plan_install(&manifest).expect("install plan should be created");

        assert_eq!(plan.status, InstallPlanStatus::Ready);
        assert_eq!(
            plan.actions[0].kind,
            InstallActionKind::CreateParentDirectory
        );
        assert_eq!(plan.actions[1].kind, InstallActionKind::CreateSymlink);
    }

    #[test]
    fn regular_file_target_requires_manual_intervention() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        File::create(&target).expect("target fixture should be created");

        let manifest = workspace.load_manifest(&source, &target);
        let plan = plan_install(&manifest).expect("install plan should be created");

        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert_eq!(plan.actions[0].kind, InstallActionKind::ManualIntervention);
    }

    #[test]
    fn sudo_required_install_is_blocked_for_manual_application() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");

        let manifest = workspace.load_manifest_with_sudo(&source, &target, true);
        let plan = plan_install(&manifest).expect("install plan should be created");

        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].kind, InstallActionKind::ManualIntervention);
        assert!(plan.actions[0].message.contains("requires sudo"));
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
            let path = std::env::temp_dir().join(format!("toolfoundry-plan-{nonce}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }

        fn load_manifest(&self, source: &Path, target: &Path) -> ToolManifest {
            self.load_manifest_with_sudo(source, target, false)
        }

        fn load_manifest_with_sudo(
            &self,
            source: &Path,
            target: &Path,
            requires_sudo: bool,
        ) -> ToolManifest {
            let manifest_path = self.write_manifest(source, target, requires_sudo);
            load_manifest(&manifest_path).expect("fixture manifest should load")
        }

        fn write_manifest(&self, source: &Path, target: &Path, requires_sudo: bool) -> PathBuf {
            let manifest_path = self.path.join("tool.yaml");
            let manifest = ManifestFixture::with_paths(
                self.path.display(),
                source.display(),
                target.display(),
            )
            .with_requires_sudo(requires_sudo)
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
