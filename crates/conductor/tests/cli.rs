//! End-to-end CLI tests: run the built `conductor` binary against a temp data
//! dir and assert the human + JSON output and exit codes. `--data-dir` and
//! `--no-color` keep this deterministic and color-free.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    // Cargo exposes the built binary path to integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_conductor"))
}

struct TempRoot {
    dir: PathBuf,
}

/// The suite binaries conductor probes for. Mirrors `sources::SUITE_BINARIES`;
/// the tests stub all of them so the wiring-gap rule stays dormant and the
/// probe's outcome doesn't depend on what the dev machine has installed.
const SUITE_BINARIES: &[&str] = &[
    "pulse",
    "rewind",
    "tripwire",
    "portman",
    "bulwark",
    "workstate",
    "proto",
    "rexops",
];

impl TempRoot {
    fn new(tag: &str) -> Self {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("conductor-cli-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempRoot { dir }
    }
    fn write(&self, rel: &str, body: &str) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(p)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }

    /// A `bin/` dir under this root holding an executable stub for every suite
    /// binary, so a PATH pointed here makes the probe report them all present —
    /// the deterministic baseline for tests that aren't about wiring gaps. The
    /// stubs are never executed (conductor Phase 1 only probes presence).
    fn stub_bin_dir(&self) -> PathBuf {
        let bin = self.dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in SUITE_BINARIES {
            let p = bin.join(name);
            std::fs::File::create(&p)
                .unwrap()
                .write_all(b"#!/bin/sh\n")
                .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        bin
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn run(root: &TempRoot, args: &[&str]) -> std::process::Output {
    // Point PATH at a dir of stub suite binaries so the probe (rule 2 / `health`)
    // is deterministic regardless of what the dev machine has installed: all
    // suite binaries read as present, so the wiring-gap rule stays dormant and
    // the data-dir contents alone drive the plan. `bin()` is an absolute path
    // (CARGO_BIN_EXE_*), so it still launches conductor itself fine.
    Command::new(bin())
        .env("PATH", root.stub_bin_dir())
        .arg("--data-dir")
        .arg(&root.dir)
        .arg("--no-color")
        .args(args)
        .output()
        .expect("failed to run conductor")
}

#[test]
fn empty_suite_status_is_nothing_to_conduct_and_exits_zero() {
    let t = TempRoot::new("empty");
    let out = run(&t, &["status"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("nothing to conduct"));
}

#[test]
fn bare_invocation_defaults_to_status() {
    let t = TempRoot::new("bare");
    let out = run(&t, &[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("nothing to conduct"));
}

#[test]
fn stale_feed_and_finding_produce_an_ordered_plan() {
    let t = TempRoot::new("plan");
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
    );
    t.write(
        "rexops/snapshot.json",
        r#"{ "attention": [ { "tool":"bulwark","id":"deploy-prod.sh","reason":"AWS key","severity":"critical" } ] }"#,
    );
    let out = run(&t, &["status"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // refresh comes before the investigate step
    let refresh = stdout.find("workstate snapshot").unwrap();
    let investigate = stdout.find("bulwark show deploy-prod.sh").unwrap();
    assert!(refresh < investigate);
    assert!(stdout.contains("changes state"));
}

#[test]
fn json_status_is_the_suite_envelope_with_ids() {
    let t = TempRoot::new("json");
    t.write(
        "rexops/snapshot.json",
        r#"{ "attention": [ { "tool":"bulwark","id":"x.sh","reason":"k","severity":"high" } ] }"#,
    );
    let out = run(&t, &["status", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["source_tool"], "conductor");
    assert!(v["plan_id"].as_str().unwrap().starts_with("plan-"));
    assert_eq!(v["steps"][0]["id"], "safety-capture");
}

#[test]
fn drift_correlation_is_visible_end_to_end() {
    let t = TempRoot::new("drift");
    t.write(
        "rexops/snapshot.json",
        r#"{ "attention": [
              { "tool":"bulwark","id":"crit.sh","reason":"k","severity":"critical" },
              { "tool":"bulwark","id":"deploy-prod.sh","reason":"key","severity":"high" }
            ] }"#,
    );
    t.write("tripwire/drift.json", r#"{ "paths": ["deploy-prod.sh"] }"#);
    let out = run(&t, &["status"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // correlated High lifted ahead of Critical, with the note
    let dep = stdout.find("investigate deploy-prod.sh").unwrap();
    let crit = stdout.find("investigate crit.sh").unwrap();
    assert!(dep < crit, "drift-correlated finding must come first");
    assert!(stdout.contains("same file as tripwire drift"));
}

#[test]
fn health_runs_and_exits_zero() {
    let t = TempRoot::new("health");
    let out = run(&t, &["health"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("feeds"));
    assert!(stdout.contains("tools on PATH"));
}

#[test]
fn plan_verb_prints_steps_without_situation() {
    let t = TempRoot::new("planverb");
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "tools": { "status": "Stale" } }"#,
    );
    let out = run(&t, &["plan"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("workstate snapshot"));
    assert!(!stdout.contains("the situation"));
}

#[test]
fn dump_view_plan_renders_steps_for_a_stale_feed_state() {
    let t = TempRoot::new("dumpplan");
    // A stale workstate feed → a refresh step in the plan.
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
    );
    let out = run(&t, &["--dump-view", "plan"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("the plan"), "frame:\n{s}");
    assert!(s.contains("workstate snapshot"), "frame:\n{s}");
    assert!(s.contains("changes state"), "frame:\n{s}");
}

#[test]
fn dump_view_healthy_says_nothing_to_conduct_when_all_clear() {
    // Empty data dir: no feeds, no findings → empty plan → healthy screen.
    // run() already points PATH at stub_bin_dir, so all 8 bins are "present"
    // and the wiring-gap rule stays dormant.
    let t = TempRoot::new("dumphealthy");
    let out = run(&t, &["--dump-view", "healthy"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("nothing to conduct"), "frame:\n{s}");
}

#[test]
fn bare_non_tty_falls_back_to_status_not_the_tui() {
    // Output is captured (not a TTY), so bare conductor must print status text.
    let t = TempRoot::new("barenontty");
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
    );
    let out = run(&t, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    // status output contains the plan heading; the TUI would have needed a TTY.
    assert!(
        s.contains("the plan") || s.contains("nothing to conduct"),
        "frame:\n{s}"
    );
}

#[test]
fn orchestrate_verb_is_listed_in_help() {
    let t = TempRoot::new("orch-help");
    let out = run(&t, &["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("orchestrate"),
        "help must list the orchestrate verb"
    );
}

#[test]
fn orchestrate_json_is_non_interactive_and_matches_status() {
    // orchestrate shares the non-TTY fallback: with --json it prints the status
    // envelope and exits 0, never opening a TUI (the test harness is not a TTY).
    let t = TempRoot::new("orch-json");
    let o = run(&t, &["orchestrate", "--json"]);
    let s = run(&t, &["status", "--json"]);
    assert!(o.status.success());
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&s.stdout),
        "orchestrate --json must equal status --json"
    );
}

#[test]
fn dump_view_confirm_renders_the_ring2_modal() {
    // A stale feed yields a Ring-2 refresh step at the top; the confirm view
    // renders its modal deterministically (no PTY).
    let t = TempRoot::new("confirm-dump");
    t.write(
        "rexops/feeds/workstate.snapshot.json",
        r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
    );
    let out = run(&t, &["--dump-view", "confirm"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("this will run:"));
    assert!(text.contains("changes state"));
    assert!(text.contains("y  run it"));
}
