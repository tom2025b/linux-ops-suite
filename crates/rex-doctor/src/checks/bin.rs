//! `bin.*` — binaries & versions. Answers "are the suite tools actually
//! installed, runnable on this arch/libc, and at compatible versions?"
//!
//! `bin.runs` is the check existence alone can't replace: a wrong-arch prebuilt
//! asset exists but won't execute — exactly the cross-arch install failure. It
//! runs each tool with a hard timeout so one hung binary can't hang the doctor.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::model::{Category, Check};
use crate::util::{self, SUITE_BINS};

const CAT: Category = Category::Bin;

/// Per-subprocess wall-clock budget for `--version`. Generous enough for a cold
/// page-in, short enough that a hung tool fails fast rather than stalling.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Run every `bin.*` check, in id order.
pub fn run() -> Vec<Check> {
    vec![present(), executable(), runs(), shadowed(), version_skew()]
}

/// `bin.present` — every suite binary resolves on `PATH`. The headline "is the
/// suite installed at all" check; a missing tool names the install command.
fn present() -> Check {
    let id = "bin.present";
    let missing: Vec<&str> = SUITE_BINS
        .iter()
        .copied()
        .filter(|b| util::which(b).is_none())
        .collect();
    if missing.is_empty() {
        Check::pass(
            id,
            CAT,
            format!("all {} suite binaries on PATH", SUITE_BINS.len()),
        )
    } else {
        Check::fail(
            id,
            CAT,
            format!("missing on PATH: {}", missing.join(", ")),
            "install: cargo run -p linux-ops-install -- --force",
        )
    }
}

/// `bin.executable` — each *found* binary is a real executable file, not a
/// 0-byte or half-downloaded asset. (Presence on PATH already implies the exec
/// bit via `which`; this re-checks the file is non-empty and still executable,
/// catching a truncated download that `which` would still surface.)
fn executable() -> Check {
    let id = "bin.executable";
    let mut bad = Vec::new();
    for bin in SUITE_BINS {
        if let Some(path) = util::which(bin) {
            let ok = std::fs::metadata(&path)
                .map(|m| m.len() > 0 && util::is_executable_file(&path))
                .unwrap_or(false);
            if !ok {
                bad.push(bin.to_string());
            }
        }
    }
    if bad.is_empty() {
        Check::pass(id, CAT, "found binaries are non-empty and executable")
    } else {
        Check::fail(
            id,
            CAT,
            format!("not a usable executable: {}", bad.join(", ")),
            "reinstall: cargo run -p linux-ops-install -- --force",
        )
    }
}

/// `bin.runs` — each found binary answers `--version` within the timeout
/// without crashing. Proves the artifact actually runs on this arch/libc.
fn runs() -> Check {
    let id = "bin.runs";
    let mut broken = Vec::new();
    for bin in SUITE_BINS {
        let Some(path) = util::which(bin) else {
            continue; // bin.present owns the missing case.
        };
        match probe_version(&path) {
            ProbeResult::Ok(_) => {}
            ProbeResult::TimedOut => broken.push(format!("{bin} (timed out)")),
            ProbeResult::Failed => broken.push(format!("{bin} (crashed/no --version)")),
        }
    }
    if broken.is_empty() {
        Check::pass(id, CAT, "every found binary runs and reports a version")
    } else {
        Check::fail(
            id,
            CAT,
            format!("binary won't run here: {}", broken.join(", ")),
            "wrong arch/libc? reinstall: cargo run -p linux-ops-install -- --force",
        )
    }
}

/// `bin.shadowed` — no suite binary has more than one copy on `PATH`. Two
/// different-version copies on PATH is a real source of "I fixed it but nothing
/// changed"; report which one wins.
fn shadowed() -> Check {
    let id = "bin.shadowed";
    let mut shadowed = Vec::new();
    for bin in SUITE_BINS {
        let hits = util::which_all(bin);
        if hits.len() > 1 {
            let winner = hits[0].display();
            shadowed.push(format!("{bin} → {winner} (+{} more)", hits.len() - 1));
        }
    }
    if shadowed.is_empty() {
        Check::pass(id, CAT, "no suite binary is shadowed by a second PATH copy")
    } else {
        Check::warn(
            id,
            CAT,
            format!("multiple copies on PATH: {}", shadowed.join("; ")),
            "remove the stale copy, or reorder PATH so the right dir wins",
        )
    }
}

/// `bin.version-skew` — flag suite binaries whose reported version differs from
/// the most common version across the suite. WARN, not FAIL: mixed versions are
/// normal mid-rollout, but you want to know which tool is the odd one out.
fn version_skew() -> Check {
    let id = "bin.version-skew";
    let mut versions: Vec<(&str, String)> = Vec::new();
    for bin in SUITE_BINS {
        if let Some(path) = util::which(bin) {
            if let ProbeResult::Ok(v) = probe_version(&path) {
                if let Some(ver) = parse_version(&v) {
                    versions.push((bin, ver));
                }
            }
        }
    }
    if versions.len() < 2 {
        return Check::skip(
            id,
            CAT,
            "fewer than two versioned binaries found; nothing to compare",
        );
    }
    // The modal version is the baseline; anything not matching it is "skew".
    let baseline = modal_version(&versions);
    let outliers: Vec<String> = versions
        .iter()
        .filter(|(_, v)| *v != baseline)
        .map(|(b, v)| format!("{b} {v}"))
        .collect();
    if outliers.is_empty() {
        Check::pass(id, CAT, format!("all suite binaries at {baseline}"))
    } else {
        Check::warn(
            id,
            CAT,
            format!("version skew vs {baseline}: {}", outliers.join(", ")),
            "align versions: cargo run -p linux-ops-install -- --force",
        )
    }
}

/// Outcome of a `--version` probe.
enum ProbeResult {
    /// Ran cleanly; carries combined stdout+stderr (trimmed).
    Ok(String),
    /// Did not finish within [`PROBE_TIMEOUT`].
    TimedOut,
    /// Spawned but exited non-zero, or could not be spawned.
    Failed,
}

/// Run `<path> --version` with a wall-clock timeout. Uses a spawn + poll loop
/// (no threads, no extra deps) so a hung child is killed rather than awaited.
fn probe_version(path: &Path) -> ProbeResult {
    let mut child = match Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return ProbeResult::Failed,
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Drain captured output regardless of exit status.
                let out = child
                    .wait_with_output()
                    .map(|o| {
                        let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                        s.push_str(&String::from_utf8_lossy(&o.stderr));
                        s.trim().to_string()
                    })
                    .unwrap_or_default();
                return if status.success() {
                    ProbeResult::Ok(out)
                } else {
                    ProbeResult::Failed
                };
            }
            Ok(None) => {
                if start.elapsed() >= PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ProbeResult::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return ProbeResult::Failed,
        }
    }
}

/// Pull a bare `x.y.z` out of a `--version` line like `bulwark 0.1.2`.
fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| {
            let core = tok.trim_start_matches('v');
            let mut parts = core.split('.');
            parts.clone().count() >= 2
                && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|tok| tok.trim_start_matches('v').to_string())
}

/// The most frequently occurring version string (ties broken by first seen).
fn modal_version(versions: &[(&str, String)]) -> String {
    let mut best = versions[0].1.clone();
    let mut best_count = 0usize;
    for (_, v) in versions {
        let count = versions.iter().filter(|(_, o)| o == v).count();
        if count > best_count {
            best_count = count;
            best = v.clone();
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Status;

    #[test]
    fn parse_version_extracts_semver_like_token() {
        assert_eq!(parse_version("bulwark 0.1.2").as_deref(), Some("0.1.2"));
        assert_eq!(parse_version("proto v1.4").as_deref(), Some("1.4"));
        assert_eq!(
            parse_version("tool 2.0.0\nbuilt: x").as_deref(),
            Some("2.0.0")
        );
        assert_eq!(parse_version("no version here").as_deref(), None);
    }

    #[test]
    fn modal_version_picks_the_majority() {
        let v = vec![
            ("a", "0.1.2".to_string()),
            ("b", "0.1.2".to_string()),
            ("c", "0.1.1".to_string()),
        ];
        assert_eq!(modal_version(&v), "0.1.2");
    }

    #[test]
    fn probe_version_runs_a_real_command() {
        // `sh --version` succeeds on bash-as-sh; `false --version` does not.
        // We assert the timeout/spawn machinery returns *some* decisive result,
        // not a hang. (Exact variant depends on the host's coreutils.)
        let sh = util::which("sh").expect("sh present");
        match probe_version(&sh) {
            ProbeResult::Ok(_) | ProbeResult::Failed => {}
            ProbeResult::TimedOut => panic!("sh --version should not time out"),
        }
    }

    #[test]
    fn version_skew_skips_without_enough_data() {
        // In a bare test env the suite binaries usually aren't installed, so
        // this should SKIP (not panic, not FAIL) — graceful degradation.
        let c = version_skew();
        assert!(matches!(
            c.status,
            Status::Skip | Status::Pass | Status::Warn
        ));
    }

    #[test]
    fn present_fails_loudly_when_suite_absent() {
        // Either the suite is installed (PASS) or it isn't (FAIL with the
        // install command) — never a crash.
        let c = present();
        assert!(matches!(c.status, Status::Pass | Status::Fail));
        if c.status == Status::Fail {
            assert!(c.fix.as_deref().unwrap_or("").contains("linux-ops-install"));
        }
    }
}
