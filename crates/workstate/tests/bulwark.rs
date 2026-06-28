use workstate::ingest::bulwark::BulwarkFeed;
use workstate::ingest::{FeedError, FeedSource};
use workstate::model::normalized::{Finding, FindingId, Severity};

/// Absolute path to the in-repo Bulwark fixture, anchored on `CARGO_MANIFEST_DIR`
/// (the crate root at compile time) so the test never depends on the process cwd.
fn fixture_path() -> String {
    format!(
        "{}/tests/fixtures/bulwark/scan_feed_v1.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Write `contents` to a unique temp file and return its path, with no `tempfile`
/// crate needed. COLLISION-SAFETY: the `tag` must be unique per call site within this
/// file — all threads of one test binary share a PID, so the tag is what prevents two
/// concurrent tests from racing on the same path; the PID only separates this run from
/// other test binaries / earlier runs. (Cleanup is best-effort `remove_file` at the
/// end of each test, so a file leaks if an assertion panics first — acceptable under
/// the zero-dev-dependency charter.) Used by the cases that drive `fetch` against
/// on-disk JSON (the pure `parse` is private, invisible to this external test crate).
fn write_temp(tag: &str, contents: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "workstate_bulwark_{}_{}.json",
        tag,
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("temp feed must be writable");
    path
}

// =============================================================================
// 1. PARITY — well-formed v1 fixture (via the real fetch path) normalizes to the
//    exact set of findings the fixture describes. This is the core assertion.
// =============================================================================
#[test]
fn wellformed_fixture_normalizes_to_expected_findings() {
    let feed = BulwarkFeed::from_path(fixture_path());

    // Drive the REAL read+parse path against the on-disk fixture.
    let raw = feed.fetch().expect("v1 fixture must fetch+parse");

    // Envelope fidelity: the fields RexOps reads, we read identically.
    assert_eq!(raw.schema_version, Some(1));
    assert_eq!(raw.source_tool, "bulwark");
    assert_eq!(raw.generated_at, "2026-06-04");
    assert_eq!(raw.items.len(), 4);

    // Normalize and assert the EXACT canonical set, in order.
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.generated_at, "2026-06-04");
    let findings = inventory.findings;
    assert_eq!(findings.len(), 4);

    // --- Finding 1: deploy-prod.sh --- location {"type":"line","line":42} → "line 42"
    assert_eq!(
        findings[0],
        Finding {
            id: FindingId("deploy-prod.sh".to_string()),
            name: Some("deploy-prod.sh".to_string()),
            rule_id: Some("aws-key".to_string()),
            description: Some("AWS access key ID detected".to_string()),
            severity: Severity::Critical,
            raw_severity: Some("critical".to_string()),
            category: Some("secret_leakage".to_string()),
            location: "line 42".to_string(),
            path: Some("/home/tom/bin/deploy-prod.sh".to_string()),
            risk: Some("critical".to_string()),
            owner: Some("tom".to_string()),
        }
    );

    // --- Finding 2: backup-db.sh --- byte_range 120..168 → "bytes 120..168"
    assert_eq!(
        findings[1],
        Finding {
            id: FindingId("backup-db.sh".to_string()),
            name: Some("backup-db.sh".to_string()),
            rule_id: Some("broad-chmod".to_string()),
            description: Some("World-writable permission change".to_string()),
            severity: Severity::Medium,
            raw_severity: Some("medium".to_string()),
            category: Some("sensitive_data".to_string()),
            location: "bytes 120..168".to_string(),
            // This fixture item carries no path/risk/owner keys -> all None.
            path: None,
            risk: None,
            owner: None,
        }
    );

    // --- Finding 3: cleanup-logs.py --- json_path → the path string
    assert_eq!(
        findings[2],
        Finding {
            id: FindingId("cleanup-logs.py".to_string()),
            name: Some("cleanup-logs.py".to_string()),
            rule_id: Some("email-address".to_string()),
            description: Some("Email address found".to_string()),
            severity: Severity::Low,
            raw_severity: Some("low".to_string()),
            category: Some("pii".to_string()),
            location: "$.contacts[0].email".to_string(),
            path: None,
            risk: None,
            owner: None,
        }
    );

    // --- Finding 4: healthcheck.sh --- {"type":"unknown"} → "unknown"
    assert_eq!(
        findings[3],
        Finding {
            id: FindingId("healthcheck.sh".to_string()),
            name: Some("healthcheck.sh".to_string()),
            rule_id: Some("curl-pipe-sh".to_string()),
            description: Some("curl piped to shell".to_string()),
            severity: Severity::High,
            raw_severity: Some("high".to_string()),
            category: Some("corp-policy-x".to_string()),
            location: "unknown".to_string(),
            path: None,
            risk: None,
            owner: None,
        }
    );
}

// =============================================================================
// 2. Unknown top-level field is ignored (forward-compat leniency)
// =============================================================================
/// An unknown top-level field must not fail the parse — we do NOT set
/// `deny_unknown_fields`. Driven through `fetch` against a temp file.
#[test]
fn unknown_toplevel_field_is_ignored() {
    let path = write_temp(
        "unknown_top",
        r#"{"_future": true, "schema_version": 1, "items": []}"#,
    );
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("unknown top-level key must not fail parse");
    assert_eq!(raw.schema_version, Some(1));
    assert!(raw.items.is_empty());
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 3. Raw boundary: missing schema_version still parses
// =============================================================================
/// A feed that omits `schema_version` must parse (to `None`) and normalize —
/// never hard-fail at the adapter boundary. The compiler rejects it later.
#[test]
fn missing_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "missing_ver",
        r#"{"source_tool": "bulwark", "items": [{"id": "x.sh"}]}"#,
    );
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed
        .fetch()
        .expect("missing schema_version must NOT fail the parse");
    assert_eq!(raw.schema_version, None);
    let inventory = feed.normalize(raw);
    let findings = inventory.findings;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id.0, "x.sh");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 4. Raw boundary: unknown schema_version still parses
// =============================================================================
/// A feed with an unrecognized version (99) parses and normalizes here; the
/// compiler rejects it later before it reaches the snapshot payload.
#[test]
fn unknown_schema_version_parses_at_raw_boundary() {
    let path = write_temp(
        "unknown_ver",
        r#"{"schema_version": 99, "items": [{"id": "y.sh"}]}"#,
    );
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("unknown version must parse");
    assert_eq!(raw.schema_version, Some(99));
    let inventory = feed.normalize(raw);
    let findings = inventory.findings;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id.0, "y.sh");
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 5. SKIP-AND-DROP — id→name fallback + subject-less items dropped
// =============================================================================
/// DIVERGENCE 1 in action: items with both `id` and `name` absent (or blank) are
/// dropped. Items with only a `name` survive using the name as their subject.
/// Unknown per-item fields are tolerated by the parse but NOT carried into the
/// canonical `Finding` (the allowlist — see the secret-safety note in `normalize`).
/// The drop count is also asserted: a subject-less drop must be accounted, not silent.
#[test]
fn subjectless_records_dropped_name_fallback_works() {
    let text = r#"{
        "schema_version": 1,
        "items": [
            {"id": "keep-1.sh"},
            {"name": "keep-2.sh"},
            {"description": "no subject at all"},
            {"id": "", "severity": "low"},
            {"id": "   ", "name": ""},
            {"id": "keep-3.sh", "future_field": 42}
        ]
    }"#;
    let path = write_temp("skip_drop", text);
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());

    let raw = feed
        .fetch()
        .expect("feed must parse despite subject-less records");
    // All SIX records parse at the wire boundary...
    assert_eq!(raw.items.len(), 6);
    // ...but normalization keeps only the three with a usable subject.
    let inventory = feed.normalize(raw);
    // Three subject-less records were dropped; the count must be recorded.
    assert_eq!(inventory.dropped_records, 3);
    let findings = inventory.findings;
    let subjects: Vec<&str> = findings.iter().map(|f| f.id.0.as_str()).collect();
    assert_eq!(subjects, vec!["keep-1.sh", "keep-2.sh", "keep-3.sh"]);
    assert_eq!(findings.len(), 3);
    // The unmodeled `future_field` was tolerated by the parse but is NOT carried
    // onto the canonical Finding — the allowlist drops every unmodeled key, so a
    // future field carrying matched content cannot leak into the snapshot.
    let serialized = serde_json::to_string(&findings[2]).expect("finding serializes");
    assert!(
        !serialized.contains("future_field"),
        "unmodeled per-item fields must not reach the snapshot: {serialized}"
    );
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 6. ABSENT FIELDS THROUGH `normalize` — Option pass-through + Unrated/Unknown
// =============================================================================
/// End-to-end proof of the modeling: an item carrying ONLY a subject id normalizes
/// to a `Finding` whose optional strings are `None` (absence PRESERVED) and whose
/// severity is `Unrated` (no risk signal). A sibling with a present-but-unrecognized
/// severity gets `Unknown`, confirming the two stay distinct through the public path.
#[test]
fn absent_optional_fields_and_severity_normalize_to_none_and_unrated() {
    let text = r#"{
        "schema_version": 1,
        "items": [
            {"id": "bare.sh"},
            {"id": "weird.sh", "severity": "spicy"}
        ]
    }"#;
    let path = write_temp("absent_fields", text);
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());
    let raw = feed.fetch().expect("must parse");
    let inventory = feed.normalize(raw);
    let findings = inventory.findings;
    assert_eq!(findings.len(), 2);

    // Item 1: nothing but an id → all optionals None, severity Unrated, location "unknown".
    assert_eq!(
        findings[0],
        Finding {
            id: FindingId("bare.sh".to_string()),
            name: None,
            rule_id: None,
            description: None,
            severity: Severity::Unrated, // absent severity → Unrated (no signal)
            raw_severity: None,
            category: None,
            location: "unknown".to_string(), // no location value → "unknown"
            path: None,
            risk: None,
            owner: None,
        }
    );

    // Item 2: a present-but-unrecognized severity → Unknown (NOT Unrated).
    assert_eq!(findings[1].severity, Severity::Unknown);
    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// 7. END-TO-END (success path): the REAL `fetch()` against the on-disk fixture
// =============================================================================
/// Confirms the on-disk file and the fixture used in the parity test agree, and that
/// fetch+normalize compose into the expected finding set via the real read path.
#[test]
fn fetch_reads_fixture_from_disk_and_normalizes() {
    let feed = BulwarkFeed::from_path(fixture_path());
    let raw = feed.fetch().expect("fixture must fetch+parse from disk");
    assert_eq!(raw.schema_version, Some(1));
    let inventory = feed.normalize(raw);
    assert_eq!(inventory.generated_at, "2026-06-04");
    let findings = inventory.findings;
    let subjects: Vec<&str> = findings.iter().map(|f| f.id.0.as_str()).collect();
    assert_eq!(
        subjects,
        vec![
            "deploy-prod.sh",
            "backup-db.sh",
            "cleanup-logs.py",
            "healthcheck.sh",
        ]
    );
}

// =============================================================================
// 8. MISSING FILE → `FeedError::NotFound`
// =============================================================================
/// Missing file must produce `NotFound` (→ Missing section), NOT `Io`
/// (→ Failed section). The whole graceful-degradation story hinges on this.
#[test]
fn missing_file_yields_not_found() {
    let feed = BulwarkFeed::from_path("/no/such/workstate/bulwark/missing-xyz123.json".to_string());
    let err = feed
        .fetch()
        .expect_err("a missing feed file must be an error");
    assert!(
        matches!(err, FeedError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

// =============================================================================
// 9. INVALID JSON → `FeedError::Parse`
// =============================================================================
/// Malformed input is a real, surfaced failure — never a silent empty feed and
/// never a crash. Driven through `fetch` against a temp file of garbage JSON.
#[test]
fn invalid_json_yields_parse_error() {
    let path = write_temp("bad_json", "{not valid json");
    let feed = BulwarkFeed::from_path(path.to_string_lossy().into_owned());
    let err = feed.fetch().expect_err("malformed JSON must be an error");
    assert!(
        matches!(err, FeedError::Parse(_)),
        "expected Parse, got {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}
