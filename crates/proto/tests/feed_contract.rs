use std::path::PathBuf;

use proto::WorkstateFeed;
use proto::core::session::{Session, StepStatus};

// Resolve a path under tests/fixtures/, anchored on the crate root.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// Load the example feed JSON as raw text.
fn example_text() -> String {
    std::fs::read_to_string(fixture("proto.workstate-feed.example.json"))
        .expect("example feed fixture must exist")
}

// =============================================================================
// 1. PRODUCER MATCHES CONTRACT — Proto's WorkstateFeed reads the example.
// =============================================================================

#[test]
fn proto_feed_deserializes_the_contract_example() {
    let text = example_text();
    // If this parses into Proto's own WorkstateFeed, the contract example and
    // Proto's model agree — the producer speaks exactly this shape.
    let feed: WorkstateFeed =
        serde_json::from_str(&text).expect("example must deserialize into proto::WorkstateFeed");

    assert_eq!(feed.schema_version, 1);
    assert_eq!(feed.source_tool, "proto");
    assert_eq!(feed.item_count, feed.items.len(), "count must equal length");
    assert!(!feed.items.is_empty(), "example should have feed items");

    // The example is meant to exercise both completion states; confirm they're
    // present so the fixture stays representative.
    let statuses: Vec<&str> = feed.items.iter().map(|i| i.status.as_str()).collect();
    assert!(statuses.contains(&"complete"));
    assert!(statuses.contains(&"incomplete"));
}

#[test]
fn example_round_trips_through_proto_feed() {
    // Deserialize then re-serialize via Proto's model and re-read: proves Proto
    // both consumes and PRODUCES this exact contract shape.
    let feed: WorkstateFeed = serde_json::from_str(&example_text()).unwrap();
    let reserialized = serde_json::to_string(&feed).unwrap();
    let again: WorkstateFeed = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(again.items.len(), feed.items.len());
    assert_eq!(again.item_count, feed.item_count);
}

// =============================================================================
// 2. CONTRACT INVARIANTS — the example obeys the schema's rules (hand-checked).
// =============================================================================

#[test]
fn example_satisfies_schema_required_fields_and_consts() {
    let value: serde_json::Value = serde_json::from_str(&example_text()).unwrap();

    // Top-level required fields from the feed schema.
    for key in [
        "schema_version",
        "source_tool",
        "generated_at",
        "item_count",
        "items",
    ] {
        assert!(
            value.get(key).is_some(),
            "required field '{key}' missing from example"
        );
    }

    // const constraints.
    assert_eq!(value["schema_version"], serde_json::json!(1));
    assert!(
        value["schema_version"].is_u64(),
        "schema_version must be an integer"
    );
    assert_eq!(value["source_tool"], serde_json::json!("proto"));

    // Each item carries the required summary fields, and status is in the enum.
    let allowed_status = ["complete", "incomplete"];
    let items = value["items"].as_array().expect("items must be an array");
    for item in items {
        for key in [
            "id",
            "protocol_id",
            "protocol_title",
            "started_at",
            "status",
            "passed",
            "failed",
            "skipped",
            "summary",
        ] {
            assert!(item.get(key).is_some(), "item missing required '{key}'");
        }
        let status = item["status"].as_str().expect("status must be a string");
        assert!(
            allowed_status.contains(&status),
            "status '{status}' is not in the contract enum"
        );
    }
}

// =============================================================================
// 3. PRODUCER INVARIANT — item_count == items.length, in-process from sessions.
// =============================================================================

#[test]
fn built_feed_count_matches_items() {
    // Build a couple of sessions IN MEMORY (no disk), summarize them into a feed
    // via Proto's own model, and assert the producer invariant holds. This is the
    // count==length guarantee JSON Schema can't express — the heart of the contract.
    let session = sample_session("rust-repo-review", "Rust Repository Review");
    let session2 = sample_session("release-readiness", "Release Readiness");

    let feed = WorkstateFeed::from_sessions(
        [
            ("rust-repo-review-20260606T084438Z", &session),
            ("release-readiness-20260605T173012Z", &session2),
        ],
        50,
    );

    assert_eq!(feed.item_count, 2);
    assert_eq!(feed.item_count, feed.items.len());
    assert_eq!(feed.source_tool, "proto");
    assert_eq!(feed.schema_version, 1);
    // Newest-first order is the caller's; the feed preserves it. The first item's
    // counts should reflect the session it was built from (all acknowledged here).
    assert_eq!(feed.items[0].protocol_id, "rust-repo-review");
}

#[test]
fn built_feed_respects_the_cap() {
    // With a cap of 1, only the first (newest) session survives — the rolling
    // window that keeps the feed file bounded no matter how many sessions exist.
    let a = sample_session("a", "A");
    let b = sample_session("b", "B");
    let feed = WorkstateFeed::from_sessions([("a-1", &a), ("b-1", &b)], 1);
    assert_eq!(feed.item_count, 1);
    assert_eq!(feed.items[0].protocol_id, "a");
}

// Build a tiny complete session for a protocol id/title with one acknowledged
// step, so `from_sessions` has something realistic to summarize. We construct it
// through the public model the same way `run` does.
fn sample_session(protocol_id: &str, title: &str) -> Session {
    // A single-step protocol, run to an acknowledged outcome.
    let protocol = proto::Protocol {
        id: protocol_id.to_string(),
        title: title.to_string(),
        description: String::new(),
        version: String::new(),
        steps: vec![proto::Step {
            id: "only".to_string(),
            title: "the one step".to_string(),
            detail: String::new(),
            kind: proto::StepKind::Info,
            command: None,
        }],
    };
    let mut session = Session::new(&protocol);
    session.steps[0].status = StepStatus::Acknowledged;
    session.finished_at = Some(session.started_at);
    session
}
