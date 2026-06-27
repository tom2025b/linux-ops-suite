mod common; // pull in tests/common/mod.rs (TempDir, MINIMAL_PROTOCOL)

use common::{MINIMAL_PROTOCOL, TempDir};
use proto::ProtoError;
use proto::core::loader;

// =============================================================================
// discover
// =============================================================================

#[test]
fn discover_finds_only_yaml_files_sorted() {
    let dir = TempDir::new("discover");
    // Two protocol files plus a non-YAML file that must be ignored.
    dir.write_file("b.yaml", MINIMAL_PROTOCOL);
    dir.write_file("a.yml", MINIMAL_PROTOCOL); // .yml spelling also counts
    dir.write_file("notes.txt", "not a protocol");

    let found = loader::discover(dir.path()).expect("discover should succeed");

    // Only the two YAML/YML files, and SORTED (a.yml before b.yaml).
    assert_eq!(found.len(), 2, "should find exactly the two yaml files");
    assert!(
        found[0].ends_with("a.yml"),
        "results must be sorted: {found:?}"
    );
    assert!(found[1].ends_with("b.yaml"));
}

#[test]
fn discover_missing_directory_is_read_dir_error() {
    // A path that doesn't exist must classify as ReadDir, so the CLI can say
    // "could not read protocols directory" rather than something generic.
    let missing = std::path::Path::new("/proto-does-not-exist-xyz");
    match loader::discover(missing) {
        Err(ProtoError::ReadDir { .. }) => {} // exactly what we expect
        other => panic!("expected ReadDir error, got {other:?}"),
    }
}

// =============================================================================
// load_file
// =============================================================================

#[test]
fn load_file_parses_a_valid_protocol() {
    let dir = TempDir::new("load_ok");
    let path = dir.write_file("sample.yaml", MINIMAL_PROTOCOL);

    let p = loader::load_file(&path).expect("valid YAML should load");
    assert_eq!(p.id, "sample");
    assert_eq!(p.title, "Sample Protocol");
    assert_eq!(p.step_count(), 2);
    // The second step set `kind: info`; the first defaulted to manual_check.
    assert_eq!(p.steps[0].kind, proto::StepKind::ManualCheck);
    assert_eq!(p.steps[1].kind, proto::StepKind::Info);
}

#[test]
fn load_file_missing_file_is_read_file_error() {
    let path = std::path::Path::new("/proto-no-such-file.yaml");
    match loader::load_file(path) {
        Err(ProtoError::ReadFile { .. }) => {}
        other => panic!("expected ReadFile error, got {other:?}"),
    }
}

#[test]
fn load_file_malformed_yaml_is_parse_error() {
    let dir = TempDir::new("load_bad");
    // `id` is present but `steps` is the wrong type (a scalar, not a list), so
    // serde_yaml fails to map it onto Vec<Step>.
    let path = dir.write_file("broken.yaml", "id: x\ntitle: X\nsteps: not-a-list\n");
    match loader::load_file(&path) {
        Err(ProtoError::ParseYaml { .. }) => {}
        other => panic!("expected ParseYaml error, got {other:?}"),
    }
}

// =============================================================================
// validate — one test per rule, all expecting ProtoError::Validation
// =============================================================================

// A tiny helper: parse YAML text into a Protocol (asserting it parses), so each
// validation test can focus on the RULE, not on file plumbing.
fn parse(yaml: &str) -> proto::Protocol {
    serde_yaml::from_str(yaml).expect("test YAML should parse into a Protocol")
}

#[test]
fn validate_accepts_a_good_protocol() {
    let p = parse(MINIMAL_PROTOCOL);
    assert!(loader::validate(&p).is_ok());
}

#[test]
fn validate_rejects_empty_id() {
    let p = parse("id: \"\"\ntitle: T\nsteps:\n  - id: a\n    title: A\n");
    assert!(matches!(
        loader::validate(&p),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn validate_rejects_empty_title() {
    let p = parse("id: x\ntitle: \"\"\nsteps:\n  - id: a\n    title: A\n");
    assert!(matches!(
        loader::validate(&p),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn validate_rejects_no_steps() {
    let p = parse("id: x\ntitle: T\nsteps: []\n");
    assert!(matches!(
        loader::validate(&p),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn validate_rejects_empty_step_title() {
    let p = parse("id: x\ntitle: T\nsteps:\n  - id: a\n    title: \"\"\n");
    assert!(matches!(
        loader::validate(&p),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn validate_rejects_duplicate_step_ids() {
    let p =
        parse("id: x\ntitle: T\nsteps:\n  - id: dup\n    title: A\n  - id: dup\n    title: B\n");
    match loader::validate(&p) {
        Err(ProtoError::Validation { reason, .. }) => {
            // The message should name the duplicate so the author can find it.
            assert!(
                reason.contains("dup"),
                "reason should mention the id: {reason}"
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn validate_rejects_non_slug_id() {
    // An uppercase/spaced id won't match a filename or type cleanly on the CLI.
    let p = parse("id: Not A Slug\ntitle: T\nsteps:\n  - id: a\n    title: A\n");
    match loader::validate(&p) {
        Err(ProtoError::Validation { reason, .. }) => {
            assert!(
                reason.contains("slug"),
                "reason should mention slug: {reason}"
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn validate_rejects_non_slug_step_id() {
    let p = parse("id: x\ntitle: T\nsteps:\n  - id: Bad_Step\n    title: A\n");
    assert!(matches!(
        loader::validate(&p),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn validate_rejects_blank_version() {
    // A present-but-empty `version:` is an authoring slip, not a missing key.
    let p = parse("id: x\ntitle: T\nversion: \" \"\nsteps:\n  - id: a\n    title: A\n");
    match loader::validate(&p) {
        Err(ProtoError::Validation { reason, .. }) => {
            assert!(
                reason.contains("version"),
                "reason should mention version: {reason}"
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn validate_aggregates_all_problems() {
    // Empty title AND a duplicate step id AND a non-slug step id: ONE error that
    // lists every problem, so the author fixes them all in a single pass.
    let p = parse(
        "id: x\ntitle: \"\"\nsteps:\n  - id: Dup\n    title: A\n  - id: Dup\n    title: \"\"\n",
    );
    match loader::validate(&p) {
        Err(ProtoError::Validation { reason, .. }) => {
            assert!(
                reason.contains("problems:"),
                "should be the multi-problem form: {reason}"
            );
            assert!(
                reason.contains("title"),
                "should mention the empty title: {reason}"
            );
            assert!(
                reason.contains("Dup"),
                "should mention the duplicate id: {reason}"
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn check_rejects_id_filename_mismatch() {
    // `check` (validate + stem) must reject a file whose stem disagrees with `id`,
    // since Proto looks protocols up by id. MINIMAL_PROTOCOL's id is `sample`.
    let dir = TempDir::new("stem");
    let path = dir.write_file("wrong-name.yaml", MINIMAL_PROTOCOL);
    let p = loader::load_file(&path).expect("parses fine");
    match loader::check(&path, &p) {
        Err(ProtoError::Validation { reason, .. }) => {
            assert!(
                reason.contains("filename"),
                "should explain the mismatch: {reason}"
            );
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn check_accepts_matching_filename() {
    let dir = TempDir::new("stem_ok");
    let path = dir.write_file("sample.yaml", MINIMAL_PROTOCOL);
    let p = loader::load_file(&path).expect("parses fine");
    assert!(loader::check(&path, &p).is_ok());
}

#[test]
fn check_reports_stem_mismatch_even_with_a_content_problem() {
    // Regression: when the id is a valid slug that disagrees with the filename,
    // the stem-mismatch must be REPORTED, not masked by an unrelated content
    // error (a blank step title here). `check` runs the stem check first for a
    // usable id, so the filename problem is the one surfaced.
    let dir = TempDir::new("stem_mask");
    let path = dir.write_file(
        "wrong-name.yaml",
        "id: real-id\ntitle: Has A Title\nsteps:\n  - id: s1\n    title: \"\"\n",
    );
    let p = loader::load_file(&path).expect("parses fine");
    match loader::check(&path, &p) {
        Err(ProtoError::Validation { reason, .. }) => assert!(
            reason.contains("filename"),
            "stem mismatch must not be masked by the content error: {reason}"
        ),
        other => panic!("expected Validation error naming the filename, got {other:?}"),
    }
}

// =============================================================================
// load_all & find
// =============================================================================

#[test]
fn load_all_returns_every_valid_protocol() {
    let dir = TempDir::new("load_all");
    // Filenames must match the protocol `id` (the stem rule), so MINIMAL_PROTOCOL
    // (id `sample`) goes in sample.yaml, and the second's id matches its file too.
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);
    dir.write_file(
        "second.yaml",
        "id: second\ntitle: Second\nsteps:\n  - id: s\n    title: S\n",
    );

    let all = loader::load_all(dir.path()).expect("both should load");
    assert_eq!(all.len(), 2);
}

#[test]
fn load_all_short_circuits_on_an_invalid_protocol() {
    let dir = TempDir::new("load_all_bad");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);
    // Empty-steps protocol: parses fine, fails validation. Filename matches its id.
    dir.write_file("bad.yaml", "id: bad\ntitle: Bad\nsteps: []\n");

    // load_all validates, so it must surface the validation failure rather than
    // silently returning only the good one.
    assert!(matches!(
        loader::load_all(dir.path()),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn find_returns_the_matching_protocol() {
    let dir = TempDir::new("find_ok");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);

    let p = loader::find(dir.path(), "sample").expect("should find by id");
    assert_eq!(p.id, "sample");
}

#[test]
fn find_unknown_id_is_not_found() {
    let dir = TempDir::new("find_miss");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);

    match loader::find(dir.path(), "nope") {
        Err(ProtoError::NotFound { id }) => assert_eq!(id, "nope"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// =============================================================================
// find_one — validate ONLY the requested protocol, ignoring sibling files
// =============================================================================

#[test]
fn find_one_ignores_a_broken_sibling() {
    // The bug this guards: resolving a GOOD protocol must not fail just because
    // an UNRELATED file in the same directory is broken. Both `find_one` AND
    // `find` (used by `run` / the picker) judge only the file you asked about —
    // one typo in a sibling checklist must not brick `run` for every other one.
    let dir = TempDir::new("find_one_sibling");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL); // valid, id = sample
    // A second protocol that PARSES but FAILS validation (empty steps).
    dir.write_file("broken.yaml", "id: broken\ntitle: Broken\nsteps: []\n");

    // The good one validates cleanly despite the broken sibling, via find_one...
    let p = loader::find_one(dir.path(), "sample").expect("good protocol must validate");
    assert_eq!(p.id, "sample");

    // ...and equally via `find` (run's path), which now delegates to find_one so
    // a broken UNRELATED file no longer makes `proto run sample` refuse.
    let p = loader::find(dir.path(), "sample").expect("find must ignore a broken sibling");
    assert_eq!(p.id, "sample");

    // The broken file is still caught when you actually ask for IT.
    assert!(
        matches!(
            loader::find(dir.path(), "broken"),
            Err(ProtoError::Validation { .. })
        ),
        "asking for the broken protocol by id should still surface its error"
    );
}

#[test]
fn find_one_tolerates_an_unparseable_sibling() {
    // A sibling that isn't even valid YAML must also be ignored — `find_one`
    // skips files it can't parse rather than letting their error mask the target.
    let dir = TempDir::new("find_one_unparseable");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);
    dir.write_file("garbage.yaml", "id: x\ntitle: X\nsteps: not-a-list\n");

    let p = loader::find_one(dir.path(), "sample").expect("good protocol must validate");
    assert_eq!(p.id, "sample");
}

#[test]
fn find_one_reports_the_requested_protocols_own_failure() {
    // When the REQUESTED protocol is the broken one, find_one must surface ITS
    // error (not hide it), so `proto validate <bad-id>` still fails honestly.
    let dir = TempDir::new("find_one_self_bad");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);
    dir.write_file("broken.yaml", "id: broken\ntitle: Broken\nsteps: []\n");

    assert!(matches!(
        loader::find_one(dir.path(), "broken"),
        Err(ProtoError::Validation { .. })
    ));
}

#[test]
fn find_one_unknown_id_is_not_found() {
    let dir = TempDir::new("find_one_miss");
    dir.write_file("sample.yaml", MINIMAL_PROTOCOL);

    match loader::find_one(dir.path(), "nope") {
        Err(ProtoError::NotFound { id }) => assert_eq!(id, "nope"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn find_one_finds_a_protocol_whose_stem_mismatches_then_flags_it() {
    // A protocol whose `id` doesn't match its filename isn't found by the stem
    // fast-path, so find_one scans for it — and then `check` flags the mismatch.
    // (i.e. we still locate it BY id, and still report the stem rule it breaks.)
    let dir = TempDir::new("find_one_stem");
    // id is `sample` but the file is wrong-name.yaml.
    dir.write_file("wrong-name.yaml", MINIMAL_PROTOCOL);

    match loader::find_one(dir.path(), "sample") {
        Err(ProtoError::Validation { reason, .. }) => {
            assert!(
                reason.contains("filename"),
                "should flag the stem mismatch: {reason}"
            );
        }
        other => panic!("expected Validation (stem mismatch), got {other:?}"),
    }
}
