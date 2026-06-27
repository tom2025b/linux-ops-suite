use chrono::NaiveDate;
use serde::Serialize;

use crate::manifest::LifecycleState;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Lifecycle review summary for one manifest.
pub struct LifecycleReport {
    pub tool_id: String,
    pub state: LifecycleState,
    pub review_after: NaiveDate,
    pub as_of: NaiveDate,
    pub review_status: ReviewStatus,
    pub allowed_next_states: Vec<LifecycleState>,
    pub replacement: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Result of checking whether a lifecycle transition is allowed.
pub struct LifecycleTransitionReport {
    pub tool_id: String,
    pub from: LifecycleState,
    pub to: LifecycleState,
    pub allowed: bool,
    pub allowed_next_states: Vec<LifecycleState>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Review due status derived from `review_after` and the evaluation date.
pub enum ReviewStatus {
    Current,
    Due,
}
