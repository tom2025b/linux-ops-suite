use chrono::NaiveDate;
use workstate::ingest::toolfoundry::ToolFoundryFeed;
use workstate::ingest::{FeedError, FeedSource};
use workstate::model::normalized::{Tool, ToolId};

/// Absolute path to the in-repo ToolFoundry fixture, anchored on `CARGO_MANIFEST_DIR`.
fn fixture_path() -> String {
    format!(
        "{}/tests/fixtures/toolfoundry/workstate_feed_v1.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Write `contents` to a unique temp file and return its path, no `tempfile` crate
/// needed. COLLISION-SAFETY: the `tag` must be unique per call site within this file —
/// all threads of one test binary share a PID, so the tag prevents two concurrent
/// tests racing on the same path; the PID only separates this run from other binaries
/// / earlier runs. Cleanup is best-effort at each test's end (a file leaks if an
/// assertion panics first — acceptable under the zero-dev-dependency charter).
fn write_temp(tag: &str, contents: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "workstate_toolfoundry_{}_{}.json",
        tag,
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("temp feed must be writable");
    path
}

// =============================================================================
// 1. PARITY — well-formed v1 fixture normalizes to the exact set of tools.
// =============================================================================
#[test]
fn wellformed_fixture_normalizes_to_expected_tools() {
    let feed = ToolFoundryFeed::from_path(fixture_path());
    let raw = feed.fetch().expect("v1 fixture must fetch+parse");

    // Envelope fidelity: the self-reported counts are now kept through normalize.
    assert_eq!(raw.schema_version, Some(1));
    assert_eq!(raw.source_tool, "toolfoundry");
    assert_eq!(raw.generated_at, "2026-06-02T00:00:00Z");
    assert_eq!(raw.as_of, "2026-06-02");
    assert_eq!(raw.tool_count, 2);
    assert_eq!(raw.attention_count, 1);
    assert_eq!(raw.tools.len(), 2);

    let inventory = feed.normalize(raw);
    assert_eq!(inventory.as_of, "2026-06-02");
    assert_eq!(inventory.tool_count, 2);
    assert_eq!(inventory.attention_count, 1);
    let tools = inventory.tools;
    let expected = vec![
        Tool {
            id: ToolId("backup-home".to_string()),
            display_name: "Backup Home".to_string(),
            owner: "tom".to_string(),
            project: "backupsage".to_string(),
            lifecycle_state: "active".to_string(),
            status: "attention".to_string(),
            review_due: None,
            review_after: Some(review_date()),
            review_due_flag: false,
            drifted: true,
            health_passed: 0,
            health_total: 2,
            manifest_path: "default.yaml".to_string(),
        },
        Tool {
            id: ToolId("log-rotator".to_string()),
            display_name: "Log Rotator".to_string(),
            owner: "tom".to_string(),
            project: "ops-utils".to_string(),
            lifecycle_state: "active".to_string(),
            status: "ok".to_string(),
            review_due: None,
            review_after: Some(review_date()),
            review_due_flag: true,
            drifted: false,
            health_passed: 3,
            health_total: 3,
            manifest_path: "log-rotator.yaml".to_string(),
        },
    ];
    assert_eq!(tools, expected);
}

// =============================================================================
// 2. RECONCILIATION 1 — `review_due_flag: true` stays available as a flag
// =============================================================================
/// A tool whose `review_due_flag` is `true` keeps the v3 timestamp absent for
/// RexOps compatibility while preserving the source bool.
#[test]
fn review_due_true_on_wire_is_preserved_as_flag() {
    let path = write_temp(
        "review_true",
        r#"{
        "schema_version": 1,
        "tools": [{"id": "needs-review", "review_due_flag": true}]
    }"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("must parse");
    // Raw faithfully preserved the wire bool...
    assert!(
        raw.tools[0].review_due_flag == Some(true),
        "raw should keep the wire bool as true"
    );
    // ...but normalization yields None (the divergence).
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.tools.len(), 1);
    assert_eq!(inventory.tools[0].review_due, None);
    assert_eq!(inventory.tools[0].review_after, None);
    assert!(inventory.tools[0].review_due_flag);
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 3. PASSTHROUGH — every non-id, non-review flag field survives unchanged
// =============================================================================
#[test]
fn all_passthrough_fields_survive_normalization() {
    let path = write_temp(
        "passthrough",
        r#"{
        "schema_version": 1,
        "tools": [{
            "id": "t1",
            "display_name": "Tool One",
            "owner": "alice",
            "project": "proj-x",
            "lifecycle_state": "deprecated",
            "review_after": "2026-09-01",
            "review_due_flag": false,
            "health_passed": 5,
            "health_total": 7,
            "drifted": true,
            "status": "broken",
            "manifest_path": "tools/t1.yaml"
        }]
    }"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("must parse");
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.tools.len(), 1);
    assert_eq!(
        inventory.tools[0],
        Tool {
            id: ToolId("t1".to_string()),
            display_name: "Tool One".to_string(),
            owner: "alice".to_string(),
            project: "proj-x".to_string(),
            lifecycle_state: "deprecated".to_string(),
            status: "broken".to_string(),
            review_due: None,
            review_after: Some(review_date()),
            review_due_flag: false,
            drifted: true,
            health_passed: 5,
            health_total: 7,
            manifest_path: "tools/t1.yaml".to_string(),
        }
    );
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 4. CONTRACT BOUNDARY — legacy `review_due` is not a ToolFoundry input
// =============================================================================
#[test]
fn legacy_review_due_is_ignored_in_neutral_feed() {
    let path = write_temp(
        "legacy_review_due",
        r#"{
        "schema_version": 1,
        "tools": [
            {"id": "legacy", "review_due": true},
            {"id": "new-false-wins", "review_due": true, "review_due_flag": false}
        ]
    }"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());

    let raw = feed.fetch().expect("feed must parse");
    assert_eq!(raw.tools[0].review_due_flag, None);
    assert_eq!(raw.tools[1].review_due_flag, Some(false));

    let inventory = feed.normalize(raw);
    let flags: Vec<bool> = inventory
        .tools
        .iter()
        .map(|tool| tool.review_due_flag)
        .collect();
    assert_eq!(flags, vec![false, false]);
    assert!(inventory.tools.iter().all(|tool| tool.review_due.is_none()));

    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 5. SKIP-AND-DROP — id-less / blank-id records dropped, rest kept
// =============================================================================
#[test]
fn idless_and_blank_id_records_are_dropped_rest_kept() {
    let path = write_temp(
        "skip_drop",
        r#"{
        "schema_version": 1,
        "tools": [
            {"id": "keep-1", "display_name": "Keep One"},
            {"display_name": "no id at all"},
            {"id": "keep-2", "future_field": 42},
            {"id": "", "display_name": "empty id"},
            {"id": "   ", "display_name": "whitespace id"}
        ]
    }"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("feed must parse despite id-less records");
    // All FIVE records parse at the wire boundary...
    assert_eq!(raw.tools.len(), 5);
    // ...but normalization keeps only the two with a usable id.
    let inventory = feed.normalize(raw);
    let kept: Vec<&str> = inventory.tools.iter().map(|t| t.id.0.as_str()).collect();
    assert_eq!(kept, vec!["keep-1", "keep-2"]);
    assert_eq!(inventory.tools.len(), 2);
    // The three id-less / blank-id records were dropped; the count must be recorded.
    assert_eq!(inventory.dropped_records, 3);
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 6. Unknown top-level field is ignored (forward-compat leniency)
// =============================================================================
#[test]
fn unknown_toplevel_field_is_ignored() {
    let path = write_temp(
        "unknown_top",
        r#"{"_comment": "hi", "schema_version": 1, "tools": []}"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("unknown top-level key must not fail parse");
    assert_eq!(raw.schema_version, Some(1));
    assert!(raw.tools.is_empty());
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 7. Unknown per-tool field lands nowhere (no `rest` bag) but does not fail
// =============================================================================
/// ToolFoundry's record is fixed (no `rest` bag), so an unmodeled per-tool key is
/// silently dropped — NOT preserved, and NOT an error.
#[test]
fn unknown_per_tool_field_is_silently_ignored() {
    let path = write_temp(
        "unknown_tool_field",
        r#"{"schema_version": 1, "tools": [{"id": "t", "surprise": [1,2,3]}]}"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("unknown per-tool field must not fail parse");
    assert_eq!(raw.tools.len(), 1);
    assert_eq!(raw.tools[0].id.as_deref(), Some("t"));
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.tools.len(), 1);
    assert_eq!(inventory.tools[0].id.0, "t");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 8. Raw boundary: missing schema_version still parses
// =============================================================================
#[test]
fn missing_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "missing_ver",
        r#"{"as_of": "2026-06-02", "tools": [{"id": "a"}]}"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("a missing schema_version must NOT fail the parse");
    assert_eq!(raw.schema_version, None);
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.tools.len(), 1);
    assert_eq!(inventory.tools[0].id.0, "a");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 9. Raw boundary: unknown schema_version still parses
// =============================================================================
#[test]
fn unknown_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "unknown_ver",
        r#"{"schema_version": 99, "tools": [{"id": "b"}]}"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("unknown version must parse");
    assert_eq!(raw.schema_version, Some(99));
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.tools.len(), 1);
    assert_eq!(inventory.tools[0].id.0, "b");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 10. END-TO-END (success path): the REAL `fetch()` against the on-disk fixture
// =============================================================================
#[test]
fn fetch_reads_fixture_from_disk_and_normalizes() {
    let feed = ToolFoundryFeed::from_path(fixture_path());
    let raw = feed.fetch().expect("fixture must fetch+parse from disk");
    assert_eq!(raw.schema_version, Some(1));
    let inventory = feed.normalize(raw);
    let ids: Vec<&str> = inventory.tools.iter().map(|t| t.id.0.as_str()).collect();
    assert_eq!(ids, vec!["backup-home", "log-rotator"]);
    // Spot-check the divergence held through the disk path too.
    assert!(inventory.tools.iter().all(|t| t.review_due.is_none()));
    assert!(inventory
        .tools
        .iter()
        .all(|t| t.review_after == Some(review_date())));
    assert_eq!(
        inventory
            .tools
            .iter()
            .map(|t| t.review_due_flag)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

// =============================================================================
// 11. MISSING FILE → `FeedError::NotFound`
// =============================================================================
#[test]
fn missing_file_yields_not_found() {
    let feed = ToolFoundryFeed::from_path(
        "/no/such/workstate/toolfoundry/missing-xyz123.json".to_string(),
    );
    let err = feed
        .fetch()
        .expect_err("a missing feed file must be an error");
    assert!(
        matches!(err, FeedError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

// =============================================================================
// 12. INVALID JSON → `FeedError::Parse`
// =============================================================================
#[test]
fn invalid_json_yields_parse_error() {
    let path = write_temp("bad_json", "{not valid json");
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let err = feed.fetch().expect_err("malformed JSON must be an error");
    assert!(
        matches!(err, FeedError::Parse(_)),
        "expected Parse, got {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 13. Empty `tools` array normalizes to an empty ToolInventory (not an error)
// =============================================================================
#[test]
fn empty_tools_normalizes_to_empty_inventory() {
    let path = write_temp("empty_tools", r#"{"schema_version": 1, "tools": []}"#);
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("empty tools must parse");
    let inventory = feed.normalize(raw);
    assert!(inventory.tools.is_empty());
    // Derived counts on an empty inventory are zero, regardless of the (absent)
    // envelope numbers.
    assert_eq!(inventory.tool_count, 0);
    assert_eq!(inventory.attention_count, 0);
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 14. COUNTS ARE RECOMPUTED, NOT ECHOED — a lying envelope is ignored.
// =============================================================================
/// The envelope deliberately self-reports wrong counts (tool_count 99,
/// attention_count 0). Normalization must IGNORE them and derive the truth from
/// `tools[]`: tool_count = survivors (the id-less record is dropped), and
/// attention_count = tools whose `status` is "attention" (case/space insensitive).
#[test]
fn counts_are_recomputed_from_tools_not_echoed_from_envelope() {
    let path = write_temp(
        "recount",
        r#"{
        "schema_version": 1,
        "tool_count": 99,
        "attention_count": 0,
        "tools": [
            {"id": "a", "status": "attention"},
            {"id": "b", "status": "  ATTENTION  "},
            {"id": "c", "status": "ok"},
            {"status": "attention"}
        ]
    }"#,
    );
    let feed = ToolFoundryFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("must parse");
    // The raw envelope still faithfully carries the (wrong) self-reported numbers...
    assert_eq!(raw.tool_count, 99);
    assert_eq!(raw.attention_count, 0);

    // ...but the normalized inventory derives them from the survivors.
    let inventory = feed.normalize(raw);
    // The id-less record is dropped, so 3 tools survive (not 99, not 4).
    assert_eq!(inventory.tools.len(), 3);
    assert_eq!(inventory.tool_count, 3);
    // Two survivors are "attention" (one padded/upper-cased), so attention_count = 2.
    assert_eq!(inventory.attention_count, 2);
    // The single id-less record that was dropped is accounted for, separately from
    // the recomputed counts.
    assert_eq!(inventory.dropped_records, 1);
    let _ = std::fs::remove_file(&path);
}

fn review_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 1).expect("review date should be valid")
}
