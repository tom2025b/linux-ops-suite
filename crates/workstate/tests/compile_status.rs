use workstate::compile::SnapshotBuilder;
use workstate::ingest::bulwark::BulwarkFeed;
use workstate::ingest::proto::ProtoFeed;
use workstate::ingest::scriptvault::ScriptVaultFeed;
use workstate::ingest::toolfoundry::ToolFoundryFeed;
use workstate::model::provenance::FeedStatus;

fn write_temp(tag: &str, contents: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "workstate_compile_status_{}_{}.json",
        tag,
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("temp feed must be writable");
    path
}

fn missing_path(tag: &str) -> String {
    format!(
        "/no/such/workstate/compile-status-{}-{}.json",
        tag,
        std::process::id()
    )
}

fn builder_with_scriptvault(path: impl Into<String>) -> SnapshotBuilder {
    SnapshotBuilder::new(
        BulwarkFeed::from_path(missing_path("bulwark")),
        ScriptVaultFeed::from_path(path.into()),
        ToolFoundryFeed::from_path(missing_path("toolfoundry")),
        ProtoFeed::from_path(missing_path("proto")),
    )
}

#[test]
fn source_timestamp_drives_stale_status_and_provenance() {
    let path = write_temp(
        "stale",
        r#"{
            "schema_version": 1,
            "source_tool": "scriptvault",
            "generated_at": "2000-01-01",
            "scripts": [{"id": "stale-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert_eq!(snapshot.scripts.status, FeedStatus::Stale);
    assert_eq!(
        snapshot
            .scripts
            .provenance
            .source_observed_at
            .expect("source timestamp should parse")
            .to_rfc3339(),
        "2000-01-01T00:00:00+00:00"
    );
    assert_eq!(
        snapshot
            .scripts
            .data
            .as_ref()
            .expect("stale data should still be attached")
            .scripts
            .len(),
        1
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn unparseable_source_timestamp_is_freshness_unknown_not_fresh() {
    // A feed that reads cleanly and has a supported schema_version, but whose
    // `generated_at` cannot be parsed (here: empty). Its source age is unknown, so
    // it must NOT be labeled Fresh — freshness fails closed to FreshnessUnknown.
    // The data is still attached (the records are fine; only the AGE is unknown).
    let path = write_temp(
        "unknown_age",
        r#"{
            "schema_version": 1,
            "source_tool": "scriptvault",
            "generated_at": "",
            "scripts": [{"id": "ageless-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert_eq!(
        snapshot.scripts.status,
        FeedStatus::FreshnessUnknown,
        "a feed with no parseable source timestamp must not default to Fresh"
    );
    assert!(
        snapshot.scripts.provenance.source_observed_at.is_none(),
        "an unparseable generated_at leaves the source time unknown"
    );
    assert_eq!(
        snapshot
            .scripts
            .data
            .as_ref()
            .expect("unknown-age data should still be attached")
            .scripts
            .len(),
        1,
        "unknown age must not drop the data, only withhold the Fresh label"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn dropped_records_count_propagates_to_provenance() {
    // Two records are id-less and get dropped during normalization. The count must
    // surface on the SECTION's provenance (not just inside the inventory), so the
    // loss is visible to RexOps even though the section otherwise reads healthy.
    let path = write_temp(
        "dropped",
        r#"{
            "schema_version": 1,
            "source_tool": "scriptvault",
            "generated_at": "2000-01-01",
            "scripts": [
                {"id": "kept"},
                {"name": "no-id-dropped"},
                {"id": "   "}
            ]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    // One record survives, two were dropped, and the drop count is on provenance.
    assert_eq!(snapshot.scripts.provenance.dropped_records, 2);
    assert_eq!(
        snapshot
            .scripts
            .data
            .as_ref()
            .expect("data should be attached")
            .scripts
            .len(),
        1
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn unsupported_schema_version_is_rejected_without_normalizing_data() {
    let path = write_temp(
        "unsupported",
        r#"{
            "schema_version": 99,
            "source_tool": "scriptvault",
            "generated_at": "2026-06-04",
            "scripts": [{"id": "future-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert_eq!(
        snapshot.scripts.status,
        FeedStatus::UnsupportedVersion {
            found: Some(99),
            supported: 1
        }
    );
    assert!(
        snapshot.scripts.data.is_none(),
        "unsupported feeds must not normalize into snapshot data"
    );
    assert!(
        snapshot.scripts.provenance.source_observed_at.is_some(),
        "provenance should still record source time for rejected parseable feeds"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn missing_schema_version_is_rejected_end_to_end() {
    // POLICY (see compile::compile_section): an ABSENT schema_version is rejected
    // and its data dropped, exactly as strictly as a wrong version — but it gets a
    // STRUCTURALLY DISTINCT status, `MissingVersion`, so a consumer can tell
    // "producer forgot to stamp a version" from "producer stamped one we don't
    // support" (the latter is `UnsupportedVersion`, covered by the wrong-version
    // test). Workstate refuses to bake an unverified shape into the persisted
    // snapshot. This test pins the missing-version arm through the full compile path.
    let path = write_temp(
        "missing_version",
        r#"{
            "source_tool": "scriptvault",
            "generated_at": "2026-06-04",
            "scripts": [{"id": "no-version-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert_eq!(
        snapshot.scripts.status,
        FeedStatus::MissingVersion { supported: 1 },
        "a missing schema_version must be its own status, distinct from a wrong one"
    );
    assert!(
        snapshot.scripts.data.is_none(),
        "version-less feeds must not normalize into snapshot data"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn mismatched_source_tool_is_rejected_end_to_end() {
    // #5: a feed that reads + parses fine but whose self-reported `source_tool`
    // disagrees with the adapter (here: a "bulwark" export handed to the ScriptVault
    // adapter) is rejected as SourceMismatch, data dropped — so swapped/misconfigured
    // feeds never get normalized under the wrong feed_id in the source of truth.
    let path = write_temp(
        "source_mismatch",
        r#"{
            "schema_version": 1,
            "source_tool": "bulwark",
            "generated_at": "2026-06-04",
            "scripts": [{"id": "wrong-tool-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert_eq!(
        snapshot.scripts.status,
        FeedStatus::SourceMismatch {
            expected: "scriptvault".to_string(),
            found: "bulwark".to_string(),
        },
        "a feed whose source_tool disagrees with the adapter must be rejected"
    );
    assert!(
        snapshot.scripts.data.is_none(),
        "a mismatched-source feed must not normalize into snapshot data"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_source_tool_is_tolerated_not_a_mismatch() {
    // The cross-check tolerates an EMPTY source_tool (prior leniency): a feed that
    // omits it still compiles normally rather than being rejected as SourceMismatch.
    let path = write_temp(
        "empty_source",
        r#"{
            "schema_version": 1,
            "source_tool": "",
            "generated_at": "2026-06-04",
            "scripts": [{"id": "ok-script"}]
        }"#,
    );

    let snapshot = builder_with_scriptvault(path.to_string_lossy().into_owned()).build();

    assert!(
        !matches!(snapshot.scripts.status, FeedStatus::SourceMismatch { .. }),
        "an empty source_tool must be tolerated, not flagged as a mismatch"
    );
    assert!(
        snapshot.scripts.data.is_some(),
        "an empty source_tool must still normalize the data"
    );

    let _ = std::fs::remove_file(&path);
}
