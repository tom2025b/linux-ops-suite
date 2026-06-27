mod common; // TempDir, MINIMAL_PROTOCOL

use common::MINIMAL_PROTOCOL;
use proto::core::session::Session; // the type under test
use proto::{Protocol, StepStatus};

// Parse the shared minimal protocol once per test that needs one.
fn sample_protocol() -> Protocol {
    serde_yaml::from_str(MINIMAL_PROTOCOL).expect("sample protocol must parse")
}

// =============================================================================
// Session::new — the bridge from a Protocol to a fresh run record.
// =============================================================================

#[test]
fn new_session_starts_all_pending_in_order() {
    let p = sample_protocol(); // 2 steps: "first", "second"
    let session = Session::new(&p);

    // One result per step, SAME ORDER and SAME ids as the protocol.
    assert_eq!(session.steps.len(), p.step_count());
    assert_eq!(session.steps[0].step_id, "first");
    assert_eq!(session.steps[1].step_id, "second");

    // Everything starts Pending and unanswered.
    assert!(
        session
            .steps
            .iter()
            .all(|r| r.status == StepStatus::Pending)
    );
    assert!(session.steps.iter().all(|r| r.answered_at.is_none()));
}

#[test]
fn new_session_snapshots_protocol_identity_and_header() {
    let p = sample_protocol();
    let session = Session::new(&p);

    // Identity is COPIED in so the session is self-describing.
    assert_eq!(session.protocol_id, "sample");
    assert_eq!(session.protocol_title, "Sample Protocol");

    // Contract header is populated from the start.
    assert_eq!(session.schema_version, 1);
    assert_eq!(session.source_tool, "proto");
    // A brand-new session is not finished.
    assert!(session.finished_at.is_none());
}

#[test]
fn is_complete_flips_only_when_no_step_is_pending() {
    let p = sample_protocol();
    let mut session = Session::new(&p);

    assert!(!session.is_complete(), "fresh session is not complete");

    // Answer the first step only — still incomplete.
    session.steps[0].status = StepStatus::Passed;
    assert!(!session.is_complete(), "one pending step => incomplete");

    // Answer the second (info) step — now complete.
    session.steps[1].status = StepStatus::Acknowledged;
    assert!(session.is_complete(), "no pending steps => complete");
}

// =============================================================================
// JSON contract — assert on the SERIALIZED bytes, not just the struct.
// =============================================================================

#[test]
fn session_json_leads_with_contract_header() {
    let p = sample_protocol();
    let session = Session::new(&p);

    // Serialize to a serde_json::Value so we can inspect keys/types precisely.
    let value: serde_json::Value =
        serde_json::to_value(&session).expect("session must serialize to JSON");

    // schema_version is an INTEGER (suite rule), and equals 1.
    assert_eq!(value["schema_version"], serde_json::json!(1));
    assert!(
        value["schema_version"].is_u64(),
        "schema_version must be an integer, not a string"
    );

    // source_tool + generated_at are present (provenance every export carries).
    assert_eq!(value["source_tool"], serde_json::json!("proto"));
    assert!(
        value["generated_at"].is_string(),
        "generated_at must be an RFC3339 string"
    );

    // Each step result is keyed by its step_id with a status.
    let steps = value["steps"].as_array().expect("steps must be an array");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["step_id"], serde_json::json!("first"));
    assert_eq!(steps[0]["status"], serde_json::json!("pending"));
}

#[test]
fn session_json_omits_not_yet_fields_rather_than_nulling() {
    // A fresh session: finished_at is None, and step answered_at/note are unset.
    // The contract says OMIT absent optional fields (consumers treat a missing
    // key as "absent"); writing `null` would be a different, sloppier shape.
    let p = sample_protocol();
    let session = Session::new(&p);

    let json = serde_json::to_string(&session).expect("serialize");

    assert!(
        !json.contains("finished_at"),
        "finished_at must be omitted while None, not written as null: {json}"
    );
    assert!(
        !json.contains("answered_at"),
        "answered_at must be omitted while None: {json}"
    );
    assert!(
        !json.contains("\"note\""),
        "empty note must be omitted: {json}"
    );
}

#[test]
fn session_timestamps_serialize_without_subsecond_noise() {
    // Regression: session timestamps are stamped to whole seconds so the wire
    // form matches the contract examples ("2026-06-06T12:00:00Z"), not noisy
    // nanoseconds ("...:00.193701135Z"). A fractional-seconds dot in any RFC3339
    // value would mean the truncation regressed.
    let p = sample_protocol();
    let session = Session::new(&p);
    let json = serde_json::to_string(&session).expect("serialize");

    // started_at/generated_at appear; none of them should carry a sub-second dot.
    assert!(
        json.contains("started_at"),
        "sanity: timestamps present: {json}"
    );
    for stamp in [&session.started_at, &session.generated_at] {
        let rendered = stamp.to_rfc3339();
        assert!(
            !rendered.contains('.'),
            "timestamp should have second precision, got {rendered}"
        );
    }
}

#[test]
fn session_round_trips_through_json() {
    // Serialize -> deserialize must yield an equivalent session, proving the
    // file we write is also a file we (or a consumer) can read back.
    let p = sample_protocol();
    let mut original = Session::new(&p);
    original.steps[0].status = StepStatus::Passed;

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: Session = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.schema_version, original.schema_version);
    assert_eq!(restored.protocol_id, original.protocol_id);
    assert_eq!(restored.steps.len(), original.steps.len());
    assert_eq!(restored.steps[0].status, StepStatus::Passed);
}
