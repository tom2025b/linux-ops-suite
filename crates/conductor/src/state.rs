//! The normalized snapshot of everything conductor read this run. It holds
//! *facts only* — no I/O (that's `sources.rs`), no rules (that's `plan::rules`),
//! no rendering (that's `report.rs`). It is the single input to the rule engine,
//! which makes the rules a pure function of this struct and trivially testable.

/// One source's freshness, collapsed to the three buckets the rules care about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Current,
    Stale,
    /// Present but unusable (unsupported version) or absent entirely.
    Unavailable,
}

/// Severity of a finding, in escalation order (so `Ord` sorts worst last).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// One thing worth the operator's attention, normalized across producers.
#[derive(Clone, Debug)]
pub struct Finding {
    pub what: String,
    pub why: String,
    pub source: String,
    pub severity: Severity,
}

/// A named feed and its freshness (drives rule 1 and the health view).
#[derive(Clone, Debug)]
pub struct FeedStatus {
    pub name: &'static str,
    pub freshness: Freshness,
}

/// A suite binary and whether it is on `$PATH` (drives rule 2 and health).
#[derive(Clone, Copy, Debug)]
pub struct BinaryStatus {
    pub name: &'static str,
    pub present: bool,
}

/// A filesystem path tripwire reports as drifted (feeds rule 5's correlation).
/// `path` is the canonical equality field rule 5 matches a finding's `what`
/// against — exact string equality, no normalization in v1.
#[derive(Clone, Debug)]
pub struct DriftedPath {
    pub path: String,
}

/// A job (Proto session) that ended in failure (drives rule 6).
#[derive(Clone, Debug)]
pub struct FailedJob {
    pub title: String,
}

/// Everything conductor read, normalized. The sole input to `plan::build`.
#[derive(Clone, Debug, Default)]
pub struct SuiteState {
    /// When the suite snapshot was built (RFC3339), for the "checked" stamp.
    pub built_at: Option<String>,
    pub feeds: Vec<FeedStatus>,
    pub findings: Vec<Finding>,
    pub drift: Vec<DriftedPath>,
    pub failed_jobs: Vec<FailedJob>,
    pub binaries: Vec<BinaryStatus>,
}

impl SuiteState {
    /// An all-empty state — the starting point a loader fills in, and the
    /// "nothing was readable" baseline.
    pub fn empty() -> Self {
        Self::default()
    }

    /// True if any feed is stale or unavailable (rule 1's precondition).
    pub fn has_stale_or_unavailable_feed(&self) -> bool {
        self.feeds.iter().any(|f| f.freshness != Freshness::Current)
    }

    /// The suite binaries missing from `$PATH` (rule 2's precondition).
    pub fn missing_binaries(&self) -> Vec<&BinaryStatus> {
        self.binaries.iter().filter(|b| !b.present).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_has_no_attention_and_no_stale_feeds() {
        let s = SuiteState::empty();
        assert!(s.findings.is_empty());
        assert!(!s.has_stale_or_unavailable_feed());
        assert!(s.missing_binaries().is_empty());
    }

    #[test]
    fn stale_or_unavailable_feed_is_detected() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "scripts",
            freshness: Freshness::Current,
        });
        assert!(!s.has_stale_or_unavailable_feed());
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        assert!(s.has_stale_or_unavailable_feed());
    }

    #[test]
    fn severity_orders_worst_last() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        let mut v = vec![Severity::Low, Severity::Critical, Severity::Medium];
        v.sort();
        assert_eq!(v, vec![Severity::Low, Severity::Medium, Severity::Critical]);
    }

    #[test]
    fn missing_binaries_lists_only_absent_ones() {
        let mut s = SuiteState::empty();
        s.binaries.push(BinaryStatus {
            name: "pulse",
            present: true,
        });
        s.binaries.push(BinaryStatus {
            name: "rewind",
            present: false,
        });
        let missing = s.missing_binaries();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "rewind");
    }
}
