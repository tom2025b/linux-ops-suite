use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Summary of install drift for one manifest.
pub struct DriftReport {
    pub tool_id: String,
    pub status: DriftStatus,
    pub artifact_path: String,
    pub resolved_artifact_path: PathBuf,
    pub artifact_exists: bool,
    pub target_path: String,
    pub resolved_target_path: PathBuf,
    pub target_exists: bool,
    pub links: Vec<LinkDrift>,
}

impl DriftReport {
    /// Return true when the install artifact, target, and desired links are current.
    pub fn is_current(&self) -> bool {
        self.status == DriftStatus::Current
    }

    /// Count desired links that already match the manifest.
    pub fn current_link_count(&self) -> usize {
        self.links
            .iter()
            .filter(|link| link.status == LinkStatus::Current)
            .count()
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Top-level drift classification for an install report.
pub enum DriftStatus {
    Current,
    ArtifactMissing,
    TargetMissing,
    LinksDrifted,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Drift details for one desired symlink.
pub struct LinkDrift {
    pub source: String,
    pub target: String,
    pub resolved_source: PathBuf,
    pub resolved_target: PathBuf,
    pub status: LinkStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Filesystem state for one desired symlink.
pub enum LinkStatus {
    Current,
    SourceMissing,
    TargetMissing,
    TargetNotSymlink,
    TargetMismatch,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Dry-run action plan for bringing an install into the desired state.
pub struct InstallPlan {
    pub tool_id: String,
    pub status: InstallPlanStatus,
    pub dry_run: bool,
    pub action_count: usize,
    pub actions: Vec<InstallAction>,
}

impl InstallPlan {
    /// Create an install plan and derive its status from its actions.
    pub fn new(tool_id: String, actions: Vec<InstallAction>) -> Self {
        let status = if actions.is_empty() {
            InstallPlanStatus::Noop
        } else if actions
            .iter()
            .any(InstallAction::requires_manual_intervention)
        {
            InstallPlanStatus::Blocked
        } else {
            InstallPlanStatus::Ready
        };

        Self {
            tool_id,
            status,
            dry_run: true,
            action_count: actions.len(),
            actions,
        }
    }

    /// Return true when the plan can be applied automatically.
    pub fn is_ready(&self) -> bool {
        matches!(
            self.status,
            InstallPlanStatus::Noop | InstallPlanStatus::Ready
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Execution readiness of an install plan.
pub enum InstallPlanStatus {
    Noop,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// One filesystem or manual action in an install plan.
pub struct InstallAction {
    pub kind: InstallActionKind,
    pub source: Option<String>,
    pub target: String,
    pub message: String,
}

impl InstallAction {
    /// Return true when this action blocks automatic installation.
    pub fn requires_manual_intervention(&self) -> bool {
        self.kind == InstallActionKind::ManualIntervention
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Supported install action kinds.
pub enum InstallActionKind {
    CreateParentDirectory,
    CreateSymlink,
    ReplaceSymlink,
    ManualIntervention,
}
