use std::path::PathBuf;

use serde::Serialize;

use crate::manifest::HealthCheckType;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Aggregate outcome of running a manifest's health checks.
pub struct HealthReport {
    pub tool_id: String,
    pub outcomes: Vec<HealthCheckOutcome>,
}

impl HealthReport {
    /// Return true when every declared health check passed.
    pub fn is_healthy(&self) -> bool {
        self.outcomes
            .iter()
            .all(|outcome| outcome.status == HealthStatus::Passed)
    }

    /// Count health checks that passed.
    pub fn passed_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.status == HealthStatus::Passed)
            .count()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Outcome for one declared health check.
pub struct HealthCheckOutcome {
    pub id: String,
    pub check_type: HealthCheckType,
    pub path: String,
    pub resolved_path: PathBuf,
    pub status: HealthStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Pass/fail status for one health check.
pub enum HealthStatus {
    Passed,
    Failed,
}
