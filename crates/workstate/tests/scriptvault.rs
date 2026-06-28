use workstate::ingest::scriptvault::ScriptVaultFeed;
use workstate::ingest::{FeedError, FeedSource};
use workstate::model::normalized::{Script, ScriptId};

/// Absolute path to the in-repo ScriptVault fixture, anchored on `CARGO_MANIFEST_DIR`.
fn fixture_path() -> String {
    format!(
        "{}/tests/fixtures/scriptvault/export_v1.json",
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
        "workstate_scriptvault_{}_{}.json",
        tag,
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("temp feed must be writable");
    path
}

// =============================================================================
// 1. PARITY — well-formed v1 fixture normalizes to the exact set of scripts.
// =============================================================================
#[test]
fn wellformed_fixture_normalizes_to_expected_scripts() {
    let feed = ScriptVaultFeed::from_path(fixture_path());
    let raw = feed.fetch().expect("v1 fixture must fetch+parse");

    // Envelope fidelity: the fields RexOps reads, we read identically — including the
    // loose `generated_at` string and the lenient `favorites`/`recents` arrays.
    assert_eq!(raw.schema_version, Some(1));
    assert_eq!(raw.source_tool, "scriptvault");
    assert_eq!(raw.generated_at, "2026-06-04");
    assert_eq!(raw.scripts.len(), 3);
    assert_eq!(raw.favorites, vec!["deploy-prod".to_string()]);
    assert_eq!(
        raw.recents,
        vec!["deploy-prod".to_string(), "backup-db".to_string()]
    );

    // Normalize and assert the EXACT canonical set, in order — all three kept.
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.generated_at, "2026-06-04");
    assert_eq!(inventory.favorites, vec!["deploy-prod".to_string()]);
    assert_eq!(
        inventory.recents,
        vec!["deploy-prod".to_string(), "backup-db".to_string()]
    );
    let scripts = inventory.scripts;
    let expected = vec![
        Script {
            id: ScriptId("deploy-prod".to_string()),
            name: Some("deploy-prod.sh".to_string()),
            description: Some("Deploy to production with safety checks".to_string()),
            rest: Default::default(),
        },
        Script {
            id: ScriptId("backup-db".to_string()),
            name: Some("backup-db.sh".to_string()),
            description: Some("Nightly database backup".to_string()),
            rest: Default::default(),
        },
        Script {
            id: ScriptId("cleanup-logs".to_string()),
            name: Some("cleanup-logs.py".to_string()),
            description: Some("Rotate and compress old logs".to_string()),
            rest: Default::default(),
        },
    ];
    assert_eq!(scripts, expected);
}

// =============================================================================
// 2. Unknown top-level field is ignored (forward-compat leniency)
// =============================================================================
#[test]
fn unknown_toplevel_field_is_ignored() {
    let path = write_temp(
        "unknown_top",
        r#"{"_comment": "hi", "schema_version": 1, "scripts": []}"#,
    );
    let feed = ScriptVaultFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("unknown top-level key must not fail parse");
    assert_eq!(raw.schema_version, Some(1));
    assert!(raw.scripts.is_empty());
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 3. Raw boundary: missing schema_version still parses
// =============================================================================
#[test]
fn missing_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "missing_ver",
        r#"{"source_tool": "scriptvault", "scripts": [{"id": "a"}]}"#,
    );
    let feed = ScriptVaultFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("a missing schema_version must NOT fail the parse");
    assert_eq!(raw.schema_version, None);
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.scripts.len(), 1);
    assert_eq!(inventory.scripts[0].id.0, "a");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 4. Raw boundary: unknown schema_version still parses
// =============================================================================
#[test]
fn unknown_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "unknown_ver",
        r#"{"schema_version": 99, "scripts": [{"id": "b"}]}"#,
    );
    let feed = ScriptVaultFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("unknown version must parse");
    assert_eq!(raw.schema_version, Some(99));
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.scripts.len(), 1);
    assert_eq!(inventory.scripts[0].id.0, "b");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 5. SKIP-AND-DROP — id-less / blank / whitespace-id records dropped, rest kept
// =============================================================================
#[test]
fn idless_and_blank_id_records_are_dropped_rest_kept() {
    let text = r#"{
        "schema_version": 1,
        "source_tool": "scriptvault",
        "scripts": [
            {"id": "keep-1", "name": "one.sh"},
            {"name": "no-id-here.sh", "description": "missing id → dropped"},
            {"id": "keep-2", "future": 42},
            {"id": "", "name": "empty-id.sh"},
            {"id": "   ", "name": "whitespace-id.sh"}
        ]
    }"#;
    let path = write_temp("skip_drop", text);
    let feed = ScriptVaultFeed::from_path(path.to_string_lossy().into_owned());

    let raw = feed
        .fetch()
        .expect("feed must parse despite id-less records");
    // All FIVE records parse at the wire boundary...
    assert_eq!(raw.scripts.len(), 5);
    // ...but normalization keeps only the two with a usable id.
    let inventory = feed.normalize(raw);
    let kept_ids: Vec<&str> = inventory.scripts.iter().map(|s| s.id.0.as_str()).collect();
    assert_eq!(kept_ids, vec!["keep-1", "keep-2"]);
    assert_eq!(inventory.scripts.len(), 2);
    // The three id-less / blank-id records were dropped; the count must be recorded
    // so the loss is never silent.
    assert_eq!(inventory.dropped_records, 3);
    assert_eq!(
        inventory.scripts[1].rest.get("future").unwrap(),
        &serde_json::Value::from(42)
    );
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 6. END-TO-END (success path): the REAL `fetch()` against the on-disk fixture
// =============================================================================
#[test]
fn fetch_reads_fixture_from_disk_and_normalizes() {
    let feed = ScriptVaultFeed::from_path(fixture_path());
    let raw = feed.fetch().expect("fixture must fetch+parse from disk");
    assert_eq!(raw.schema_version, Some(1));
    let inventory = feed.normalize(raw);
    let ids: Vec<&str> = inventory.scripts.iter().map(|s| s.id.0.as_str()).collect();
    assert_eq!(ids, vec!["deploy-prod", "backup-db", "cleanup-logs"]);
}

// =============================================================================
// 7. MISSING FILE → `FeedError::NotFound`
// =============================================================================
#[test]
fn missing_file_yields_not_found() {
    let feed = ScriptVaultFeed::from_path(
        "/no/such/workstate/scriptvault/missing-xyz123.json".to_string(),
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
// 8. INVALID JSON → `FeedError::Parse`
// =============================================================================
#[test]
fn invalid_json_yields_parse_error() {
    let path = write_temp("bad_json", "{not valid json");
    let feed = ScriptVaultFeed::from_path(path.to_string_lossy().into_owned());
    let err = feed.fetch().expect_err("malformed JSON must be an error");
    assert!(
        matches!(err, FeedError::Parse(_)),
        "expected Parse, got {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}
