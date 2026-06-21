//! `pulse status` end-to-end: a real subprocess, an isolated empty data dir, so
//! the contract (one JSON line + exit code) is verified exactly as RexOps will
//! invoke it.

use std::process::Command;

fn run_status(data_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pulse"))
        .arg("status")
        .env("PULSE_DATA_DIR", data_dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run pulse status")
}

#[test]
fn status_prints_one_json_line_with_the_contract_fields() {
    let tmp = std::env::temp_dir().join(format!("pulse-status-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let out = run_status(&tmp);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // exactly one line
    assert_eq!(stdout.lines().count(), 1, "stdout was:\n{stdout}");
    // valid JSON carrying the three contract fields
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON line");
    assert!(v.get("healthy").and_then(|x| x.as_bool()).is_some());
    assert!(v.get("detail").and_then(|x| x.as_str()).is_some());
    assert!(v.get("latency_ms").and_then(|x| x.as_u64()).is_some());

    // an empty data dir → not healthy → exit 1
    assert_eq!(out.status.code(), Some(1), "stdout:\n{stdout}");

    let _ = std::fs::remove_dir_all(&tmp);
}
