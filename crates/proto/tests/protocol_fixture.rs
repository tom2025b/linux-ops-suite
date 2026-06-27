use std::path::PathBuf;

use proto::StepKind;
use proto::core::loader;

// Absolute path to the shipped example, anchored on the crate root so the test
// works regardless of the directory `cargo test` is invoked from.
fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("protocols/rust-repo-review.yaml")
}

#[test]
fn shipped_example_loads_and_validates() {
    let path = example_path();
    let protocol = loader::load_file(&path).expect("shipped example must parse");
    loader::validate(&protocol).expect("shipped example must validate");

    // Documented identity.
    assert_eq!(protocol.id, "rust-repo-review");
    assert!(!protocol.title.trim().is_empty());

    // It should be a substantial checklist.
    assert!(
        protocol.step_count() >= 5,
        "example should be a real checklist, got {} steps",
        protocol.step_count()
    );
}

#[test]
fn shipped_example_has_unique_step_ids() {
    // Validation already enforces this, but assert it directly on the real file
    // so a duplicate introduced by an edit is pinpointed to THIS artifact.
    let protocol = loader::load_file(&example_path()).expect("parse");
    let mut ids: Vec<&str> = protocol.steps.iter().map(|s| s.id.as_str()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "step ids in the example must be unique");
}

#[test]
fn shipped_example_exercises_every_step_kind() {
    // The example is also our DOCUMENTATION of the format, so it should
    // demonstrate all three kinds (manual_check, info, command). If a refactor
    // ever changes the kinds, updating this test is a deliberate, visible choice.
    let protocol = loader::load_file(&example_path()).expect("parse");

    let kinds: Vec<StepKind> = protocol.steps.iter().map(|s| s.kind).collect();
    assert!(
        kinds.contains(&StepKind::ManualCheck),
        "example should include a manual_check step"
    );
    assert!(
        kinds.contains(&StepKind::Info),
        "example should include an info step"
    );
    assert!(
        kinds.contains(&StepKind::Command),
        "example should include a command step"
    );
}
