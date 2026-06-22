//! The v1 rules: pure helpers turning a `SuiteState` into ordered `Step`s, in
//! priority order. This is the product's brain — deterministic (same state ⇒
//! same plan) and the densest test module in the crate. Each rule is a small
//! function with its precondition; `super::build` runs them in order and
//! assembles the `Plan`. See CONDUCTOR_DESIGN.md "How Conductor Builds the Plan".

use super::{quote_arg, slug, Ring, Step};
use crate::state::{Severity, SuiteState};

/// Rule 1 — trust the data first. A stale/unavailable feed means every later
/// step reads possibly-wrong data, so refresh first. One step regardless of how
/// many feeds are affected: `workstate snapshot` re-probes ALL producers, so it
/// both refreshes a stale feed AND re-checks an unavailable one (clearing it if
/// its producer is now yielding data). The label is "refresh suite snapshot"
/// rather than "refresh stale data" because the trigger may be unavailability,
/// not staleness — the situation lines say precisely which, and only promise a
/// fix for the stale ones.
pub(super) fn refresh_stale_feeds(state: &SuiteState) -> Option<Step> {
    if state.has_stale_or_unavailable_feed() {
        Some(Step::new(
            "refresh-stale-data",
            "refresh suite snapshot",
            Some("workstate snapshot".to_string()),
            Ring::ChangesState,
        ))
    } else {
        None
    }
}

/// Rule 2 — wiring gaps. Each suite binary missing from `$PATH` becomes an Info
/// step naming the one command that fixes it. Informational because conductor
/// can't install for you.
pub(super) fn wiring_gaps(state: &SuiteState) -> Vec<Step> {
    state
        .missing_binaries()
        .into_iter()
        .map(|b| {
            Step::new(
                format!("install-{}", slug(b.name)),
                format!("{} is not on PATH", b.name),
                Some(format!("install.sh --only {}", b.name)),
                Ring::Info,
            )
        })
        .collect()
}

/// Rule 4 + 5 — investigate findings, worst first, with the drift-correlated one
/// pulled to the front and annotated. Returns the ordered investigate steps.
pub(super) fn investigate_findings(state: &SuiteState) -> Vec<Step> {
    // findings arrive already sorted worst-first (sources::read_findings).
    let drifted: Vec<&str> = state.drift.iter().map(|d| d.path.as_str()).collect();

    let mut correlated: Vec<Step> = Vec::new();
    let mut rest: Vec<Step> = Vec::new();
    for f in &state.findings {
        if f.severity < Severity::High {
            continue;
        }
        let mut step = Step::new(
            format!("investigate-{}", slug(&f.what)),
            format!("investigate {}", f.what),
            Some(format!("bulwark show {}", quote_arg(&f.what))),
            Ring::ReadOnly,
        );
        if drifted.iter().any(|p| *p == f.what) {
            step = step.annotated("same file as tripwire drift — start here");
            correlated.push(step);
        } else {
            rest.push(step);
        }
    }
    correlated.extend(rest);
    correlated
}

/// Rule 6 — review failed jobs (read-only).
pub(super) fn review_failed_jobs(state: &SuiteState) -> Vec<Step> {
    state
        .failed_jobs
        .iter()
        .map(|j| {
            Step::new(
                format!("review-{}", slug(&j.title)),
                format!("review failed job: {}", j.title),
                Some(format!("proto show {}", quote_arg(&j.title))),
                Ring::ReadOnly,
            )
        })
        .collect()
}

/// Rule 3 — capture before you change. Prepended whenever the plan guides real
/// work — i.e. there is ≥1 finding to investigate or failed job to review — so a
/// pure refresh-only (or wiring-only) plan doesn't force a capture. The capture
/// is itself the Ring-2 step the operator confirms; it is gated on the presence
/// of work, not on another Ring-2 step already being in the plan (the
/// investigate/review steps it guards are read-only). Returns the step.
pub(super) fn safety_capture() -> Step {
    Step::new(
        "safety-capture",
        "capture a safety point",
        Some("rewind capture --label pre-conductor".to_string()),
        Ring::ChangesState,
    )
}

/// The human "situation" lines explaining why there's a plan.
pub(super) fn situation(state: &SuiteState) -> Vec<String> {
    let mut lines = Vec::new();
    // Stale and unavailable feeds need DIFFERENT remedies, so they get different
    // lines. Stale (readable but old) is fixed by a refresh; unavailable (absent
    // or unusable) is NOT — re-running `workstate snapshot` won't conjure data a
    // producer isn't yielding, so saying "refresh" there would be the exact false
    // advice that sends an operator in circles. Name the affected feeds so it's
    // actionable.
    let stale = state.stale_feeds();
    if !stale.is_empty() {
        lines.push(format!(
            "workstate snapshot is stale ({}) — refresh before trusting feeds",
            stale.join(", ")
        ));
    }
    let unavailable = state.unavailable_feeds();
    if !unavailable.is_empty() {
        lines.push(format!(
            "workstate feed unavailable ({}) — its producer isn't yielding usable data; a refresh won't fix it",
            unavailable.join(", ")
        ));
    }
    let drifted: Vec<&str> = state.drift.iter().map(|d| d.path.as_str()).collect();
    let correlated = state
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::High && drifted.iter().any(|p| *p == f.what))
        .count();
    if correlated > 0 {
        let noun = if correlated == 1 {
            "finding correlates"
        } else {
            "findings correlate"
        };
        lines.push(format!("{correlated} {noun} with a tripwire drift"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        BinaryStatus, DriftedPath, FailedJob, FeedStatus, Finding, Freshness, Severity, SuiteState,
    };

    fn finding(what: &str, sev: Severity) -> Finding {
        Finding {
            what: what.into(),
            why: "because".into(),
            source: "bulwark".into(),
            severity: sev,
        }
    }

    /// sources::read_findings sorts worst-first; emulate that contract in tests
    /// that depend on input order.
    fn sort_findings(s: &mut SuiteState) {
        s.findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    }

    // ── fixture matrix (CONDUCTOR_DESIGN.md / merged plan) ───────────────────

    #[test]
    fn clean_state_yields_nothing_to_conduct() {
        let plan = super::super::build(&SuiteState::empty());
        assert!(plan.is_empty());
        assert!(plan.situation.is_empty());
        assert_eq!(plan.plan_id(), "nothing-to-conduct");
    }

    #[test]
    fn stale_snapshot_only_emits_a_single_refresh_first() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.feeds.push(FeedStatus {
            name: "scripts",
            freshness: Freshness::Unavailable,
        });
        let plan = super::super::build(&s);
        assert_eq!(plan.steps[0].id, "refresh-stale-data");
        assert_eq!(plan.steps[0].ring, Ring::ChangesState);
        assert_eq!(plan.steps[0].command.as_deref(), Some("workstate snapshot"));
        assert_eq!(
            plan.steps
                .iter()
                .filter(|s| s.id == "refresh-stale-data")
                .count(),
            1,
            "one refresh, not one per stale feed"
        );
    }

    #[test]
    fn refresh_only_plan_does_not_force_a_safety_capture() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        let plan = super::super::build(&s);
        assert!(!plan.steps.iter().any(|s| s.id == "safety-capture"));
    }

    #[test]
    fn stale_and_unavailable_feeds_get_distinct_honest_situation_lines() {
        // The crux of the wording fix: a STALE feed is told to refresh; an
        // UNAVAILABLE feed is told a refresh WON'T fix it. Conflating them is what
        // made conductor say "refresh" for a feed re-running could never clear.
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "scripts",
            freshness: Freshness::Stale,
        });
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Unavailable,
        });
        let lines = situation(&s);

        // One line names the stale feed and offers the refresh remedy.
        let stale_line = lines
            .iter()
            .find(|l| l.contains("stale"))
            .expect("a stale feed must produce a stale line");
        assert!(stale_line.contains("scripts"), "names the stale feed");
        assert!(stale_line.contains("refresh"), "stale → refresh remedy");

        // A SEPARATE line names the unavailable feed and explicitly says a refresh
        // won't help — the false-advice fix.
        let unavail_line = lines
            .iter()
            .find(|l| l.contains("unavailable"))
            .expect("an unavailable feed must produce its own line");
        assert!(unavail_line.contains("tools"), "names the unavailable feed");
        assert!(
            unavail_line.contains("won't fix it"),
            "unavailable → refresh explicitly does NOT fix it"
        );
    }

    #[test]
    fn a_purely_stale_feed_does_not_emit_an_unavailable_line() {
        // No unavailable feed ⇒ no "unavailable" line at all (and vice-versa),
        // so the operator never sees a remedy that doesn't apply.
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "findings",
            freshness: Freshness::Stale,
        });
        let lines = situation(&s);
        assert!(lines.iter().any(|l| l.contains("stale")));
        assert!(
            !lines.iter().any(|l| l.contains("unavailable")),
            "no unavailable feed ⇒ no unavailable line"
        );
    }

    #[test]
    fn critical_finding_with_no_drift_investigates_read_only_after_capture() {
        let mut s = SuiteState::empty();
        s.findings
            .push(finding("deploy-prod.sh", Severity::Critical));
        let plan = super::super::build(&s);
        // capture precedes investigation; investigation is read-only
        let cap = plan
            .steps
            .iter()
            .position(|s| s.id == "safety-capture")
            .unwrap();
        let inv = plan
            .steps
            .iter()
            .position(|s| s.id == "investigate-deploy-prod-sh")
            .unwrap();
        assert!(cap < inv);
        assert_eq!(plan.steps[inv].ring, Ring::ReadOnly);
        assert!(
            plan.steps[inv].annotation.is_none(),
            "no drift ⇒ no correlation note"
        );
    }

    #[test]
    fn findings_get_a_safety_capture_after_refresh_and_before_investigation() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.findings
            .push(finding("deploy-prod.sh", Severity::Critical));
        let plan = super::super::build(&s);
        let refresh = plan
            .steps
            .iter()
            .position(|s| s.id == "refresh-stale-data")
            .unwrap();
        let capture = plan
            .steps
            .iter()
            .position(|s| s.id == "safety-capture")
            .unwrap();
        let investigate = plan
            .steps
            .iter()
            .position(|s| s.id.starts_with("investigate"))
            .unwrap();
        assert!(refresh < capture, "capture must follow refresh");
        assert!(capture < investigate, "capture must precede investigation");
    }

    #[test]
    fn findings_are_worst_first_and_read_only() {
        let mut s = SuiteState::empty();
        s.findings.push(finding("hi.sh", Severity::High));
        s.findings.push(finding("crit.sh", Severity::Critical));
        sort_findings(&mut s);
        let plan = super::super::build(&s);
        let investigate: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.id.starts_with("investigate"))
            .collect();
        assert_eq!(investigate[0].id, "investigate-crit-sh");
        assert!(investigate.iter().all(|s| s.ring == Ring::ReadOnly));
    }

    #[test]
    fn drift_x_finding_same_file_lifts_and_annotates_the_match() {
        // The signature correlation: deploy-prod.sh is High but drift-correlated,
        // so it must jump ahead of the Critical finding and carry the note.
        let mut s = SuiteState::empty();
        s.findings.push(finding("crit.sh", Severity::Critical));
        s.findings.push(finding("deploy-prod.sh", Severity::High));
        sort_findings(&mut s);
        s.drift.push(DriftedPath {
            path: "deploy-prod.sh".into(),
        });
        let plan = super::super::build(&s);
        let investigate: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.id.starts_with("investigate"))
            .collect();
        assert_eq!(investigate[0].id, "investigate-deploy-prod-sh");
        assert_eq!(
            investigate[0].annotation.as_deref(),
            Some("same file as tripwire drift — start here")
        );
        assert!(plan.situation.iter().any(|l| l.contains("correlate")));
    }

    #[test]
    fn real_work_present_forces_a_safety_capture() {
        // A finding means real investigative work follows, so the safety capture
        // is prepended. The capture itself is the Ring-2 step; what *triggers* it
        // is the presence of work (a finding or a failed job), not the ring of
        // the investigate steps (which are read-only).
        let mut s = SuiteState::empty();
        s.findings.push(finding("x.sh", Severity::High));
        let plan = super::super::build(&s);
        let cap = plan
            .steps
            .iter()
            .find(|s| s.id == "safety-capture")
            .unwrap();
        assert_eq!(cap.ring, Ring::ChangesState);
        assert_eq!(
            cap.command.as_deref(),
            Some("rewind capture --label pre-conductor")
        );
    }

    #[test]
    fn missing_binary_emits_an_info_fix_step() {
        let mut s = SuiteState::empty();
        s.binaries.push(BinaryStatus {
            name: "rewind",
            present: false,
        });
        let plan = super::super::build(&s);
        let fix = plan
            .steps
            .iter()
            .find(|s| s.id == "install-rewind")
            .unwrap();
        assert_eq!(fix.ring, Ring::Info);
        assert_eq!(fix.command.as_deref(), Some("install.sh --only rewind"));
    }

    #[test]
    fn failed_job_emits_a_read_only_review_step() {
        let mut s = SuiteState::empty();
        s.failed_jobs.push(FailedJob {
            title: "nightly-backup".into(),
        });
        let plan = super::super::build(&s);
        let review = plan
            .steps
            .iter()
            .find(|s| s.id == "review-nightly-backup")
            .unwrap();
        assert_eq!(review.ring, Ring::ReadOnly);
        assert_eq!(review.command.as_deref(), Some("proto show nightly-backup"));
    }

    #[test]
    fn mixed_priority_and_correlation_orders_all_groups_correctly() {
        // refresh → wiring → capture → findings(correlated first) → jobs
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.binaries.push(BinaryStatus {
            name: "portman",
            present: false,
        });
        s.findings.push(finding("crit.sh", Severity::Critical));
        s.findings.push(finding("deploy-prod.sh", Severity::High));
        sort_findings(&mut s);
        s.drift.push(DriftedPath {
            path: "deploy-prod.sh".into(),
        });
        s.failed_jobs.push(FailedJob {
            title: "backup".into(),
        });
        let plan = super::super::build(&s);
        let ids: Vec<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();
        let pos = |needle: &str| ids.iter().position(|t| *t == needle).unwrap();
        assert!(pos("refresh-stale-data") < pos("install-portman"));
        assert!(pos("install-portman") < pos("safety-capture"));
        assert!(pos("safety-capture") < pos("investigate-deploy-prod-sh"));
        // correlated High lifted ahead of the Critical
        assert!(pos("investigate-deploy-prod-sh") < pos("investigate-crit-sh"));
        assert!(pos("investigate-crit-sh") < pos("review-backup"));
    }
}
