use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

#[test]
fn workstate_feed_json_matches_versioned_fixture() {
    let manifest_dir = workspace_root().join("config");
    let fixture_path = workspace_root()
        .join("fixtures")
        .join("workstate_feed_v1.json");

    let output = Command::new(env!("CARGO_BIN_EXE_toolfoundry"))
        .arg("workstate-feed")
        .arg(manifest_dir)
        .arg("--as-of")
        .arg("2026-06-02")
        .arg("--generated-at")
        .arg("2026-06-02T00:00:00Z")
        .output()
        .expect("workstate-feed command should run");

    assert!(
        output.status.success(),
        "workstate-feed should exit zero even when attention is present"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    let actual = serde_json::from_str::<Value>(&stdout).expect("stdout should be valid JSON");

    let expected = fs::read_to_string(fixture_path).expect("fixture should be readable");
    let expected = serde_json::from_str::<Value>(&expected).expect("fixture should be valid JSON");

    assert_eq!(actual, expected);
}

#[test]
fn output_flag_writes_feed_atomically_without_leftover_tmp() {
    let manifest_dir = workspace_root().join("config");
    let out_dir = unique_temp_dir();
    // Point at a nested path that does not exist yet, to also exercise the
    // parent-directory creation in the atomic writer.
    let output_path = out_dir.join("nested").join("feed.json");

    let status = Command::new(env!("CARGO_BIN_EXE_toolfoundry"))
        .arg("workstate-feed")
        .arg(&manifest_dir)
        .arg("--as-of")
        .arg("2026-06-02")
        .arg("--generated-at")
        .arg("2026-06-02T00:00:00Z")
        .arg("--output")
        .arg(&output_path)
        .status()
        .expect("workstate-feed --output should run");

    assert!(status.success(), "workstate-feed --output should exit zero");

    // The final file must exist and contain the same contract we serve on stdout.
    let written = fs::read_to_string(&output_path).expect("output file should exist");
    let value = serde_json::from_str::<Value>(&written).expect("output should be valid JSON");
    assert_eq!(value["source_tool"], "toolfoundry");
    assert_eq!(value["schema_version"], 1);

    // No temporary file may survive a successful publish: the writer renames its
    // ".<name>.<pid>.tmp" sidecar into place, so the directory should contain
    // exactly the final file and nothing matching *.tmp.
    let leftovers: Vec<String> = fs::read_dir(output_path.parent().expect("output has parent"))
        .expect("output directory should be readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no .tmp files should remain, found: {leftovers:?}"
    );

    let _ = fs::remove_dir_all(&out_dir);
}

fn unique_temp_dir() -> PathBuf {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("toolfoundry-output-{nonce}-{id}"));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
