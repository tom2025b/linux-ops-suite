//! Integration tests: the full Workstate-mediated pipeline.
//!
//! The input snapshot is not hand-written — it is compiled by the REAL
//! Workstate library (`SnapshotBuilder` + `write_snapshot`) from a Bulwark
//! workstate-feed fixture. That way the bridge is tested against exactly the
//! bytes Workstate publishes, and a Workstate contract change that would
//! break the bridge breaks these tests first.

use std::path::{Path, PathBuf};
use std::process::Command;

use workstate::compile::SnapshotBuilder;
use workstate::ingest::bulwark::BulwarkFeed;
use workstate::ingest::scriptvault::ScriptVaultFeed;
use workstate::ingest::toolfoundry::ToolFoundryFeed;
use workstate::write_snapshot;

use toolbox_bridge::{convert, feed, snapshot, BridgeError, SidecarFeed};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bulwark.workstate-feed.v1.json"
);

/// Compile a real v4 snapshot whose findings section comes from `bulwark_path`.
/// ScriptVault/ToolFoundry feeds point nowhere — their sections degrade to
/// Missing, which must never bother the bridge.
fn compile_snapshot(bulwark_path: &str, out: &Path) {
    let builder = SnapshotBuilder::new(
        BulwarkFeed {
            path: bulwark_path.to_string(),
        },
        ScriptVaultFeed {
            path: "/nonexistent/scriptvault.json".to_string(),
        },
        ToolFoundryFeed {
            path: "/nonexistent/toolfoundry.json".to_string(),
        },
    );
    write_snapshot(&builder.build(), out).expect("write snapshot fixture");
}

fn snapshot_from_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("workstate.snapshot.json");
    compile_snapshot(FIXTURE, &path);
    path
}

#[test]
fn end_to_end_snapshot_to_sidecar_feed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot_path = snapshot_from_fixture(dir.path());

    let snap = snapshot::load_snapshot(&snapshot_path).expect("load");
    let view = snapshot::findings_view(&snap).expect("findings");
    let conversion = convert::convert(&view.inventory.findings);

    // 5 fixture findings -> 3 sidecars (orphan has no path, sidecar-suffix
    // path is skipped), sorted by path.
    assert_eq!(conversion.skipped.len(), 2);
    let paths: Vec<_> = conversion
        .sidecars
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec![
            "/home/tom/bin/cleanup-logs.py",
            "/home/tom/bin/deploy-prod.sh",
            "/home/tom/bin/healthcheck.sh",
        ]
    );

    let deploy = &conversion.sidecars[1];
    assert_eq!(deploy.tags, vec!["risk:critical", "owner:tom"]);
    assert_eq!(
        deploy.desc.as_deref(),
        Some("AWS access key ID detected  [RISK: CRITICAL]")
    );

    // Leading separator junk and whitespace runs flattened; low risk -> no badge.
    let cleanup = &conversion.sidecars[0];
    assert_eq!(cleanup.tags, vec!["risk:low", "owner:user"]);
    assert_eq!(
        cleanup.desc.as_deref(),
        Some("Email address found in comments")
    );

    // No `risk` passthrough -> falls back to the raw severity string.
    let health = &conversion.sidecars[2];
    assert_eq!(health.tags, vec!["risk:medium"]);
    assert_eq!(
        health.desc.as_deref(),
        Some("curl piped to shell  [RISK: MEDIUM]")
    );

    // Publish and read back: envelope follows the suite contract rules.
    let feed_path = dir.path().join("feeds/toolbox-bridge.json");
    let out = SidecarFeed::new(
        conversion.sidecars,
        &view.inventory.generated_at,
        chrono::Utc::now(),
    );
    feed::write_feed(&out, &feed_path).expect("write feed");

    let parsed: SidecarFeed =
        serde_json::from_str(&std::fs::read_to_string(&feed_path).expect("read")).expect("parse");
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.source_tool, "toolbox-bridge");
    assert_eq!(parsed.item_count, 3);
    assert_eq!(parsed.source_generated_at, "2026-06-10T08:00:00Z");
    assert_eq!(parsed.sidecars.len(), 3);
}

#[test]
fn missing_snapshot_is_a_clean_error() {
    let err = snapshot::load_snapshot(Path::new("/nonexistent/snapshot.json"))
        .expect_err("must not succeed");
    assert!(matches!(err, BridgeError::SnapshotNotFound(_)), "{err}");
    assert!(err.to_string().contains("run `workstate` first"), "{err}");
}

#[test]
fn malformed_snapshot_is_a_parse_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snapshot.json");
    std::fs::write(&path, "{ not json").expect("write");
    let err = snapshot::load_snapshot(&path).expect_err("must not parse");
    assert!(matches!(err, BridgeError::SnapshotParse { .. }), "{err}");
}

#[test]
fn unknown_schema_version_is_refused_before_shape_parsing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snapshot.json");
    // v99 with a shape our Snapshot type could never hold: version check
    // must win over the field-level parse error.
    std::fs::write(&path, r#"{"schema_version": 99, "everything": "changed"}"#).expect("write");
    let err = snapshot::load_snapshot(&path).expect_err("must refuse");
    match err {
        BridgeError::UnsupportedSchema { found, supported } => {
            assert_eq!(found, Some(99));
            // Bind to Workstate's own constant so this can't drift when the
            // contract version bumps (currently v4).
            assert_eq!(supported, workstate::model::snapshot::SCHEMA_VERSION);
        }
        other => panic!("expected UnsupportedSchema, got: {other}"),
    }
}

#[test]
fn missing_findings_section_degrades_to_a_clear_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snapshot.json");
    // Bulwark feed absent -> findings section Missing in a perfectly valid snapshot.
    compile_snapshot("/nonexistent/bulwark.json", &path);

    let snap = snapshot::load_snapshot(&path).expect("snapshot itself is fine");
    let err = snapshot::findings_view(&snap).expect_err("no findings to bridge");
    match err {
        BridgeError::FindingsUnavailable { status } => assert_eq!(status, "Missing"),
        other => panic!("expected FindingsUnavailable, got: {other}"),
    }
}

// --- CLI behaviour (the installed binary, end to end) -----------------------

fn bridge_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_toolbox-bridge"))
}

#[test]
fn cli_writes_the_feed_and_reports_each_stage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot_path = snapshot_from_fixture(dir.path());
    let feed_path = dir.path().join("feeds/toolbox-bridge.json");

    let output = bridge_cmd()
        .arg("--snapshot")
        .arg(&snapshot_path)
        .arg("--output")
        .arg(&feed_path)
        .output()
        .expect("run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("Read snapshot"), "stdout: {stdout}");
    assert!(
        stdout.contains("3 sidecar record(s), 2 skipped"),
        "stdout: {stdout}"
    );
    // Skips are reported per subject on stderr.
    assert!(stderr.contains("orphan-finding"), "stderr: {stderr}");

    let parsed: SidecarFeed =
        serde_json::from_str(&std::fs::read_to_string(&feed_path).expect("read")).expect("parse");
    assert_eq!(parsed.item_count, 3);
}

#[test]
fn cli_dry_run_prints_the_feed_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot_path = snapshot_from_fixture(dir.path());
    let feed_path = dir.path().join("feeds/toolbox-bridge.json");

    let output = bridge_cmd()
        .arg("--snapshot")
        .arg(&snapshot_path)
        .arg("--output")
        .arg(&feed_path)
        .arg("--dry-run")
        .output()
        .expect("run binary");

    assert!(output.status.success());
    // Stdout IS the feed (pipeable JSON preview).
    let parsed: SidecarFeed =
        serde_json::from_slice(&output.stdout).expect("stdout parses as the feed");
    assert_eq!(parsed.item_count, 3);
    assert!(!feed_path.exists(), "dry run must not write");
}

#[test]
fn cli_missing_snapshot_fails_with_guidance() {
    let output = bridge_cmd()
        .arg("--snapshot")
        .arg("/nonexistent/snapshot.json")
        .arg("--output")
        .arg("/tmp/never-written.json")
        .output()
        .expect("run binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("snapshot not found"), "stderr: {stderr}");
    assert!(stderr.contains("run `workstate` first"), "stderr: {stderr}");
}
