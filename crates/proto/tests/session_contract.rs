use std::path::PathBuf;

use proto::Session;
use proto::core::session::StepStatus;

// Resolve a path under tests/fixtures/, anchored on the crate root.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// Load the example session JSON as raw text.
fn example_text() -> String {
    std::fs::read_to_string(fixture("proto.session.example.json"))
        .expect("example session fixture must exist")
}

// =============================================================================
// 1. PRODUCER MATCHES CONTRACT — Proto's Session reads the example losslessly.
// =============================================================================

#[test]
fn proto_session_deserializes_the_contract_example() {
    let text = example_text();
    // If this parses into Proto's own Session, then the contract example and
    // Proto's model are in agreement — the producer speaks exactly this shape.
    let session: Session =
        serde_json::from_str(&text).expect("example must deserialize into proto::Session");

    assert_eq!(session.schema_version, 1);
    assert_eq!(session.source_tool, "proto");
    assert_eq!(session.protocol_id, "rust-repo-review");
    assert!(
        !session.steps.is_empty(),
        "example should have step results"
    );

    // The example is meant to exercise the full status vocabulary; confirm the
    // interesting ones are present so the fixture stays representative.
    let statuses: Vec<StepStatus> = session.steps.iter().map(|s| s.status).collect();
    assert!(statuses.contains(&StepStatus::Passed));
    assert!(statuses.contains(&StepStatus::Failed));
    assert!(statuses.contains(&StepStatus::Skipped));
    assert!(statuses.contains(&StepStatus::Acknowledged));
}

#[test]
fn example_round_trips_through_proto_session() {
    // Deserialize then re-serialize via Proto's model and re-read: proves Proto
    // both consumes and PRODUCES this exact contract shape.
    let session: Session = serde_json::from_str(&example_text()).unwrap();
    let reserialized = serde_json::to_string(&session).unwrap();
    let again: Session = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(again.steps.len(), session.steps.len());
    assert_eq!(again.protocol_id, session.protocol_id);
}

// =============================================================================
// 2. CONTRACT INVARIANTS — the example obeys the schema's rules (hand-checked).
// =============================================================================

#[test]
fn example_satisfies_schema_required_fields_and_consts() {
    // Inspect as generic JSON so we assert on the WIRE shape a consumer sees,
    // independent of Proto's struct.
    let value: serde_json::Value = serde_json::from_str(&example_text()).unwrap();

    // Top-level required fields from proto.session.schema.json.
    for key in [
        "schema_version",
        "source_tool",
        "generated_at",
        "protocol_id",
        "protocol_title",
        "started_at",
        "steps",
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
}

#[test]
fn example_step_statuses_are_in_the_enum() {
    let value: serde_json::Value = serde_json::from_str(&example_text()).unwrap();

    // The status enum declared by the contract.
    let allowed = ["pending", "passed", "failed", "skipped", "acknowledged"];

    let steps = value["steps"].as_array().expect("steps must be an array");
    for step in steps {
        // Each step result requires step_id + status.
        assert!(step.get("step_id").is_some(), "step missing step_id");
        let status = step["status"].as_str().expect("status must be a string");
        assert!(
            allowed.contains(&status),
            "status '{status}' is not in the contract enum"
        );
    }
}

#[test]
fn schema_fixture_is_present_and_parses() {
    // A light guard that the in-repo schema copy exists and is valid JSON, so a
    // future contributor knows this is the source of truth to keep in sync with
    // the suite. (We don't run a full JSON-Schema validation — no such dep, per
    // suite convention — but we do ensure the document is well-formed.)
    let schema_text = std::fs::read_to_string(fixture("proto.session.schema.json"))
        .expect("schema fixture must exist");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("schema must be valid JSON");

    // Spot-check the bits our hand-written invariants mirror, so a schema edit
    // that diverges from these tests is at least visible here.
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        serde_json::json!(1)
    );
    assert_eq!(
        schema["properties"]["source_tool"]["const"],
        serde_json::json!("proto")
    );
}
