use std::path::Path;
use std::process::Command;

use tempfile::tempdir; // auto-cleaning temp directory

// A minimal valid protocol, written into the scratch dir by tests that need one.
const VALID: &str = "\
id: sample
title: Sample Protocol
steps:
  - id: first
    title: Do the first thing
  - id: second
    title: Acknowledge this
    kind: info
";

// Build a `proto` Command pre-pointed at `dir` via the global --dir flag, so each
// test runs against its own isolated protocols directory.
fn proto_in(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_proto"));
    cmd.arg("--dir").arg(dir);
    cmd
}

// Tiny helpers to read captured output as owned Strings for `.contains` checks.
fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// =============================================================================
// list
// =============================================================================

#[test]
fn list_shows_a_valid_protocol() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();

    let out = proto_in(dir.path()).arg("list").output().unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("sample"), "should list the id: {stdout}");
    assert!(stdout.contains("Sample Protocol"), "should list the title");
}

#[test]
fn list_empty_dir_reports_none_and_succeeds() {
    let dir = tempdir().unwrap(); // empty
    let out = proto_in(dir.path()).arg("list").output().unwrap();

    // An empty but valid directory is success, with a friendly message — not an
    // error and not silent.
    assert!(out.status.success());
    assert!(stdout_of(&out).contains("No protocols found"));
}

// =============================================================================
// validate
// =============================================================================

#[test]
fn validate_all_succeeds_on_good_dir() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();

    let out = proto_in(dir.path()).arg("validate").output().unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("valid"));
}

#[test]
fn validate_all_fails_nonzero_when_a_protocol_is_invalid() {
    let dir = tempdir().unwrap();
    // VALID's id is `sample`, so the file must be sample.yaml (the stem rule).
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();
    // Empty-steps protocol: parses but fails validation. Filename matches its id.
    std::fs::write(
        dir.path().join("bad.yaml"),
        "id: bad\ntitle: Bad\nsteps: []\n",
    )
    .unwrap();

    let out = proto_in(dir.path()).arg("validate").output().unwrap();

    // The CLI contract: non-zero exit when anything fails (so scripts can gate).
    assert!(
        !out.status.success(),
        "validate must exit non-zero on failure"
    );
    // The per-file summary should still mark the good one and flag the bad one.
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("FAIL"),
        "should report the failing file: {stdout}"
    );
}

#[test]
fn validate_single_known_id_succeeds() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();

    let out = proto_in(dir.path())
        .arg("validate")
        .arg("sample")
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("OK"));
}

#[test]
fn validate_single_id_ignores_a_broken_sibling() {
    // Regression: `proto validate <good-id>` must succeed even when an UNRELATED
    // protocol in the same directory is broken. Previously the single-id path went
    // through `find` (which validates the whole directory) and failed here, naming
    // the wrong file.
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();
    // A broken sibling: parses but fails validation (empty steps).
    std::fs::write(
        dir.path().join("broken.yaml"),
        "id: broken\ntitle: Broken\nsteps: []\n",
    )
    .unwrap();

    let out = proto_in(dir.path())
        .arg("validate")
        .arg("sample")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "validate <good-id> must ignore a broken sibling; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("OK"),
        "should report the good protocol: {stdout}"
    );
    // It must NOT mention the unrelated broken file.
    assert!(
        !stdout.contains("broken") && !stderr_of(&out).contains("broken"),
        "validate sample must not mention the unrelated broken.yaml"
    );
}

#[test]
fn validate_single_unknown_id_fails_nonzero() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();

    let out = proto_in(dir.path())
        .arg("validate")
        .arg("nope")
        .output()
        .unwrap();

    assert!(!out.status.success(), "unknown id must fail");
    // The error message (on stderr, via main.rs) should name the missing id.
    assert!(
        stderr_of(&out).contains("nope"),
        "stderr should name the id"
    );
}

// =============================================================================
// run
// =============================================================================

#[test]
fn run_walks_steps_and_writes_a_session() {
    // `run` is interactive: it reads stdin. We feed answers and check it writes a
    // session JSON into an isolated --sessions-dir (a temp dir), so the test
    // never touches the real ~/.proto store and cleans up after itself.
    let protocols = tempdir().unwrap();
    let sessions = tempdir().unwrap();
    let feed = tempdir().unwrap(); // isolate the feed too (don't touch ~/.local)
    std::fs::write(protocols.path().join("sample.yaml"), VALID).unwrap();

    use std::io::Write;
    use std::process::Stdio;

    // VALID has 2 steps. Each step now reads TWO lines: the answer, then an
    // optional note (blank = none). So: "y", note-skip, Enter(info), note-skip.
    let mut child = Command::new(env!("CARGO_BIN_EXE_proto"))
        .arg("--dir")
        .arg(protocols.path()) // protocols come from the scratch dir
        .arg("--sessions-dir")
        .arg(sessions.path()) // sessions land in an isolated scratch dir
        .arg("--feed-dir")
        .arg(feed.path()) // feed lands in an isolated scratch dir
        .arg("run")
        .arg("sample")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Feed answer+note for each of the two steps, then close stdin.
    {
        let stdin = child.stdin.as_mut().expect("stdin pipe");
        stdin.write_all(b"y\n\n\n\n").unwrap();
    }
    let out = child.wait_with_output().unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));

    // A session file should now exist in the isolated sessions dir.
    let entries: Vec<_> = std::fs::read_dir(sessions.path())
        .expect("sessions dir should exist")
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1, "exactly one session should be written");

    // And it should be valid JSON with the contract header + our outcome.
    let body = std::fs::read_to_string(entries[0].path()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&body).expect("session is JSON");
    assert_eq!(value["schema_version"], serde_json::json!(1));
    assert_eq!(value["source_tool"], serde_json::json!("proto"));
    assert_eq!(value["protocol_id"], serde_json::json!("sample"));
    // First step answered "y" => passed; second (info) => acknowledged.
    assert_eq!(value["steps"][0]["status"], serde_json::json!("passed"));
    assert_eq!(
        value["steps"][1]["status"],
        serde_json::json!("acknowledged")
    );
}

// =============================================================================
// sessions & show — the persistence round-trip through the binary
// =============================================================================

// Helper: run `sample` to completion in an isolated sessions dir, feeding a note
// on the first step. Returns the protocols+sessions temp dirs so the caller can
// then exercise `sessions`/`show` against the same store.
fn run_one_session() -> (tempfile::TempDir, tempfile::TempDir) {
    use std::io::Write;
    use std::process::Stdio;

    let protocols = tempdir().unwrap();
    let sessions = tempdir().unwrap();
    let feed = tempdir().unwrap(); // isolate the feed write from the real tree
    std::fs::write(protocols.path().join("sample.yaml"), VALID).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_proto"))
        .arg("--dir")
        .arg(protocols.path())
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("--feed-dir")
        .arg(feed.path())
        .arg("run")
        .arg("sample")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().expect("stdin pipe");
        // step1: "n" + note "needs work"; step2 (info): Enter + blank note.
        stdin.write_all(b"n\nneeds work\n\n\n").unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));

    (protocols, sessions)
}

#[test]
fn sessions_lists_a_completed_run() {
    let (_protocols, sessions) = run_one_session();

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("sessions")
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    // The listing should name the protocol and show the (1 failed, 1 info) tally.
    assert!(
        stdout.contains("Sample Protocol"),
        "should list protocol: {stdout}"
    );
    assert!(
        stdout.contains("sample-"),
        "should show a session id: {stdout}"
    );
    assert!(
        stdout.contains("failed"),
        "tally should reflect the 'n' answer"
    );
}

#[test]
fn sessions_empty_store_reports_none() {
    let sessions = tempdir().unwrap(); // empty store
    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("sessions")
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(stdout_of(&out).contains("No sessions yet"));
}

#[test]
fn show_displays_a_session_with_its_note() {
    let (_protocols, sessions) = run_one_session();

    // Find the session id by reading the store dir (one file).
    let entry = std::fs::read_dir(sessions.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let id = entry
        .path()
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("show")
        .arg(&id)
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("Sample Protocol"));
    assert!(stdout.contains("[FAIL]"), "first step failed: {stdout}");
    assert!(
        stdout.contains("needs work"),
        "should show the note: {stdout}"
    );
}

#[test]
fn show_unknown_id_fails_nonzero() {
    let sessions = tempdir().unwrap();
    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("show")
        .arg("no-such-session")
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(stderr_of(&out).contains("no-such-session"));
}

#[test]
fn run_unknown_id_fails_nonzero() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("sample.yaml"), VALID).unwrap();

    let out = proto_in(dir.path())
        .arg("run")
        .arg("nope")
        .output() // no stdin needed; it fails before prompting
        .unwrap();

    assert!(!out.status.success());
    assert!(stderr_of(&out).contains("nope"));
}

// Read the (single) session id from a sessions dir — the filename stem of the
// one .json file `run_one_session` wrote. Used by the Phase D command tests.
fn only_session_id(sessions_dir: &Path) -> String {
    let entry = std::fs::read_dir(sessions_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .expect("one session file should exist");
    entry
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

// =============================================================================
// delete / search / export — Phase D session management
// =============================================================================

#[test]
fn delete_with_yes_removes_the_session() {
    let (_protocols, sessions) = run_one_session();
    let id = only_session_id(sessions.path());
    let feed = tempdir().unwrap();

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("--feed-dir")
        .arg(feed.path())
        .arg("delete")
        .arg(&id)
        .arg("--yes") // skip the confirmation prompt
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("Deleted"),
        "should confirm deletion"
    );

    // The session file is gone, and the feed was refreshed to 0 items.
    let remaining = std::fs::read_dir(sessions.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .count();
    assert_eq!(remaining, 0, "the session file should be deleted");
}

#[test]
fn delete_unknown_id_fails() {
    let sessions = tempdir().unwrap();
    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("delete")
        .arg("no-such-session")
        .arg("--yes")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "deleting a missing session must fail"
    );
}

#[test]
fn search_matches_a_note() {
    // run_one_session writes the note "needs work" on step one.
    let (_protocols, sessions) = run_one_session();

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("search")
        .arg("needs work")
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("match"), "should report a match: {stdout}");
    assert!(
        stdout.contains("note"),
        "should say it matched a note: {stdout}"
    );
}

#[test]
fn search_no_match_reports_none() {
    let (_protocols, sessions) = run_one_session();
    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("search")
        .arg("zzz-nothing-matches-this")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(stdout_of(&out).contains("No sessions match"));
}

#[test]
fn export_markdown_to_stdout() {
    let (_protocols, sessions) = run_one_session();
    let id = only_session_id(sessions.path());

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("export")
        .arg(&id)
        // no format flag => markdown default
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    // Markdown heading (protocol title) and the steps table header.
    assert!(stdout.contains("# Sample Protocol"), "md heading: {stdout}");
    assert!(
        stdout.contains("| # | Step | Status | Note |"),
        "md table: {stdout}"
    );
}

#[test]
fn export_json_round_trips() {
    let (_protocols, sessions) = run_one_session();
    let id = only_session_id(sessions.path());

    let out = proto_in(Path::new("unused"))
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("export")
        .arg(&id)
        .arg("--json")
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr:\n{}", stderr_of(&out));
    // The stdout should be parseable JSON carrying the contract header.
    let value: serde_json::Value = serde_json::from_str(&stdout_of(&out)).expect("export is JSON");
    assert_eq!(value["schema_version"], serde_json::json!(1));
    assert_eq!(value["source_tool"], serde_json::json!("proto"));
    assert_eq!(value["protocol_id"], serde_json::json!("sample"));
}

// =============================================================================
// top-level UX
// =============================================================================

#[test]
fn bare_invocation_non_tty_prints_help_and_succeeds() {
    // Bare `proto` branches on whether stdin is a terminal: on a TTY it shows the
    // interactive picker; NON-interactively (here, `.output()` gives piped stdin)
    // it prints help and exits 0 — so `proto | …` and CI stay predictable and
    // never block on a prompt. This test pins the non-TTY branch.
    let dir = tempdir().unwrap();
    let out = proto_in(dir.path()).output().unwrap();

    assert!(
        out.status.success(),
        "bare proto (non-TTY) should print help and exit 0"
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("list"), "help should mention subcommands");
    assert!(stdout.contains("validate"));
    assert!(stdout.contains("run"));
    // It must NOT have tried to run the picker (which would prompt "Pick a ...").
    assert!(
        !stdout.contains("Pick a protocol"),
        "non-TTY must print help, not the interactive picker"
    );
}

#[test]
fn run_succeeds_despite_a_broken_sibling_protocol() {
    // Regression (P2): one invalid/unparseable file in the protocols dir must NOT
    // stop `proto run <good-id>`. `run` resolves only the requested protocol.
    let protocols = tempdir().unwrap();
    let sessions = tempdir().unwrap();
    let feed = tempdir().unwrap();
    std::fs::write(protocols.path().join("sample.yaml"), VALID).unwrap();
    // A sibling that doesn't even parse as YAML — the harshest case.
    std::fs::write(
        protocols.path().join("broken.yaml"),
        "id: broken\ntitle: Broken\nsteps:\n  - id: a\n    title: ok\n  not valid:::\n",
    )
    .unwrap();

    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_proto"))
        .arg("--dir")
        .arg(protocols.path())
        .arg("--sessions-dir")
        .arg(sessions.path())
        .arg("--feed-dir")
        .arg(feed.path())
        .arg("run")
        .arg("sample")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"y\n\n\n\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(
        out.status.success(),
        "run should ignore the broken sibling; stderr:\n{}",
        stderr_of(&out)
    );
    assert_eq!(
        std::fs::read_dir(sessions.path()).unwrap().count(),
        1,
        "the good run should have written its session"
    );
}

#[test]
fn malformed_yaml_error_is_not_printed_twice() {
    // Regression (P3): the serde line/column detail used to appear twice because
    // the error's Display embedded {source} AND main.rs walked the cause chain.
    // The detail must now appear exactly once.
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("broken.yaml"),
        "id: broken\ntitle: Broken\nsteps:\n  - id: a\n    title: ok\n  not valid:::\n",
    )
    .unwrap();

    let out = proto_in(dir.path()).arg("list").output().unwrap();
    assert!(!out.status.success(), "malformed YAML must fail");

    let stderr = stderr_of(&out);
    // serde_yaml reports a "did not find expected" diagnostic for this input; it
    // must show up once, not duplicated.
    let occurrences = stderr.matches("did not find expected").count();
    assert_eq!(
        occurrences, 1,
        "serde detail should appear exactly once, got {occurrences}:\n{stderr}"
    );
}
