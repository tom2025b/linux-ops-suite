use toolfoundry_core::{
    health::HealthStatus,
    install::{DriftStatus, InstallActionKind, InstallPlanStatus, LinkStatus},
    lifecycle::ReviewStatus,
};

pub(super) fn exists_label(exists: bool, path: &str) -> String {
    if exists {
        format!("exists at {path}")
    } else {
        format!("missing at {path}")
    }
}

pub(super) fn link_status_label(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::Current => "current",
        LinkStatus::SourceMissing => "source-missing",
        LinkStatus::TargetMissing => "target-missing",
        LinkStatus::TargetNotSymlink => "target-not-symlink",
        LinkStatus::TargetMismatch => "target-mismatch",
    }
}

pub(super) fn drift_status_label(status: DriftStatus) -> &'static str {
    match status {
        DriftStatus::Current => "current",
        DriftStatus::ArtifactMissing => "artifact-missing",
        DriftStatus::TargetMissing => "target-missing",
        DriftStatus::LinksDrifted => "links-drifted",
    }
}

pub(super) fn review_status_label(status: ReviewStatus) -> &'static str {
    match status {
        ReviewStatus::Current => "current",
        ReviewStatus::Due => "due",
    }
}

pub(super) fn install_plan_status_label(status: InstallPlanStatus) -> &'static str {
    match status {
        InstallPlanStatus::Noop => "noop",
        InstallPlanStatus::Ready => "ready",
        InstallPlanStatus::Blocked => "blocked",
    }
}

pub(super) fn install_action_kind_label(kind: InstallActionKind) -> &'static str {
    match kind {
        InstallActionKind::CreateParentDirectory => "create-parent-directory",
        InstallActionKind::CreateSymlink => "create-symlink",
        InstallActionKind::ReplaceSymlink => "replace-symlink",
        InstallActionKind::ManualIntervention => "manual-intervention",
    }
}

pub(super) fn health_status_label(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Passed => "pass",
        HealthStatus::Failed => "fail",
    }
}
