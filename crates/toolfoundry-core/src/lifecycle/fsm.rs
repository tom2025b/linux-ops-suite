use chrono::NaiveDate;

use crate::{
    lifecycle::report::{LifecycleReport, LifecycleTransitionReport, ReviewStatus},
    manifest::{LifecycleState, ToolManifest},
};

/// Evaluate lifecycle review status and allowed next states for a manifest.
pub fn evaluate_lifecycle(manifest: &ToolManifest, as_of: NaiveDate) -> LifecycleReport {
    let state = manifest.lifecycle.state.clone();

    LifecycleReport {
        tool_id: manifest.identity.id.clone(),
        state: state.clone(),
        review_after: manifest.lifecycle.review_after,
        as_of,
        review_status: review_status(manifest.lifecycle.review_after, as_of),
        allowed_next_states: allowed_next_states(&state),
        replacement: manifest.lifecycle.replacement.clone(),
    }
}

/// Return true when a lifecycle transition is allowed by the state machine.
pub fn transition_allowed(from: &LifecycleState, to: &LifecycleState) -> bool {
    allowed_next_states(from).contains(to)
}

/// Evaluate one requested lifecycle transition without mutating the manifest.
pub fn evaluate_transition(
    manifest: &ToolManifest,
    to: LifecycleState,
) -> LifecycleTransitionReport {
    let from = manifest.lifecycle.state.clone();
    let allowed_next_states = allowed_next_states(&from);
    let allowed = allowed_next_states.contains(&to);

    LifecycleTransitionReport {
        tool_id: manifest.identity.id.clone(),
        from,
        to,
        allowed,
        allowed_next_states,
    }
}

/// Return every lifecycle state that may follow the given state.
pub fn allowed_next_states(from: &LifecycleState) -> Vec<LifecycleState> {
    match from {
        LifecycleState::Experimental => vec![
            LifecycleState::Active,
            LifecycleState::Deprecated,
            LifecycleState::Archived,
        ],
        LifecycleState::Active => vec![
            LifecycleState::Stale,
            LifecycleState::Risky,
            LifecycleState::Broken,
            LifecycleState::Deprecated,
            LifecycleState::Archived,
        ],
        LifecycleState::Stale => vec![
            LifecycleState::Active,
            LifecycleState::Risky,
            LifecycleState::Broken,
            LifecycleState::Deprecated,
            LifecycleState::Archived,
        ],
        LifecycleState::Risky => vec![
            LifecycleState::Active,
            LifecycleState::Broken,
            LifecycleState::Deprecated,
            LifecycleState::Archived,
        ],
        LifecycleState::Broken => vec![
            LifecycleState::Active,
            LifecycleState::Deprecated,
            LifecycleState::Archived,
        ],
        LifecycleState::Deprecated => vec![LifecycleState::Archived],
        LifecycleState::Archived => Vec::new(),
    }
}

fn review_status(review_after: NaiveDate, as_of: NaiveDate) -> ReviewStatus {
    if as_of >= review_after {
        ReviewStatus::Due
    } else {
        ReviewStatus::Current
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::{
        lifecycle::{ReviewStatus, evaluate_lifecycle, evaluate_transition, transition_allowed},
        manifest::{LifecycleState, load_manifest},
        test_support::ManifestFixture,
    };

    #[test]
    fn reports_current_review_before_review_date() {
        let workspace = TempManifest::write(&ManifestFixture::default_paths().yaml());
        let manifest = load_manifest(&workspace.path).expect("manifest should load");
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 31).expect("date should be valid");

        let report = evaluate_lifecycle(&manifest, as_of);

        assert_eq!(report.review_status, ReviewStatus::Current);
        assert!(report.allowed_next_states.contains(&LifecycleState::Stale));
    }

    #[test]
    fn reports_due_review_on_review_date() {
        let workspace = TempManifest::write(&ManifestFixture::default_paths().yaml());
        let manifest = load_manifest(&workspace.path).expect("manifest should load");
        let as_of = NaiveDate::from_ymd_opt(2026, 9, 1).expect("date should be valid");

        let report = evaluate_lifecycle(&manifest, as_of);

        assert_eq!(report.review_status, ReviewStatus::Due);
    }

    #[test]
    fn archived_state_is_terminal() {
        assert!(!transition_allowed(
            &LifecycleState::Archived,
            &LifecycleState::Active
        ));
    }

    #[test]
    fn reports_allowed_transition() {
        let workspace = TempManifest::write(&ManifestFixture::default_paths().yaml());
        let manifest = load_manifest(&workspace.path).expect("manifest should load");

        let report = evaluate_transition(&manifest, LifecycleState::Stale);

        assert_eq!(report.from, LifecycleState::Active);
        assert_eq!(report.to, LifecycleState::Stale);
        assert!(report.allowed);
    }

    #[test]
    fn transition_table_is_exhaustive_and_irreflexive() {
        use LifecycleState::*;

        // Full expected adjacency for the lifecycle FSM. Pinning the entire table
        // here means any future edit to allowed_next_states is caught, not just the
        // states a spot check happens to touch.
        let expected: &[(LifecycleState, &[LifecycleState])] = &[
            (Experimental, &[Active, Deprecated, Archived]),
            (Active, &[Stale, Risky, Broken, Deprecated, Archived]),
            (Stale, &[Active, Risky, Broken, Deprecated, Archived]),
            (Risky, &[Active, Broken, Deprecated, Archived]),
            (Broken, &[Active, Deprecated, Archived]),
            (Deprecated, &[Archived]),
            (Archived, &[]),
        ];

        let all = [
            Experimental,
            Active,
            Stale,
            Risky,
            Broken,
            Deprecated,
            Archived,
        ];

        for (from, allowed) in expected {
            let actual = super::allowed_next_states(from);
            assert_eq!(&actual, allowed, "transitions from {from:?}");

            // No state may transition to itself, and every allowed target must be a
            // real state (guards against typos once states multiply).
            assert!(
                !actual.contains(from),
                "{from:?} must not transition to itself"
            );
            for next in &actual {
                assert!(
                    all.contains(next),
                    "{next:?} is not a known lifecycle state"
                );
            }
        }
    }

    #[test]
    fn reports_blocked_transition() {
        let workspace = TempManifest::write(&ManifestFixture::default_paths().yaml());
        let manifest = load_manifest(&workspace.path).expect("manifest should load");

        let report = evaluate_transition(&manifest, LifecycleState::Experimental);

        assert_eq!(report.from, LifecycleState::Active);
        assert!(!report.allowed);
    }

    struct TempManifest {
        path: std::path::PathBuf,
    }

    impl TempManifest {
        fn write(contents: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("toolfoundry-lifecycle-{nonce}"));
            std::fs::create_dir_all(&dir).expect("temp manifest dir should be created");
            let path = dir.join("tool.yaml");
            std::fs::write(&path, contents).expect("temp manifest should be written");

            Self { path }
        }
    }

    impl Drop for TempManifest {
        fn drop(&mut self) {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }
}
