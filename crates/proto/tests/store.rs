mod common;

use common::TempDir;
use proto::ProtoError;
use proto::core::session::{Session, StepStatus};
use proto::core::store;
use proto::{Protocol, StepKind};

// Build a small completed-ish session to save. We construct a Protocol in code
// (not from YAML) to keep the test focused on the store, not the loader.
fn make_session(protocol_id: &str) -> Session {
    // A 2-step protocol via serde so we don't hand-build nested structs.
    let yaml = format!(
        "id: {protocol_id}\ntitle: Store Test\nsteps:\n  - id: a\n    title: A\n  - id: b\n    title: B\n    kind: info\n"
    );
    let protocol: Protocol = serde_yaml::from_str(&yaml).unwrap();
    // Sanity: the second step is the info kind we'll acknowledge.
    assert_eq!(protocol.steps[1].kind, StepKind::Info);

    let mut session = Session::new(&protocol);
    session.steps[0].status = StepStatus::Passed;
    session.steps[1].status = StepStatus::Acknowledged;
    session.steps[0].note = "looked good".to_string();
    session
}

#[test]
fn save_then_load_round_trips() {
    let dir = TempDir::new("store_rt");
    let session = make_session("round-trip");

    let path = store::save(dir.path(), &session).expect("save should succeed");
    assert!(path.exists(), "save must create the file");

    // The id is the filename stem; loading it back yields an equal session.
    let id = store::session_id(&session);
    let loaded = store::load(dir.path(), &id).expect("load should find it");

    assert_eq!(loaded.protocol_id, session.protocol_id);
    assert_eq!(loaded.steps.len(), session.steps.len());
    assert_eq!(loaded.steps[0].status, StepStatus::Passed);
    assert_eq!(loaded.steps[0].note, "looked good");
    assert_eq!(loaded.steps[1].status, StepStatus::Acknowledged);
}

#[test]
fn list_is_empty_for_a_missing_store() {
    // A directory that doesn't exist yet is "no sessions", NOT an error — first
    // run friendliness and the suite's graceful-degradation rule.
    let missing = std::path::Path::new("/proto-store-does-not-exist-xyz");
    let entries = store::list(missing).expect("missing store should be empty, not error");
    assert!(entries.is_empty());
}

#[test]
fn list_returns_newest_first() {
    let dir = TempDir::new("store_order");

    // Two runs of the SAME protocol at different times. Ids are
    // `<protocol_id>-<timestamp>`, so for a shared protocol_id the timestamp
    // drives the order — newest first. (Across DIFFERENT protocols, ids sort by
    // protocol name first; that grouping is intentional, so this test pins the
    // within-protocol chronological case it actually guarantees.)
    let mut older = make_session("deploy");
    let mut newer = make_session("deploy");
    older.generated_at = "2026-06-01T00:00:00Z".parse().unwrap();
    newer.generated_at = "2026-06-02T00:00:00Z".parse().unwrap();

    store::save(dir.path(), &older).unwrap();
    store::save(dir.path(), &newer).unwrap();

    let entries = store::list(dir.path()).unwrap();
    assert_eq!(entries.len(), 2);
    // Newest first: the 2026-06-02 run leads.
    assert!(
        entries[0].id.ends_with("20260602T000000Z"),
        "newest run should be first: {}",
        entries[0].id
    );
    assert!(
        entries[0].session.generated_at > entries[1].session.generated_at,
        "list must be newest-first within a protocol"
    );
}

#[test]
fn load_unknown_id_is_not_found() {
    let dir = TempDir::new("store_miss");
    store::save(dir.path(), &make_session("present")).unwrap();

    match store::load(dir.path(), "nope-not-here") {
        Err(ProtoError::NotFound { id }) => assert_eq!(id, "nope-not-here"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn save_creates_missing_parent_dirs() {
    // Saving into a nested, not-yet-created path should just work (create_dir_all),
    // so a brand-new ~/.proto/sessions doesn't need pre-creation.
    let base = TempDir::new("store_mkdir");
    let nested = base.path().join("deep").join("sessions");
    let session = make_session("nested");

    let path = store::save(&nested, &session).expect("save should create parents");
    assert!(path.exists());
}

#[test]
fn delete_removes_a_session_and_reports_missing() {
    let dir = TempDir::new("store_delete");
    let session = make_session("to-delete");
    store::save(dir.path(), &session).unwrap();
    let id = store::session_id(&session);

    // Deleting an existing session succeeds and the file is gone.
    store::delete(dir.path(), &id).expect("delete should succeed");
    assert!(
        store::load(dir.path(), &id).is_err(),
        "session should be gone"
    );

    // Deleting a missing id is NotFound (same vocabulary as load).
    match store::delete(dir.path(), "never-existed") {
        Err(ProtoError::NotFound { id }) => assert_eq!(id, "never-existed"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn latest_run_by_protocol_picks_the_newest_per_id() {
    let dir = TempDir::new("store_latest");

    // Two runs of "deploy" and one of "review". For "deploy" the picker should
    // report the NEWER started_at. We set started_at AND generated_at (the id is
    // derived from generated_at, so they must differ to be distinct files).
    let mut deploy_old = make_session("deploy");
    deploy_old.started_at = "2026-06-01T09:00:00Z".parse().unwrap();
    deploy_old.generated_at = "2026-06-01T09:00:00Z".parse().unwrap();
    let mut deploy_new = make_session("deploy");
    deploy_new.started_at = "2026-06-05T09:00:00Z".parse().unwrap();
    deploy_new.generated_at = "2026-06-05T09:00:00Z".parse().unwrap();
    let mut review = make_session("review");
    review.started_at = "2026-06-03T09:00:00Z".parse().unwrap();
    review.generated_at = "2026-06-03T09:00:00Z".parse().unwrap();

    store::save(dir.path(), &deploy_old).unwrap();
    store::save(dir.path(), &deploy_new).unwrap();
    store::save(dir.path(), &review).unwrap();

    let latest = store::latest_run_by_protocol(dir.path()).unwrap();
    assert_eq!(latest.len(), 2, "two distinct protocols");
    assert_eq!(
        latest["deploy"], deploy_new.started_at,
        "newest deploy wins"
    );
    assert_eq!(latest["review"], review.started_at);
}

#[test]
fn build_and_save_feed_round_trips() {
    let dir = TempDir::new("store_feed");
    let feed_dir = TempDir::new("store_feed_out");
    store::save(dir.path(), &make_session("alpha")).unwrap();
    store::save(dir.path(), &make_session("beta")).unwrap();

    // Build a feed from the saved sessions and write it.
    let feed = store::build_feed(dir.path()).unwrap();
    assert_eq!(feed.item_count, feed.items.len());
    assert_eq!(feed.item_count, 2);

    let path = store::save_feed(feed_dir.path(), &feed).unwrap();
    assert!(path.ends_with("proto.json"), "feed file is proto.json");

    // It reads back as a valid feed with the same count.
    let text = std::fs::read_to_string(&path).unwrap();
    let again: proto::WorkstateFeed = serde_json::from_str(&text).unwrap();
    assert_eq!(again.item_count, 2);
    assert_eq!(again.source_tool, "proto");
}
