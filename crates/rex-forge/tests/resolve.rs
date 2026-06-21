use rex_forge::error::ResolveError;
use rex_forge::registry;
use rex_forge::resolve::{resolve, ResolvePlan};

#[test]
fn resolves_simple_request() {
    let reg = registry::load();
    let plan = resolve(&reg, "rust-bin", &["clap".into()]).unwrap();
    assert_eq!(
        plan,
        ResolvePlan {
            base: "rust-bin".into(),
            components: vec!["clap".into()]
        }
    );
}

#[test]
fn unknown_component_errors() {
    let reg = registry::load();
    let err = resolve(&reg, "rust-bin", &["nope".into()]).unwrap_err();
    assert!(matches!(err, ResolveError::UnknownComponent(c) if c == "nope"));
}

#[test]
fn component_not_applicable_to_base_errors() {
    let reg = registry::load();
    let err = resolve(&reg, "go-bin", &["clap".into()]).unwrap_err();
    assert!(matches!(err, ResolveError::BaseMismatch { component, .. } if component == "clap"));
}

#[test]
fn empty_request_is_ok() {
    let reg = registry::load();
    let plan = resolve(&reg, "rust-bin", &[]).unwrap();
    assert!(plan.components.is_empty());
}

#[test]
fn base_mismatch_is_checked_before_anything_else() {
    // thiserror applies to rust-lib only; requesting it on rust-bin -> BaseMismatch.
    let reg = registry::load();
    let err = resolve(&reg, "rust-bin", &["thiserror".into()]).unwrap_err();
    assert!(
        matches!(err, ResolveError::BaseMismatch { component, .. } if component == "thiserror")
    );
}

#[test]
fn same_base_conflicting_pair_errors_with_sorted_names() {
    // On rust-lib both anyhow and thiserror apply, and they conflict.
    let reg = registry::load();
    let err = resolve(&reg, "rust-lib", &["anyhow".into(), "thiserror".into()]).unwrap_err();
    match err {
        ResolveError::Conflict { a, b } => {
            assert_eq!(a, "anyhow");
            assert_eq!(b, "thiserror");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
}
