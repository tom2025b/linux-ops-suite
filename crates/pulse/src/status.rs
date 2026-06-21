//! `pulse status` — a machine-readable liveness line for parent processes
//! (RexOps' StatusCommand health source). Read-only, non-interactive: it reuses
//! the same snapshot→Verdict pipeline the screens use and serializes the result
//! instead of rendering it. One JSON line, then exit 0 (healthy) / 1 (not).

use std::process::ExitCode;

use serde::Serialize;

use crate::verdict::{State, Verdict};

/// The tiny contract RexOps parses. Stable: add fields only additively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub struct StatusReport {
    /// True only when the verdict is fully healthy.
    pub healthy: bool,
    /// A short human reason (the top cause, or a state summary).
    pub detail: String,
    /// Wall-time Pulse spent reading its snapshot + computing the verdict.
    pub latency_ms: u64,
}

#[allow(dead_code)]
impl StatusReport {
    /// Derive the report from an already-computed verdict. No new health logic:
    /// `healthy` mirrors `State::Healthy`; `detail` is the top cause if any,
    /// else a one-line summary of the state.
    pub fn from_verdict(v: &Verdict, latency_ms: u64) -> Self {
        let healthy = matches!(v.state, State::Healthy);
        let detail = match v.causes.first() {
            Some(c) => format!("{}: {}", c.what, c.why),
            None => match v.state {
                State::Healthy => "all clear".to_owned(),
                State::NeedsAttention => "needs attention".to_owned(),
                State::Incomplete => "snapshot incomplete".to_owned(),
            },
        };
        StatusReport {
            healthy,
            detail,
            latency_ms,
        }
    }

    /// The single JSON line printed on stdout (no trailing newline added here;
    /// the caller uses `println!`).
    pub fn to_json_line(&self) -> String {
        // serde_json never fails for this plain struct.
        serde_json::to_string(self).expect("StatusReport is always serializable")
    }

    /// Process exit code: success when healthy, `1` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        if self.healthy {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::{Cause, State, Verdict};

    fn verdict(state: State, causes: Vec<Cause>) -> Verdict {
        Verdict {
            state,
            age: "1m ago".to_owned(),
            critical: 0,
            high: 0,
            confidence_reduced: false,
            unavailable: 0,
            stale: 0,
            causes,
            sources: Vec::new(),
        }
    }

    #[test]
    fn healthy_verdict_serializes_to_one_json_line_and_exits_zero() {
        let r = StatusReport::from_verdict(&verdict(State::Healthy, Vec::new()), 7);
        assert_eq!(
            r.to_json_line(),
            r#"{"healthy":true,"detail":"all clear","latency_ms":7}"#
        );
        assert!(!r.to_json_line().contains('\n'));
        assert_eq!(r.exit_code(), std::process::ExitCode::SUCCESS);
    }

    #[test]
    fn attention_verdict_uses_top_cause_as_detail_and_exits_one() {
        let causes = vec![Cause {
            what: "bulwark".to_owned(),
            why: "1 critical finding".to_owned(),
            source: "bulwark".to_owned(),
        }];
        let r = StatusReport::from_verdict(&verdict(State::NeedsAttention, causes), 12);
        assert_eq!(r.healthy, false);
        assert_eq!(r.detail, "bulwark: 1 critical finding");
        assert_eq!(r.exit_code(), std::process::ExitCode::from(1));
    }

    #[test]
    fn incomplete_with_no_causes_is_not_healthy_and_has_a_detail() {
        let r = StatusReport::from_verdict(&verdict(State::Incomplete, Vec::new()), 3);
        assert_eq!(r.healthy, false);
        assert_eq!(r.detail, "snapshot incomplete");
        assert_eq!(r.exit_code(), std::process::ExitCode::from(1));
    }
}

// Learning Notes
// - `detail` reuses the verdict's existing `causes`/`state`; Pulse gains no new
//   health model — the contract is a *view* of the verdict, like the screens.
// - serde_json is already a workspace dependency (used by the contract readers);
//   no new crate is introduced.
