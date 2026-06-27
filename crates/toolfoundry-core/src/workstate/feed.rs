use chrono::{DateTime, NaiveDate, Utc};

use crate::{
    error::{Result, ToolFoundryError},
    health::run_health_checks,
    install::check_install_drift,
    lifecycle::{ReviewStatus, evaluate_lifecycle},
    manifest::{LifecycleState, ToolManifest, load_manifest},
    registry::manifest_paths,
    workstate::report::{ToolStatus, WorkstateFeed, WorkstateTool},
};

/// Build ToolFoundry's neutral Workstate feed from manifests and local checks.
pub fn load_workstate_feed(
    directory: impl AsRef<std::path::Path>,
    as_of: NaiveDate,
    generated_at: DateTime<Utc>,
) -> Result<WorkstateFeed> {
    let directory = directory.as_ref();
    let mut tools = Vec::new();
    // Track which manifest path first claimed each id so a duplicate can report
    // both offending files (P1-3). BTreeMap keeps the error message deterministic.
    let mut seen_ids: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();

    for path in manifest_paths(directory)? {
        let manifest = load_manifest(&path)?;
        let label = manifest_path_label(directory, &path);

        // Duplicate `identity.id` across manifests is a configuration bug: the feed
        // would otherwise emit two records with the same id and undefined precedence
        // for Workstate. Hard-fail with both paths.
        if let Some(first_path) = seen_ids.get(&manifest.identity.id) {
            return Err(ToolFoundryError::DuplicateToolId {
                id: manifest.identity.id.clone(),
                first_path: first_path.clone(),
                second_path: label,
            });
        }
        seen_ids.insert(manifest.identity.id.clone(), label.clone());

        let health = run_health_checks(&manifest)?;
        let drift = check_install_drift(&manifest)?;
        let lifecycle = evaluate_lifecycle(&manifest, as_of);

        tools.push(WorkstateTool::from_parts(
            label,
            manifest,
            health.passed_count(),
            health.outcomes.len(),
            !drift.is_current(),
            lifecycle.review_status == ReviewStatus::Due,
        ));
    }

    Ok(WorkstateFeed::new(generated_at, as_of, tools))
}

fn manifest_path_label(directory: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(directory)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Decide whether one tool needs operational attention in the Workstate feed.
///
/// A tool is flagged `attention` when ANY of these hold:
///   * not every health check passed (`health_passed != health_total`),
///   * its installed artifact has drifted from the manifest (`drifted`),
///   * its review is due (`review_due_flag`),
///   * its lifecycle state signals an *active problem*.
///
/// Lifecycle policy (P1-2): only `broken` and `risky` count as active problems.
/// `stale` and `deprecated` are deliberately treated as *managed/expected* states
/// — they are tracked elsewhere and should not, on their own, raise an attention
/// flag here. `experimental`, `active`, and `archived` are likewise not problems.
/// Pulled out as a free function so the rule can be unit-tested in isolation,
/// independent of manifest loading or the filesystem.
fn needs_attention(
    health_passed: usize,
    health_total: usize,
    drifted: bool,
    review_due_flag: bool,
    lifecycle_state: &LifecycleState,
) -> bool {
    let lifecycle_problem = matches!(
        lifecycle_state,
        LifecycleState::Broken | LifecycleState::Risky
    );

    health_passed != health_total || drifted || review_due_flag || lifecycle_problem
}

impl WorkstateTool {
    fn from_parts(
        manifest_path: String,
        manifest: ToolManifest,
        health_passed: usize,
        health_total: usize,
        drifted: bool,
        review_due_flag: bool,
    ) -> Self {
        let attention = needs_attention(
            health_passed,
            health_total,
            drifted,
            review_due_flag,
            &manifest.lifecycle.state,
        );

        Self {
            id: manifest.identity.id,
            display_name: manifest.identity.display_name,
            owner: manifest.ownership.owner,
            project: manifest.ownership.project,
            lifecycle_state: manifest.lifecycle.state.to_string(),
            status: if attention {
                ToolStatus::Attention
            } else {
                ToolStatus::Ok
            },
            review_after: manifest.lifecycle.review_after,
            review_due_flag,
            health_passed,
            health_total,
            drifted,
            manifest_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::{DateTime, NaiveDate, Utc};

    use crate::{manifest::LifecycleState, test_support::ManifestFixture, workstate::ToolStatus};

    use super::{load_workstate_feed, needs_attention};

    // ---- Attention rule: the three boolean inputs (health mismatch, drift, ----
    // ---- review-due) across all 2^3 = 8 combinations, with a neutral state. ----
    #[test]
    fn attention_rule_covers_all_boolean_combinations() {
        // Each row: (health_passed, health_total, drifted, review_due, expected).
        // health_passed != health_total encodes the "health mismatch" boolean.
        // With lifecycle Active (not a problem state), attention is true iff ANY
        // of the three booleans is set — i.e. everything except the all-false row.
        let cases = [
            (2usize, 2usize, false, false, false), // 0 0 0 -> ok
            (2, 2, false, true, true),             // 0 0 1 -> review due
            (2, 2, true, false, true),             // 0 1 0 -> drift
            (2, 2, true, true, true),              // 0 1 1
            (1, 2, false, false, true),            // 1 0 0 -> health fail
            (1, 2, false, true, true),             // 1 0 1
            (1, 2, true, false, true),             // 1 1 0
            (1, 2, true, true, true),              // 1 1 1
        ];

        for (passed, total, drifted, review_due, expected) in cases {
            let actual =
                needs_attention(passed, total, drifted, review_due, &LifecycleState::Active);
            assert_eq!(
                actual, expected,
                "passed={passed} total={total} drifted={drifted} review_due={review_due}"
            );
        }
    }

    // ---- Attention rule: lifecycle-state policy (P1-2). With all three ----
    // ---- booleans clear, ONLY broken/risky should raise attention. ----
    #[test]
    fn attention_rule_flags_only_broken_and_risky_lifecycle_states() {
        // (state, expected_attention) holding health=2/2, no drift, not due.
        let cases = [
            (LifecycleState::Experimental, false),
            (LifecycleState::Active, false),
            (LifecycleState::Stale, false), // managed/expected -> ok
            (LifecycleState::Deprecated, false), // managed/expected -> ok
            (LifecycleState::Risky, true),  // active problem -> attention
            (LifecycleState::Broken, true), // active problem -> attention
            (LifecycleState::Archived, false),
        ];

        for (state, expected) in cases {
            let actual = needs_attention(2, 2, false, false, &state);
            assert_eq!(actual, expected, "state={state}");
        }
    }

    #[test]
    fn lifecycle_problem_overrides_otherwise_healthy_tool() {
        // A fully healthy, non-drifted, not-due tool still gets attention if broken.
        assert!(needs_attention(3, 3, false, false, &LifecycleState::Broken));
        // ...but a deprecated one in the same shape stays ok.
        assert!(!needs_attention(
            3,
            3,
            false,
            false,
            &LifecycleState::Deprecated
        ));
    }

    // ---- Feed assembly (black-box) for the neutral Workstate contract. ----

    #[test]
    fn reports_attention_for_due_review_and_drift() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        workspace.write_default_manifest(&source, &target);

        // as_of on/after review_after (2026-09-01) makes the review due; the
        // missing symlink target makes the tool drifted.
        let as_of = NaiveDate::from_ymd_opt(2026, 9, 1).expect("date should be valid");
        let feed =
            load_workstate_feed(&workspace.path, as_of, generated_at()).expect("feed should load");

        assert_eq!(feed.attention_count, 1);
        assert_eq!(feed.tools[0].status, ToolStatus::Attention);
        assert!(feed.tools[0].review_due_flag);
        assert!(feed.tools[0].drifted);
    }

    #[test]
    fn reports_ok_for_current_manifest_state() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);
        workspace.write_default_manifest(&source, &target);

        let as_of = NaiveDate::from_ymd_opt(2026, 8, 31).expect("date should be valid");
        let feed =
            load_workstate_feed(&workspace.path, as_of, generated_at()).expect("feed should load");

        assert_eq!(feed.attention_count, 0);
        assert_eq!(feed.tools[0].status, ToolStatus::Ok);
        assert_eq!(feed.tools[0].health_passed, 1);
    }

    #[test]
    fn broken_lifecycle_state_reports_attention_end_to_end() {
        // Proves the P1-2 rule survives full manifest load + assembly, not just
        // the isolated unit. Tool is otherwise healthy and current.
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        fs::create_dir_all(target.parent().expect("target should have parent"))
            .expect("target parent should be created");
        create_symlink(&source, &target);
        workspace.write_manifest(
            ManifestFixture::with_paths(
                workspace.path.display(),
                source.display(),
                target.display(),
            )
            .with_lifecycle_state("broken")
            .yaml(),
        );

        let as_of = NaiveDate::from_ymd_opt(2026, 8, 31).expect("date should be valid");
        let feed =
            load_workstate_feed(&workspace.path, as_of, generated_at()).expect("feed should load");

        assert_eq!(feed.tools[0].status, ToolStatus::Attention);
        assert_eq!(feed.tools[0].lifecycle_state, "broken");
    }

    #[test]
    fn normalizes_manifest_path_to_directory_relative_value() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");
        workspace.write_default_manifest(&source, &target);

        let as_of = NaiveDate::from_ymd_opt(2026, 6, 2).expect("date should be valid");
        let feed =
            load_workstate_feed(&workspace.path, as_of, generated_at()).expect("feed should load");

        assert_eq!(feed.tools[0].manifest_path, "tool.yaml");
    }

    // ---- Duplicate id detection (P1-3) ----

    #[test]
    fn duplicate_tool_id_across_manifests_is_an_error() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");

        // Two manifests, distinct files, SAME identity.id ("backup-home" default).
        let manifest = ManifestFixture::with_paths(
            workspace.path.display(),
            source.display(),
            target.display(),
        )
        .yaml();
        fs::write(workspace.path.join("a.yaml"), &manifest)
            .expect("first manifest should be written");
        fs::write(workspace.path.join("b.yaml"), &manifest)
            .expect("second manifest should be written");

        let as_of = NaiveDate::from_ymd_opt(2026, 6, 2).expect("date should be valid");
        let error = load_workstate_feed(&workspace.path, as_of, generated_at())
            .expect_err("duplicate ids should hard-fail");

        let message = error.to_string();
        assert!(message.contains("backup-home"), "message: {message}");
        // Both offending file labels should appear so the user can fix the config.
        assert!(message.contains("a.yaml"), "message: {message}");
        assert!(message.contains("b.yaml"), "message: {message}");
    }

    #[test]
    fn distinct_tool_ids_do_not_trigger_duplicate_error() {
        let workspace = TempWorkspace::new();
        let source = workspace.path.join("backup-home");
        let target = workspace.path.join("bin").join("backup-home");
        File::create(&source).expect("source fixture should be created");

        fs::write(
            workspace.path.join("a.yaml"),
            ManifestFixture::with_paths(
                workspace.path.display(),
                source.display(),
                target.display(),
            )
            .yaml(),
        )
        .expect("first manifest should be written");
        fs::write(
            workspace.path.join("b.yaml"),
            ManifestFixture::with_paths(
                workspace.path.display(),
                source.display(),
                target.display(),
            )
            .with_identity("log-rotator", "Log Rotator", "Rotate logs.")
            .yaml(),
        )
        .expect("second manifest should be written");

        let as_of = NaiveDate::from_ymd_opt(2026, 6, 2).expect("date should be valid");
        let feed = load_workstate_feed(&workspace.path, as_of, generated_at())
            .expect("distinct ids should load");

        assert_eq!(feed.tool_count, 2);
    }

    // ---- Test harness helpers ----

    fn generated_at() -> DateTime<Utc> {
        "2026-06-02T00:00:00Z"
            .parse()
            .expect("timestamp should parse")
    }

    #[cfg(unix)]
    fn create_symlink(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).expect("symlink should be created");
    }

    #[cfg(windows)]
    fn create_symlink(source: &Path, target: &Path) {
        std::os::windows::fs::symlink_file(source, target).expect("symlink should be created");
    }

    static NEXT_DIR_ID: AtomicUsize = AtomicUsize::new(0);

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            // Combine a per-process nonce with a monotonic counter so parallel
            // tests never collide on the same temp directory.
            let id = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("toolfoundry-workstate-{nonce}-{id}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }

        fn write_default_manifest(&self, source: &Path, target: &Path) {
            self.write_manifest(
                ManifestFixture::with_paths(
                    self.path.display(),
                    source.display(),
                    target.display(),
                )
                .yaml(),
            );
        }

        fn write_manifest(&self, manifest: String) {
            let manifest_path = self.path.join("tool.yaml");
            fs::write(manifest_path, manifest).expect("fixture manifest should be written");
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
