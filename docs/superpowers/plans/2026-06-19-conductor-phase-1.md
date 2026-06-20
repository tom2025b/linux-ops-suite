# Conductor Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Conductor Phase 1 — the Ring 0, 100% read-only foundation: read the suite's contract files, derive a deterministic ordered plan from built-in rules, and render it (and a health view) as human text or a JSON envelope via `conductor status | health | plan`.

**Architecture:** Thin `main.rs` parses flags and dispatches to a subcommand; the library does all the work and returns values. A layered pipeline with one job per module: `sources.rs` reads contract files fault-tolerantly (missing/malformed ⇒ "unavailable", never panics); `state.rs` holds the normalized facts with no I/O; `plan/rules.rs` is a pure `&SuiteState -> Plan` function (the product's brain); `report.rs` renders state+plan to human text or the suite JSON envelope. No subprocess and no TUI in Phase 1 — those are Phases 2 and 3.

**Tech Stack:** Rust 2021 (workspace), clap 4 (derive), serde + serde_json, chrono. No new third-party dependency. `tempfile` as a dev-dependency for reader tests (matches rewind). Hand-rolled `isatty`/XDG helpers via tiny `extern "C"` (matches rewind/pulse).

## Global Constraints

- **Location:** crate lives at `crates/conductor/`; binary and lib both named `conductor`.
- **MSRV / edition:** `rust-version = "1.85"`, `edition = "2021"` — inherit via `.workspace = true`.
- **Dependencies:** workspace deps only — `clap`, `serde`, `serde_json`, `chrono`; dev-dep `tempfile = "3"`. **No new third-party crate.** No network, no async.
- **Read-only by contract:** Phase 1 code touches NO live file for writing and spawns NO subprocess. Every reader resolves a missing/empty/malformed/wrong-major file to "unavailable" and never panics (`docs/CONTRACT_RULES.md`).
- **JSON envelope:** every `--json` output is an object with `schema_version: 1` and `source_tool: "conductor"` as the first two fields (matches rewind's `TimelineEnvelope`).
- **Exit codes:** `0` ok (incl. "nothing to conduct"); `3` conductor itself could not run (e.g. no data dir). `1` and `2` are reserved for Phase 3 (`orchestrate`) — do not emit them.
- **Color rule:** color on only when stdout is a TTY and `NO_COLOR` is unset, force-off via `--no-color`. Fully legible with color off — state carried by word + glyph, never color alone.
- **Data paths:** read under `$XDG_DATA_HOME` (fallback `~/.local/share`); `--data-dir <DIR>` overrides the root wholesale (mirrors `PULSE_DATA_DIR`). Paths follow `docs/INTEGRATION_MAP.md` "Expected output paths".
- **No wrappers:** no `r-conductor` wrapper, no shell alias — bare binary on `$PATH` only.
- **Style:** thin `main`, library does the work, renderers derive from the model. Match rewind/tripwire/pulse idioms (doc-comment each module's single responsibility).

---

## File Structure

```text
crates/conductor/
  Cargo.toml          # workspace deps only; bin+lib both "conductor"
  README.md           # what it is, the rings, Phase-1 usage
  src/
    main.rs           # thin clap CLI: parse → dispatch status|health|plan → render → exit code
    lib.rs            # public surface: load_state(), build_plan(); re-exports
    error.rs          # ConductorError (NoDataDir → exit 3). Read failures are NOT errors.
    util.rs           # isatty + XDG data-dir resolution (hand-rolled, dep-free)
    sources.rs        # fault-tolerant readers for every contract + $PATH probe
    state.rs          # SuiteState: normalized facts, no I/O / no rules / no render
    plan/
      mod.rs          # Plan, Step, Ring, StepStatus types + build(&SuiteState) -> Plan
      rules.rs        # the v1 rules: pure helpers state -> steps, in priority order
    report.rs         # human + JSON renderers for status / health / plan
```

Module-boundary intent (one job each): `sources.rs` only reads files → tolerant raw values; `state.rs` only holds normalized facts; `plan/rules.rs` only decides steps from facts (pure, deterministic, the densest tests); `report.rs` only renders. A render change can't alter which steps exist; a new rule can't change rendering.

Phase 1 deliberately omits `run.rs` and `tui/` (Phases 2–3). `main.rs` dispatches only the three read-only verbs; `orchestrate`/`doctor` are added in later phases.

---

## Task 1: Crate skeleton — manifest, errors, util, workspace registration

**Files:**
- Create: `crates/conductor/Cargo.toml`
- Create: `crates/conductor/src/lib.rs` (temporary stub, expanded in later tasks)
- Create: `crates/conductor/src/error.rs`
- Create: `crates/conductor/src/util.rs`
- Modify: `Cargo.toml` (root) — add `"crates/conductor"` to `members`

**Interfaces:**
- Produces: `conductor::error::ConductorError` (enum, `NoDataDir` variant; `Display` + `std::error::Error`); `conductor::util::stdout_is_tty() -> bool`; `conductor::util::data_root() -> Option<PathBuf>` (the suite data root, honoring `$XDG_DATA_HOME` then `~/.local/share`, with NO per-tool suffix — Conductor reads other tools' subtrees under this root).

- [ ] **Step 1: Add the crate to the workspace members**

Modify root `Cargo.toml`, the `members` array, adding the line after `"crates/rewind",`:

```toml
members = [
    "crates/suite-ui",
    "crates/thomas-tui",
    "crates/toolbox-bridge",
    "crates/linux-ops-install",
    "crates/rex-check",
    "crates/rex-doctor",
    "crates/portman",
    "crates/pulse",
    "crates/tripwire",
    "crates/rewind",
    "crates/conductor",
]
```

- [ ] **Step 2: Write the crate manifest**

Create `crates/conductor/Cargo.toml`:

```toml
[package]
name = "conductor"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "The Linux Ops Suite's guided operator: reads the suite's own state, derives a short ordered runbook (do these things, in this order), and walks the operator through it — delegating every step to the tool that owns it. Read-only by default; it never writes a live file itself."

[[bin]]
name = "conductor"
path = "src/main.rs"

[lib]
name = "conductor"
path = "src/lib.rs"

# Lean on purpose, same philosophy as rewind/tripwire/pulse: Conductor reads the
# suite's contract files (serde_json), probes $PATH, and renders. It hand-rolls
# its isatty/XDG helpers via tiny extern "C" calls (no libc dep) and uses chrono
# only for the "checked Nm ago" stamp. No network, no async, no new 3rd-party dep.
[dependencies]
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write the error type (test first)**

Create `crates/conductor/src/error.rs`:

```rust
//! Typed errors for the few things that can stop conductor *itself* from
//! running. A file we can't read, a malformed feed, a missing binary — none of
//! those are errors. They are data: a source that resolves to "unavailable",
//! which narrows the plan. These variants are only for "conductor could not even
//! produce a view": no data dir to anchor reads. They map to exit code 3, the
//! same `NoDataDir` rewind makes.

use std::fmt;

/// Errors that abort a command before it can produce output (exit code 3).
#[derive(Debug)]
pub enum ConductorError {
    /// Neither `$XDG_DATA_HOME` nor `$HOME` resolves, so there's nowhere to read
    /// the suite's contract files from.
    NoDataDir,
}

impl fmt::Display for ConductorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConductorError::NoDataDir => f.write_str(
                "cannot resolve $XDG_DATA_HOME or $HOME; conductor needs one to read suite state",
            ),
        }
    }
}

impl std::error::Error for ConductorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_dir_displays_a_helpful_message() {
        let msg = ConductorError::NoDataDir.to_string();
        assert!(msg.contains("conductor needs one"));
    }
}
```

- [ ] **Step 4: Write util (test first)**

Create `crates/conductor/src/util.rs`:

```rust
//! Small, dependency-free helpers. The TTY rule mirrors rewind/tripwire/pulse
//! (same `isatty(3)` call) so the suite agrees on what "a terminal" means. The
//! data root follows the same XDG path the rest of the suite uses, but with NO
//! per-tool suffix: conductor reads *other* tools' subtrees (rexops/…,
//! workstate/…, proto/…) under this one root.

use std::env;
use std::path::PathBuf;

/// Whether stdout is a TTY — gates color.
pub fn stdout_is_tty() -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(1) == 1 }
}

/// The user's home directory; `None` when `$HOME` is unset/empty.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The suite *data root*: `$XDG_DATA_HOME`, else `~/.local/share`. Unlike
/// rewind's per-tool `data_dir`, this is the shared root the other tools write
/// their subtrees under, so conductor can read them. `None` only when neither
/// `$XDG_DATA_HOME` nor `$HOME` is usable.
pub fn data_root() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/share")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_root_prefers_xdg_data_home() {
        // Save + restore env to avoid cross-test leakage.
        let prev_xdg = env::var_os("XDG_DATA_HOME");
        env::set_var("XDG_DATA_HOME", "/tmp/conductor-xdg-test");
        assert_eq!(data_root(), Some(PathBuf::from("/tmp/conductor-xdg-test")));
        match prev_xdg {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
    }

    #[test]
    fn data_root_falls_back_to_home_local_share() {
        let prev_xdg = env::var_os("XDG_DATA_HOME");
        let prev_home = env::var_os("HOME");
        env::remove_var("XDG_DATA_HOME");
        env::set_var("HOME", "/home/example");
        assert_eq!(
            data_root(),
            Some(PathBuf::from("/home/example/.local/share"))
        );
        match prev_xdg {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
        match prev_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }
}
```

- [ ] **Step 5: Write the temporary lib stub**

Create `crates/conductor/src/lib.rs`:

```rust
//! conductor — the Linux Ops Suite's guided operator.
//!
//! Phase 1 (this build) is the Ring 0, read-only foundation: read the suite's
//! contract files, derive a deterministic ordered plan, and render it. The
//! library does the work and returns values; the binary only parses flags and
//! prints. See `CONDUCTOR_DESIGN.md` at the repo root.

pub mod error;
pub mod util;

pub use error::ConductorError;
```

- [ ] **Step 6: Verify it builds and the unit tests pass**

Run: `cargo test -p conductor`
Expected: PASS — `error` and `util` tests green; crate compiles.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/conductor/Cargo.toml crates/conductor/src/error.rs crates/conductor/src/util.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): crate skeleton — manifest, errors, util, workspace registration"
```

---

## Task 2: The SuiteState model

**Files:**
- Create: `crates/conductor/src/state.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod state;`)

**Interfaces:**
- Consumes: nothing (pure data types).
- Produces: the normalized fact types the rules and renderers read.
  - `enum Freshness { Current, Stale, Unavailable }`
  - `enum Severity { Low, Medium, High, Critical }` (derives `PartialOrd, Ord` so highest-severity-first sorting works)
  - `struct Finding { pub what: String, pub why: String, pub source: String, pub severity: Severity }`
  - `struct FeedStatus { pub name: &'static str, pub freshness: Freshness }`
  - `struct BinaryStatus { pub name: &'static str, pub present: bool }`
  - `struct DriftedPath { pub path: String }`
  - `struct FailedJob { pub title: String }`
  - `struct SuiteState { pub built_at: Option<String>, pub feeds: Vec<FeedStatus>, pub findings: Vec<Finding>, pub drift: Vec<DriftedPath>, pub failed_jobs: Vec<FailedJob>, pub binaries: Vec<BinaryStatus> }`
  - `impl SuiteState { pub fn empty() -> Self }` and `pub fn has_stale_or_unavailable_feed(&self) -> bool`, `pub fn missing_binaries(&self) -> Vec<&BinaryStatus>`.

- [ ] **Step 1: Write the failing test**

Create `crates/conductor/src/state.rs`:

```rust
//! The normalized snapshot of everything conductor read this run. It holds
//! *facts only* — no I/O (that's `sources.rs`), no rules (that's `plan::rules`),
//! no rendering (that's `report.rs`). It is the single input to the rule engine,
//! which makes the rules a pure function of this struct and trivially testable.

/// One source's freshness, collapsed to the three buckets the rules care about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Current,
    Stale,
    /// Present but unusable (unsupported version) or absent entirely.
    Unavailable,
}

/// Severity of a finding, in escalation order (so `Ord` sorts worst last).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// One thing worth the operator's attention, normalized across producers.
#[derive(Clone, Debug)]
pub struct Finding {
    pub what: String,
    pub why: String,
    pub source: String,
    pub severity: Severity,
}

/// A named feed and its freshness (drives rule 1 and the health view).
#[derive(Clone, Debug)]
pub struct FeedStatus {
    pub name: &'static str,
    pub freshness: Freshness,
}

/// A suite binary and whether it is on `$PATH` (drives rule 2 and health).
#[derive(Clone, Copy, Debug)]
pub struct BinaryStatus {
    pub name: &'static str,
    pub present: bool,
}

/// A filesystem path tripwire reports as drifted (feeds rule 5's correlation).
#[derive(Clone, Debug)]
pub struct DriftedPath {
    pub path: String,
}

/// A job (Proto session) that ended in failure (drives rule 6).
#[derive(Clone, Debug)]
pub struct FailedJob {
    pub title: String,
}

/// Everything conductor read, normalized. The sole input to `plan::build`.
#[derive(Clone, Debug, Default)]
pub struct SuiteState {
    /// When the suite snapshot was built (RFC3339), for the "checked" stamp.
    pub built_at: Option<String>,
    pub feeds: Vec<FeedStatus>,
    pub findings: Vec<Finding>,
    pub drift: Vec<DriftedPath>,
    pub failed_jobs: Vec<FailedJob>,
    pub binaries: Vec<BinaryStatus>,
}

impl SuiteState {
    /// An all-empty state — the starting point a loader fills in, and the
    /// "nothing was readable" baseline.
    pub fn empty() -> Self {
        Self::default()
    }

    /// True if any feed is stale or unavailable (rule 1's precondition).
    pub fn has_stale_or_unavailable_feed(&self) -> bool {
        self.feeds
            .iter()
            .any(|f| f.freshness != Freshness::Current)
    }

    /// The suite binaries missing from `$PATH` (rule 2's precondition).
    pub fn missing_binaries(&self) -> Vec<&BinaryStatus> {
        self.binaries.iter().filter(|b| !b.present).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_has_no_attention_and_no_stale_feeds() {
        let s = SuiteState::empty();
        assert!(s.findings.is_empty());
        assert!(!s.has_stale_or_unavailable_feed());
        assert!(s.missing_binaries().is_empty());
    }

    #[test]
    fn stale_or_unavailable_feed_is_detected() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "scripts", freshness: Freshness::Current });
        assert!(!s.has_stale_or_unavailable_feed());
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        assert!(s.has_stale_or_unavailable_feed());
    }

    #[test]
    fn severity_orders_worst_last() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        let mut v = vec![Severity::Low, Severity::Critical, Severity::Medium];
        v.sort();
        assert_eq!(v, vec![Severity::Low, Severity::Medium, Severity::Critical]);
    }

    #[test]
    fn missing_binaries_lists_only_absent_ones() {
        let mut s = SuiteState::empty();
        s.binaries.push(BinaryStatus { name: "pulse", present: true });
        s.binaries.push(BinaryStatus { name: "rewind", present: false });
        let missing = s.missing_binaries();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "rewind");
    }
}
```

- [ ] **Step 2: Wire the module**

Modify `crates/conductor/src/lib.rs`, add after `pub mod error;`:

```rust
pub mod state;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p conductor state::`
Expected: PASS — all four `state` tests green.

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/state.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): SuiteState model — normalized facts, the rule engine's sole input"
```

---

## Task 3: Fault-tolerant sources — feed freshness + the binary probe

**Files:**
- Create: `crates/conductor/src/sources.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod sources;`)

This task ports Pulse's proven `sources.rs` discipline (the single `read_json` choke point that turns every failure into "unavailable"). It covers the Workstate-snapshot freshness reader and the `$PATH` binary probe. Findings/drift/jobs are Task 4 so each stays independently reviewable.

**Interfaces:**
- Consumes: `state::{Freshness, FeedStatus, BinaryStatus}` from Task 2; `util::data_root` from Task 1.
- Produces:
  - `struct DataDir { root: PathBuf }` with `pub fn new(root: PathBuf) -> Self`, `pub fn from_env() -> Result<Self, ConductorError>` (uses `util::data_root`, `Err(NoDataDir)` when absent), and path accessors: `workstate_snapshot()`, `rexops_snapshot()`, `bulwark_feed()`, `proto_sessions()` — each returning `PathBuf` rooted at `root`, exactly the locations from `docs/INTEGRATION_MAP.md`.
  - `pub fn read_feeds(dir: &DataDir) -> (Option<String>, Vec<FeedStatus>)` — returns `(built_at, feeds)`; missing snapshot ⇒ `(None, vec![])`.
  - `pub fn read_binaries() -> Vec<BinaryStatus>` over `SUITE_BINARIES`.
  - `pub const SUITE_BINARIES: &[&str]`.

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/sources.rs`:

```rust
//! Reading the suite's file contracts for conductor.
//!
//! Conductor is a passive reader (see CONDUCTOR_DESIGN.md): it consumes
//! published artifacts and never mutates a producer. Every reader here is
//! **fault-tolerant by design** — a missing, unreadable, empty, malformed, or
//! wrong-major file resolves to "unavailable" and never panics
//! (`docs/CONTRACT_RULES.md`). serde ignores unknown fields, so additive
//! contract changes (same major) need no change here. This discipline is lifted
//! from pulse's `sources.rs`.
//!
//! Paths follow `docs/INTEGRATION_MAP.md` ("Expected output paths"), rooted at
//! the suite data root (`$XDG_DATA_HOME`, fallback `~/.local/share`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConductorError;
use crate::state::{BinaryStatus, FeedStatus, Freshness};
use crate::util;

/// The resolved on-disk layout conductor reads from.
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Build from an explicit root (used by `--data-dir` and tests).
    pub fn new(root: PathBuf) -> Self {
        DataDir { root }
    }

    /// Resolve from the environment; `Err(NoDataDir)` when no root is usable.
    pub fn from_env() -> Result<Self, ConductorError> {
        util::data_root()
            .map(|root| DataDir { root })
            .ok_or(ConductorError::NoDataDir)
    }

    /// Workstate's suite snapshot, as RexOps reads it.
    pub fn workstate_snapshot(&self) -> PathBuf {
        self.root.join("rexops/feeds/workstate.snapshot.json")
    }

    /// RexOps's assembled suite snapshot.
    pub fn rexops_snapshot(&self) -> PathBuf {
        self.root.join("rexops/snapshot.json")
    }

    /// Bulwark's Workstate feed.
    pub fn bulwark_feed(&self) -> PathBuf {
        self.root.join("workstate/feeds/bulwark.json")
    }

    /// Directory of Proto session records, one JSON per run.
    pub fn proto_sessions(&self) -> PathBuf {
        self.root.join("proto/sessions")
    }

    /// Tripwire's (optional, not-yet-contracted) drift file. The single point
    /// that becomes a real tripwire contract later; see `read_drift`.
    pub fn tripwire_drift(&self) -> PathBuf {
        self.root.join("tripwire/drift.json")
    }

    /// The data root itself. `pub(crate)` so sibling modules can join an
    /// uncontracted path (drift) without widening the public path surface.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

/// Read a file and parse it as `T`, returning `None` on *any* failure (absent,
/// unreadable, empty, malformed). The single choke point that makes every reader
/// fault-tolerant.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ── Feed freshness — Workstate snapshot ──────────────────────────────────────

#[derive(Deserialize)]
struct WorkstateSnapshot {
    #[serde(default)]
    built_at: Option<String>,
    #[serde(default)]
    scripts: Option<Section>,
    #[serde(default)]
    tools: Option<Section>,
    #[serde(default)]
    findings: Option<Section>,
}

#[derive(Deserialize)]
struct Section {
    /// `"Fresh"`, `"Stale"`, or an object variant like `{"UnsupportedVersion":…}`.
    /// Read as a free `Value` so the object form never fails to parse.
    status: serde_json::Value,
}

impl Section {
    fn freshness(&self) -> Freshness {
        match &self.status {
            serde_json::Value::String(s) if s == "Fresh" => Freshness::Current,
            serde_json::Value::String(s) if s == "Stale" => Freshness::Stale,
            _ => Freshness::Unavailable,
        }
    }
}

/// Read Workstate snapshot freshness. Always returns a value: a missing/malformed
/// file yields `(None, vec![])` — the rules read that as the suite view being
/// unavailable, not as an error.
pub fn read_feeds(dir: &DataDir) -> (Option<String>, Vec<FeedStatus>) {
    let Some(snap): Option<WorkstateSnapshot> = read_json(&dir.workstate_snapshot()) else {
        return (None, Vec::new());
    };
    let mut feeds = Vec::new();
    if let Some(s) = &snap.scripts {
        feeds.push(FeedStatus { name: "scripts", freshness: s.freshness() });
    }
    if let Some(s) = &snap.tools {
        feeds.push(FeedStatus { name: "tools", freshness: s.freshness() });
    }
    if let Some(s) = &snap.findings {
        feeds.push(FeedStatus { name: "findings", freshness: s.freshness() });
    }
    (snap.built_at, feeds)
}

// ── Binary presence on $PATH ─────────────────────────────────────────────────

/// The suite binaries conductor expects, and may need to spawn in later phases.
pub const SUITE_BINARIES: &[&str] =
    &["pulse", "rewind", "tripwire", "portman", "bulwark", "workstate", "proto", "rexops"];

/// Probe each suite binary on `$PATH`. Pure filesystem lookups, no subprocess —
/// conductor stays read-only and can't hang on a slow child.
pub fn read_binaries() -> Vec<BinaryStatus> {
    SUITE_BINARIES
        .iter()
        .map(|&name| BinaryStatus { name, present: which(name) })
        .collect()
}

/// Whether `name` resolves to an executable on `$PATH`. An in-process `which(1)`:
/// scan `$PATH` entries for an executable file, no fork.
fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name)))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A throwaway data dir, unique per test, cleaned on drop.
    struct TempData {
        dir: PathBuf,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let mut dir = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            dir.push(format!("conductor-test-{tag}-{nanos}"));
            std::fs::create_dir_all(&dir).unwrap();
            TempData { dir }
        }
        fn data(&self) -> DataDir {
            DataDir::new(self.dir.clone())
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::File::create(p).unwrap().write_all(body.as_bytes()).unwrap();
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn missing_snapshot_reads_as_no_feeds_not_a_panic() {
        let t = TempData::new("missing");
        let (built, feeds) = read_feeds(&t.data());
        assert!(built.is_none());
        assert!(feeds.is_empty());
    }

    #[test]
    fn malformed_snapshot_reads_as_no_feeds() {
        let t = TempData::new("malformed");
        t.write("rexops/feeds/workstate.snapshot.json", "{ not json ");
        let (built, feeds) = read_feeds(&t.data());
        assert!(built.is_none());
        assert!(feeds.is_empty());
    }

    #[test]
    fn snapshot_freshness_maps_each_section_status() {
        let t = TempData::new("fresh");
        t.write(
            "rexops/feeds/workstate.snapshot.json",
            r#"{
              "schema_version": 4,
              "built_at": "2026-06-14T12:00:00Z",
              "scripts":  { "status": "Fresh" },
              "tools":    { "status": "Stale" },
              "findings": { "status": { "UnsupportedVersion": { "found": null, "supported": 1 } } }
            }"#,
        );
        let (built, feeds) = read_feeds(&t.data());
        assert_eq!(built.as_deref(), Some("2026-06-14T12:00:00Z"));
        assert_eq!(feeds.len(), 3);
        assert_eq!(feeds[0].name, "scripts");
        assert_eq!(feeds[0].freshness, Freshness::Current);
        assert_eq!(feeds[1].freshness, Freshness::Stale);
        assert_eq!(feeds[2].freshness, Freshness::Unavailable);
    }

    #[test]
    fn binary_probe_covers_the_suite_and_detects_a_real_binary() {
        let checks = read_binaries();
        assert_eq!(checks.len(), SUITE_BINARIES.len());
        // `sh` is on every PATH — sanity-check the probe itself without depending
        // on suite tools being installed.
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-binary-xyzzy"));
    }
}
```

- [ ] **Step 2: Wire the module**

Modify `crates/conductor/src/lib.rs`, add after `pub mod state;`:

```rust
pub mod sources;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p conductor sources::`
Expected: PASS — all reader/probe tests green.

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/sources.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): fault-tolerant sources — feed freshness + \$PATH binary probe"
```

---

## Task 4: Sources — findings, drift, failed jobs

**Files:**
- Modify: `crates/conductor/src/sources.rs` (append readers + tests)

**Interfaces:**
- Consumes: `state::{Finding, Severity, DriftedPath, FailedJob}`; the `DataDir`, `read_json`, and `TempData` test helper from Task 3.
- Produces:
  - `pub fn read_findings(dir: &DataDir) -> Vec<Finding>` — prefers the RexOps aggregate snapshot's `attention`; falls back to the Bulwark feed (high/critical only) when the aggregate is absent. Sorted highest-severity first.
  - `pub fn read_failed_jobs(dir: &DataDir) -> Vec<FailedJob>` — Proto sessions whose outcome is failed.
  - `pub fn read_drift(dir: &DataDir) -> Vec<DriftedPath>` — Phase 1 has no tripwire feed file in the contract set, so this reads an **optional** `tripwire/drift.json` if present (a list of paths) and otherwise returns `vec![]`. Documented as the single point that becomes a real tripwire contract later; returning empty keeps rule 5 dormant until then.

- [ ] **Step 1: Write the failing tests (append to the existing `tests` module)**

Add these readers above the `#[cfg(test)]` module in `crates/conductor/src/sources.rs`:

```rust
// ── Findings — RexOps aggregate snapshot, else Bulwark feed ───────────────────

use crate::state::{DriftedPath, FailedJob, Finding, Severity};

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "low" => Some(Severity::Low),
        "medium" => Some(Severity::Medium),
        "high" => Some(Severity::High),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

#[derive(Deserialize)]
struct RexopsSnapshot {
    #[serde(default)]
    attention: Vec<RexopsAttention>,
}

#[derive(Deserialize)]
struct RexopsAttention {
    #[serde(default)]
    tool: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    severity: String,
}

#[derive(Deserialize)]
struct BulwarkFeed {
    #[serde(default)]
    items: Vec<BulwarkItem>,
}

#[derive(Deserialize)]
struct BulwarkItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    description: String,
}

/// Read findings, preferring the RexOps aggregate (richest single input). When
/// the aggregate is absent, fall back to the Bulwark feed and surface only its
/// high/critical items (low/medium inventory noise is left out of the plan).
/// Always sorted highest-severity first.
pub fn read_findings(dir: &DataDir) -> Vec<Finding> {
    let mut findings: Vec<Finding> = if let Some(snap) =
        read_json::<RexopsSnapshot>(&dir.rexops_snapshot())
    {
        snap.attention
            .into_iter()
            .map(|a| Finding {
                what: if a.id.is_empty() { a.tool.clone() } else { a.id },
                why: a.reason,
                source: a.tool,
                severity: parse_severity(&a.severity).unwrap_or(Severity::Low),
            })
            .collect()
    } else if let Some(feed) = read_json::<BulwarkFeed>(&dir.bulwark_feed()) {
        feed.items
            .into_iter()
            .filter_map(|it| {
                let sev = parse_severity(&it.severity)?;
                if sev < Severity::High {
                    return None;
                }
                Some(Finding {
                    what: if it.name.is_empty() { it.id } else { it.name },
                    why: if it.description.is_empty() {
                        "flagged by bulwark".to_string()
                    } else {
                        it.description
                    },
                    source: "bulwark".to_string(),
                    severity: sev,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    // Highest severity first; stable so equal severities keep producer order.
    findings.sort_by(|a, b| b.severity.cmp(&a.severity));
    findings
}

// ── Failed jobs — Proto sessions ─────────────────────────────────────────────

#[derive(Deserialize)]
struct ProtoSession {
    #[serde(default)]
    protocol_title: Option<String>,
    #[serde(default)]
    steps: Vec<ProtoStep>,
}

#[derive(Deserialize)]
struct ProtoStep {
    #[serde(default)]
    status: String,
}

/// Read every Proto session and keep only those with a failed step. Unreadable
/// individual files are skipped; a missing directory yields an empty list.
pub fn read_failed_jobs(dir: &DataDir) -> Vec<FailedJob> {
    let Ok(entries) = std::fs::read_dir(dir.proto_sessions()) else {
        return Vec::new();
    };
    let mut jobs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(s): Option<ProtoSession> = read_json(&path) else {
            continue;
        };
        if s.steps.iter().any(|st| st.status == "failed") {
            jobs.push(FailedJob {
                title: s.protocol_title.unwrap_or_else(|| "protocol run".to_string()),
            });
        }
    }
    jobs
}

// ── Drift — optional tripwire feed ───────────────────────────────────────────

#[derive(Deserialize)]
struct TripwireDrift {
    #[serde(default)]
    paths: Vec<String>,
}

/// Read drifted paths from an optional `tripwire/drift.json`. Tripwire has no
/// published Workstate feed in the contract set yet, so this is the single point
/// that becomes a real contract later; until the file exists, it returns empty
/// and rule 5 (drift×finding correlation) stays dormant — never an error. Uses
/// `DataDir::tripwire_drift()` (added in Task 3) and `root()` is available if a
/// further uncontracted path is ever needed.
pub fn read_drift(dir: &DataDir) -> Vec<DriftedPath> {
    match read_json::<TripwireDrift>(&dir.tripwire_drift()) {
        Some(d) => d.paths.into_iter().map(|p| DriftedPath { path: p }).collect(),
        None => Vec::new(),
    }
}
```

The `tripwire_drift()` and `pub(crate) fn root()` methods were added to `DataDir` in Task 3, so `read_drift` is a plain reader like the others — no private-field reconstruction.

Append these tests inside the existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn findings_prefer_rexops_and_sort_by_severity() {
        let t = TempData::new("findings");
        t.write(
            "rexops/snapshot.json",
            r#"{
              "schema_version": 1, "source_tool": "rexops",
              "attention": [
                { "tool": "toolfoundry", "id": "backup-home", "reason": "health failing", "severity": "high" },
                { "tool": "bulwark", "id": "deploy-prod.sh", "reason": "AWS key", "severity": "critical" }
              ]
            }"#,
        );
        let f = read_findings(&t.data());
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].severity, Severity::Critical);
        assert_eq!(f[0].what, "deploy-prod.sh");
        assert_eq!(f[0].source, "bulwark");
    }

    #[test]
    fn findings_fall_back_to_bulwark_high_and_critical_only() {
        let t = TempData::new("bulwark");
        t.write(
            "workstate/feeds/bulwark.json",
            r#"{
              "schema_version": 1, "source_tool": "bulwark", "item_count": 3,
              "items": [
                { "name": "low.sh",  "severity": "low",      "description": "noise" },
                { "name": "hi.sh",   "severity": "high",     "description": "exec bit on secret" },
                { "name": "crit.sh", "severity": "critical", "description": "private key committed" }
              ]
            }"#,
        );
        let f = read_findings(&t.data());
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity >= Severity::High));
        assert_eq!(f[0].severity, Severity::Critical);
    }

    #[test]
    fn no_findings_anywhere_reads_as_empty() {
        let t = TempData::new("nofind");
        assert!(read_findings(&t.data()).is_empty());
    }

    #[test]
    fn failed_jobs_keep_only_failures() {
        let t = TempData::new("jobs");
        t.write(
            "proto/sessions/a.json",
            r#"{ "protocol_title":"Passed Run", "steps":[{"status":"passed"}] }"#,
        );
        t.write(
            "proto/sessions/b.json",
            r#"{ "protocol_title":"Failed Run", "steps":[{"status":"failed"}] }"#,
        );
        t.write("proto/sessions/notes.txt", "ignore me");
        let jobs = read_failed_jobs(&t.data());
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, "Failed Run");
    }

    #[test]
    fn drift_is_empty_when_absent_and_parsed_when_present() {
        let t = TempData::new("drift");
        assert!(read_drift(&t.data()).is_empty());
        t.write("tripwire/drift.json", r#"{ "paths": ["deploy-prod.sh", "etc/hosts"] }"#);
        let d = read_drift(&t.data());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].path, "deploy-prod.sh");
    }
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p conductor sources::`
Expected: PASS — Task 3 tests plus the five new ones.

- [ ] **Step 3: Commit**

```bash
git add crates/conductor/src/sources.rs
git commit -m "feat(conductor): sources — findings (rexops/bulwark), failed jobs, optional drift"
```

---

## Task 5: load_state — assemble the SuiteState from all readers

**Files:**
- Modify: `crates/conductor/src/lib.rs` (add `load_state`)

**Interfaces:**
- Consumes: `sources::{DataDir, read_feeds, read_binaries, read_findings, read_failed_jobs, read_drift}`; `state::SuiteState`; `error::ConductorError`.
- Produces: `pub fn load_state(dir: &sources::DataDir) -> SuiteState` — runs every reader and fills a `SuiteState`. Pure aggregation, no rules. (No `Result`: readers never fail; only `DataDir::from_env` can fail, and that's the caller's concern.)

- [ ] **Step 1: Write the failing test**

Replace `crates/conductor/src/lib.rs` with:

```rust
//! conductor — the Linux Ops Suite's guided operator.
//!
//! Phase 1 (this build) is the Ring 0, read-only foundation: read the suite's
//! contract files, derive a deterministic ordered plan, and render it. The
//! library does the work and returns values; the binary only parses flags and
//! prints. See `CONDUCTOR_DESIGN.md` at the repo root.

pub mod error;
pub mod sources;
pub mod state;
pub mod util;

pub use error::ConductorError;
use state::SuiteState;

/// Assemble the normalized suite state by running every fault-tolerant reader.
/// Pure aggregation: no rules, no rendering. Never fails — a missing feed just
/// yields fewer facts (resolving `DataDir` is the only fallible step, done by the
/// caller via [`sources::DataDir::from_env`]).
pub fn load_state(dir: &sources::DataDir) -> SuiteState {
    let (built_at, feeds) = sources::read_feeds(dir);
    SuiteState {
        built_at,
        feeds,
        findings: sources::read_findings(dir),
        drift: sources::read_drift(dir),
        failed_jobs: sources::read_failed_jobs(dir),
        binaries: sources::read_binaries(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("conductor-loadstate-{tag}-{nanos}"));
        dir
    }

    fn write(root: &PathBuf, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(p).unwrap().write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn load_state_aggregates_every_reader() {
        let root = temp_root("agg");
        write(
            &root,
            "rexops/feeds/workstate.snapshot.json",
            r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
        );
        write(
            &root,
            "rexops/snapshot.json",
            r#"{ "attention": [ { "tool":"bulwark","id":"x.sh","reason":"key","severity":"critical" } ] }"#,
        );
        let dir = sources::DataDir::new(root.clone());
        let s = load_state(&dir);
        assert_eq!(s.built_at.as_deref(), Some("2026-06-14T12:00:00Z"));
        assert!(s.has_stale_or_unavailable_feed());
        assert_eq!(s.findings.len(), 1);
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_state_on_empty_root_is_all_empty_but_for_binaries() {
        let root = temp_root("empty");
        std::fs::create_dir_all(&root).unwrap();
        let dir = sources::DataDir::new(root.clone());
        let s = load_state(&dir);
        assert!(s.feeds.is_empty());
        assert!(s.findings.is_empty());
        assert!(s.failed_jobs.is_empty());
        assert!(s.drift.is_empty());
        // binaries are always probed (presence may vary by machine)
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
        let _ = std::fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p conductor`
Expected: PASS — all prior tests plus the two `load_state` tests.

- [ ] **Step 3: Commit**

```bash
git add crates/conductor/src/lib.rs
git commit -m "feat(conductor): load_state — assemble SuiteState from every reader"
```

---

## Task 6: Plan types — Plan, Step, Ring, StepStatus

**Files:**
- Create: `crates/conductor/src/plan/mod.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod plan;`)

**Interfaces:**
- Consumes: nothing (pure types). `build` is added in Task 7.
- Produces:
  - `enum Ring { ReadOnly, ChangesState, Info }` — Info = a Ring 0 informational step (e.g. a fix command) that runs nothing.
  - `enum StepStatus { Pending, Done, Skipped }` (Phase 1 only ever constructs `Pending`; the others exist for Phase 2/3).
  - `struct Step { pub title: String, pub command: Option<String>, pub ring: Ring, pub annotation: Option<String>, pub status: StepStatus }` with `pub fn new(title, command: Option<String>, ring) -> Self` (status defaults to `Pending`, annotation `None`) and `pub fn annotated(self, note: impl Into<String>) -> Self`.
  - `struct Plan { pub situation: Vec<String>, pub steps: Vec<Step> }` with `pub fn is_empty(&self) -> bool` (true when there are no steps).
  - `enum Ring { … }` derives `PartialEq, Eq, Clone, Copy, Debug`; `StepStatus` likewise.

- [ ] **Step 1: Write the failing test**

Create `crates/conductor/src/plan/mod.rs`:

```rust
//! The Plan and its parts. A `Plan` is conductor's whole output: a short
//! `situation` (why there's a plan) and an ordered list of `Step`s. Each step
//! carries its literal `command`, its `ring` (what running it would do), an
//! optional correlation `annotation`, and a `status`. These are *types only*;
//! the rules that fill them live in [`rules`], and rendering lives in `report`.

pub mod rules;

/// What running a step would do — the safety classification from the design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// Spawns a sibling that only reads (Ring 1).
    ReadOnly,
    /// Spawns a sibling that writes (Ring 2) — requires a confirm in Phase 3.
    ChangesState,
    /// Conductor's own informational step (Ring 0): shows a fix/next command but
    /// runs nothing itself.
    Info,
}

impl Ring {
    /// The short tag rendered at the step's right edge.
    pub fn tag(self) -> &'static str {
        match self {
            Ring::ReadOnly => "read-only",
            Ring::ChangesState => "changes state",
            Ring::Info => "info",
        }
    }
}

/// A step's lifecycle. Phase 1 only ever produces `Pending`; `Done`/`Skipped`
/// are driven by the Phase 2/3 TUI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepStatus {
    Pending,
    Done,
    Skipped,
}

/// One ordered action in the plan.
#[derive(Clone, Debug)]
pub struct Step {
    pub title: String,
    /// The literal command conductor would spawn (shown verbatim), or `None` for
    /// a pure-prose step.
    pub command: Option<String>,
    pub ring: Ring,
    /// A correlation note (rule 5), rendered inline (e.g. "same file as drift").
    pub annotation: Option<String>,
    pub status: StepStatus,
}

impl Step {
    pub fn new(title: impl Into<String>, command: Option<String>, ring: Ring) -> Self {
        Step {
            title: title.into(),
            command,
            ring,
            annotation: None,
            status: StepStatus::Pending,
        }
    }

    /// Attach a correlation annotation, builder-style.
    pub fn annotated(mut self, note: impl Into<String>) -> Self {
        self.annotation = Some(note.into());
        self
    }
}

/// Conductor's complete output for a run.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub situation: Vec<String>,
    pub steps: Vec<Step>,
}

impl Plan {
    /// True when there is nothing to conduct.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_tags_are_words_so_color_is_never_load_bearing() {
        assert_eq!(Ring::ReadOnly.tag(), "read-only");
        assert_eq!(Ring::ChangesState.tag(), "changes state");
        assert_eq!(Ring::Info.tag(), "info");
    }

    #[test]
    fn new_step_defaults_to_pending_and_unannotated() {
        let s = Step::new("do a thing", Some("pulse".to_string()), Ring::ReadOnly);
        assert_eq!(s.status, StepStatus::Pending);
        assert!(s.annotation.is_none());
        assert_eq!(s.command.as_deref(), Some("pulse"));
    }

    #[test]
    fn annotated_attaches_a_note() {
        let s = Step::new("investigate x", None, Ring::ReadOnly).annotated("same file as drift");
        assert_eq!(s.annotation.as_deref(), Some("same file as drift"));
    }

    #[test]
    fn empty_plan_reports_empty() {
        assert!(Plan::default().is_empty());
    }
}
```

- [ ] **Step 2: Create a placeholder `rules` module so `mod.rs` compiles**

Create `crates/conductor/src/plan/rules.rs` with a temporary doc + empty body (filled in Task 7):

```rust
//! The v1 rules: pure helpers turning a `SuiteState` into ordered `Step`s, in
//! priority order. Filled in Task 7. See CONDUCTOR_DESIGN.md "How Conductor
//! Builds the Plan".
```

- [ ] **Step 3: Wire the module**

Modify `crates/conductor/src/lib.rs`, add after `pub mod state;`:

```rust
pub mod plan;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p conductor plan::`
Expected: PASS — the four `plan` type tests.

- [ ] **Step 5: Commit**

```bash
git add crates/conductor/src/plan/mod.rs crates/conductor/src/plan/rules.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): Plan/Step/Ring/StepStatus types"
```

---

## Task 7: The rule engine — build(&SuiteState) -> Plan (the brain)

**Files:**
- Modify: `crates/conductor/src/plan/rules.rs` (the rules + the densest tests)
- Modify: `crates/conductor/src/plan/mod.rs` (add the public `build`)

**Interfaces:**
- Consumes: `crate::state::{SuiteState, Freshness, Severity}`; `super::{Plan, Step, Ring}`.
- Produces: `pub fn build(state: &SuiteState) -> Plan` (in `mod.rs`), delegating step construction to the ordered rule helpers in `rules.rs`.

The rules, in the exact priority order from the design (CONDUCTOR_DESIGN.md "The v1 rules"):
1. stale/unavailable feed ⇒ a `ChangesState` step `workstate snapshot`.
2. each missing binary ⇒ an `Info` step with the fix command.
3. if the plan will contain any `ChangesState` step from rules ≥4's group **other than the refresh**, prepend a `ChangesState` safety-capture step `rewind capture --label pre-conductor`. (In Phase 1 only rule 1 and this rule produce `ChangesState`; rules 4/6 are read-only. So the trigger for the safety capture is: there is at least one finding or failed job to act on, i.e. the plan will guide real work.) Insert it **after** the refresh (rule 1) and **before** the findings.
4. each critical/high finding ⇒ a `ReadOnly` investigate step `bulwark show <what>`, highest severity first.
5. if a finding's `what` matches a drifted path, move that finding's step to the front of the findings group and annotate it.
6. each failed job ⇒ a `ReadOnly` review step `proto show <title>`.
7. nothing fired ⇒ empty plan (`is_empty()` true).

Situation lines: add a line for a stale feed ("workstate snapshot is stale — refresh before trusting feeds") and for a correlated finding ("N finding(s) correlate with tripwire drift").

- [ ] **Step 1: Write the failing tests**

Replace `crates/conductor/src/plan/rules.rs` with:

```rust
//! The v1 rules: pure helpers turning a `SuiteState` into ordered `Step`s, in
//! priority order. This is the product's brain — deterministic (same state ⇒
//! same plan) and the densest test module in the crate. Each rule is a small
//! function with its precondition; `super::build` runs them in order and
//! assembles the `Plan`. See CONDUCTOR_DESIGN.md "How Conductor Builds the Plan".

use super::{Plan, Ring, Step};
use crate::state::{Severity, SuiteState};

/// Rule 1 — trust the data first. A stale/unavailable feed means every later
/// step reads possibly-wrong data, so refresh first. One step regardless of how
/// many feeds are stale (a single `workstate snapshot` refreshes them).
pub(super) fn refresh_stale_feeds(state: &SuiteState) -> Option<Step> {
    if state.has_stale_or_unavailable_feed() {
        Some(Step::new(
            "refresh stale data",
            Some("workstate snapshot".to_string()),
            Ring::ChangesState,
        ))
    } else {
        None
    }
}

/// Rule 2 — wiring gaps. Each suite binary missing from `$PATH` becomes an Info
/// step naming the one command that fixes it. Informational because conductor
/// can't install for you.
pub(super) fn wiring_gaps(state: &SuiteState) -> Vec<Step> {
    state
        .missing_binaries()
        .into_iter()
        .map(|b| {
            Step::new(
                format!("{} is not on PATH", b.name),
                Some(format!("install.sh --only {}", b.name)),
                Ring::Info,
            )
        })
        .collect()
}

/// Rule 4 + 5 — investigate findings, worst first, with the drift-correlated one
/// pulled to the front and annotated. Returns the ordered investigate steps.
pub(super) fn investigate_findings(state: &SuiteState) -> Vec<Step> {
    // findings arrive already sorted worst-first (sources::read_findings).
    let drifted: Vec<&str> = state.drift.iter().map(|d| d.path.as_str()).collect();

    let mut correlated: Vec<Step> = Vec::new();
    let mut rest: Vec<Step> = Vec::new();
    for f in &state.findings {
        if f.severity < Severity::High {
            continue;
        }
        let mut step = Step::new(
            format!("investigate {}", f.what),
            Some(format!("bulwark show {}", f.what)),
            Ring::ReadOnly,
        );
        if drifted.iter().any(|p| *p == f.what) {
            step = step.annotated("same file as tripwire drift — start here");
            correlated.push(step);
        } else {
            rest.push(step);
        }
    }
    correlated.extend(rest);
    correlated
}

/// Rule 6 — review failed jobs (read-only).
pub(super) fn review_failed_jobs(state: &SuiteState) -> Vec<Step> {
    state
        .failed_jobs
        .iter()
        .map(|j| {
            Step::new(
                format!("review failed job: {}", j.title),
                Some(format!("proto show {}", j.title)),
                Ring::ReadOnly,
            )
        })
        .collect()
}

/// Rule 3 — capture before you change. Prepended only when the plan will guide
/// real work (≥1 finding or failed job), so a pure refresh-only plan doesn't
/// force a capture. Returns the safety-capture step.
pub(super) fn safety_capture() -> Step {
    Step::new(
        "capture a safety point",
        Some("rewind capture --label pre-conductor".to_string()),
        Ring::ChangesState,
    )
}

/// The human "situation" lines explaining why there's a plan.
pub(super) fn situation(state: &SuiteState) -> Vec<String> {
    let mut lines = Vec::new();
    if state.has_stale_or_unavailable_feed() {
        lines.push("workstate snapshot is stale — refresh before trusting feeds".to_string());
    }
    let drifted: Vec<&str> = state.drift.iter().map(|d| d.path.as_str()).collect();
    let correlated = state
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::High && drifted.iter().any(|p| *p == f.what))
        .count();
    if correlated > 0 {
        let noun = if correlated == 1 { "finding correlates" } else { "findings correlate" };
        lines.push(format!("{correlated} {noun} with a tripwire drift"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{DriftedPath, FailedJob, FeedStatus, Finding, Freshness, Severity, SuiteState};

    fn finding(what: &str, sev: Severity) -> Finding {
        Finding { what: what.into(), why: "because".into(), source: "bulwark".into(), severity: sev }
    }

    #[test]
    fn healthy_state_yields_no_steps() {
        let plan = super::super::build(&SuiteState::empty());
        assert!(plan.is_empty());
        assert!(plan.situation.is_empty());
    }

    #[test]
    fn stale_feed_emits_a_single_refresh_first() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.feeds.push(FeedStatus { name: "scripts", freshness: Freshness::Unavailable });
        let plan = super::super::build(&s);
        assert_eq!(plan.steps[0].title, "refresh stale data");
        assert_eq!(plan.steps[0].ring, Ring::ChangesState);
        assert_eq!(plan.steps[0].command.as_deref(), Some("workstate snapshot"));
        // one refresh, not one per stale feed
        assert_eq!(plan.steps.iter().filter(|s| s.title == "refresh stale data").count(), 1);
    }

    #[test]
    fn refresh_only_plan_does_not_force_a_safety_capture() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        let plan = super::super::build(&s);
        assert!(!plan.steps.iter().any(|s| s.title == "capture a safety point"));
    }

    #[test]
    fn findings_get_a_safety_capture_after_refresh_and_before_investigation() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(finding("deploy-prod.sh", Severity::Critical));
        let plan = super::super::build(&s);
        let titles: Vec<&str> = plan.steps.iter().map(|s| s.title.as_str()).collect();
        let refresh = titles.iter().position(|t| *t == "refresh stale data").unwrap();
        let capture = titles.iter().position(|t| *t == "capture a safety point").unwrap();
        let investigate = titles.iter().position(|t| t.starts_with("investigate")).unwrap();
        assert!(refresh < capture, "capture must follow refresh");
        assert!(capture < investigate, "capture must precede investigation");
    }

    #[test]
    fn findings_are_worst_first_and_read_only() {
        let mut s = SuiteState::empty();
        // deliberately out of order; read_findings would sort, but build must not
        // rely on that for ordering within its own group beyond severity.
        s.findings.push(finding("hi.sh", Severity::High));
        s.findings.push(finding("crit.sh", Severity::Critical));
        // emulate sources' worst-first contract:
        s.findings.sort_by(|a, b| b.severity.cmp(&a.severity));
        let plan = super::super::build(&s);
        let investigate: Vec<&Step> =
            plan.steps.iter().filter(|s| s.title.starts_with("investigate")).collect();
        assert_eq!(investigate[0].title, "investigate crit.sh");
        assert!(investigate.iter().all(|s| s.ring == Ring::ReadOnly));
    }

    #[test]
    fn drift_correlation_lifts_and_annotates_the_matching_finding() {
        let mut s = SuiteState::empty();
        s.findings.push(finding("crit.sh", Severity::Critical));
        s.findings.push(finding("deploy-prod.sh", Severity::High));
        s.findings.sort_by(|a, b| b.severity.cmp(&a.severity));
        s.drift.push(DriftedPath { path: "deploy-prod.sh".into() });
        let plan = super::super::build(&s);
        let investigate: Vec<&Step> =
            plan.steps.iter().filter(|s| s.title.starts_with("investigate")).collect();
        // correlated finding jumps to the front of the group despite lower severity
        assert_eq!(investigate[0].title, "investigate deploy-prod.sh");
        assert_eq!(
            investigate[0].annotation.as_deref(),
            Some("same file as tripwire drift — start here")
        );
        assert!(plan.situation.iter().any(|l| l.contains("correlate")));
    }

    #[test]
    fn missing_binary_emits_an_info_fix_step() {
        let mut s = SuiteState::empty();
        s.binaries.push(crate::state::BinaryStatus { name: "rewind", present: false });
        let plan = super::super::build(&s);
        let fix = plan.steps.iter().find(|s| s.title.contains("rewind")).unwrap();
        assert_eq!(fix.ring, Ring::Info);
        assert_eq!(fix.command.as_deref(), Some("install.sh --only rewind"));
    }

    #[test]
    fn failed_job_emits_a_read_only_review_step() {
        let mut s = SuiteState::empty();
        s.failed_jobs.push(FailedJob { title: "nightly-backup".into() });
        let plan = super::super::build(&s);
        let review = plan.steps.iter().find(|s| s.title.contains("nightly-backup")).unwrap();
        assert_eq!(review.ring, Ring::ReadOnly);
        assert_eq!(review.command.as_deref(), Some("proto show nightly-backup"));
    }

    #[test]
    fn full_plan_orders_groups_correctly() {
        // refresh → wiring → capture → findings → jobs
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.binaries.push(crate::state::BinaryStatus { name: "portman", present: false });
        s.findings.push(finding("crit.sh", Severity::Critical));
        s.failed_jobs.push(FailedJob { title: "backup".into() });
        let plan = super::super::build(&s);
        let titles: Vec<&str> = plan.steps.iter().map(|s| s.title.as_str()).collect();
        let pos = |needle: &str| titles.iter().position(|t| t.contains(needle)).unwrap();
        assert!(pos("refresh stale data") < pos("not on PATH"));
        assert!(pos("not on PATH") < pos("capture a safety point"));
        assert!(pos("capture a safety point") < pos("investigate"));
        assert!(pos("investigate") < pos("review failed job"));
    }
}
```

- [ ] **Step 2: Implement `build` in `mod.rs`**

Add to `crates/conductor/src/plan/mod.rs`, after the `Plan` impl block:

```rust
/// Build the runbook from suite state by running the v1 rules in priority order
/// (CONDUCTOR_DESIGN.md). Deterministic: the same state always yields the same
/// plan. Order: refresh → wiring fixes → safety-capture (only if real work
/// follows) → investigate findings (drift-correlated first) → review failed jobs.
pub fn build(state: &crate::state::SuiteState) -> Plan {
    let mut steps: Vec<Step> = Vec::new();

    // 1. trust the data first
    if let Some(refresh) = rules::refresh_stale_feeds(state) {
        steps.push(refresh);
    }
    // 2. wiring gaps
    steps.extend(rules::wiring_gaps(state));

    // 4/5 + 6 computed up front so rule 3 knows whether real work follows
    let findings = rules::investigate_findings(state);
    let jobs = rules::review_failed_jobs(state);

    // 3. capture before you change — only when the plan guides real work
    if !findings.is_empty() || !jobs.is_empty() {
        steps.push(rules::safety_capture());
    }
    // 4/5
    steps.extend(findings);
    // 6
    steps.extend(jobs);

    Plan {
        situation: rules::situation(state),
        steps,
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p conductor plan::`
Expected: PASS — all rule-engine tests green (this is the heaviest module).

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/plan/rules.rs crates/conductor/src/plan/mod.rs
git commit -m "feat(conductor): the v1 rule engine — deterministic state -> ordered plan"
```

---

## Task 8: report.rs — human + JSON rendering for status / health / plan

**Files:**
- Create: `crates/conductor/src/report.rs`
- Modify: `crates/conductor/src/lib.rs` (add `pub mod report;` and re-exports)

**Interfaces:**
- Consumes: `state::{SuiteState, Freshness}`; `plan::{Plan, Step, Ring}`; `util::stdout_is_tty`.
- Produces:
  - `struct Style { … }` with `pub fn resolve(force_off: bool) -> Self` (same shape as rewind's `Style`).
  - `pub fn print_status(plan: &Plan, built_at: Option<&str>, style: &Style) -> String` — human "situation + plan" text (the `status` verb); for an empty plan, the "nothing to conduct" text.
  - `pub fn print_plan(plan: &Plan, style: &Style) -> String` — just the ordered steps (the `plan` verb), no situation prose.
  - `pub fn print_health(state: &SuiteState, style: &Style) -> String` — per-feed/per-binary readiness lines (the `health` verb).
  - `pub fn status_json(plan: &Plan, built_at: Option<&str>) -> String` and `pub fn health_json(state: &SuiteState) -> String` — the suite envelope (`schema_version: 1`, `source_tool: "conductor"`).
  - `pub fn health_exit_code(state: &SuiteState) -> u8` — `0` when all feeds current and all binaries present, else `0` still (Phase 1 keeps health informational; reserve non-zero for a later policy). Document this explicitly.

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/report.rs`:

```rust
//! Rendering. Turns a `Plan` (and the raw `SuiteState`, for `health`) into human
//! text or the suite's JSON envelope. Color follows the suite rule (TTY +
//! `NO_COLOR`, force-off via `--no-color`); structure is plain and reads the same
//! with color stripped — state is carried by word and glyph, never color alone.
//! The library does the work; these functions only present it. Mirrors rewind's
//! `report.rs`.

use serde::Serialize;

use crate::plan::{Plan, Ring, Step, StepStatus};
use crate::state::{Freshness, SuiteState};
use crate::util;

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as rewind/pulse.
pub struct Style {
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub grn: &'static str,
    pub ylw: &'static str,
    pub cyn: &'static str,
    pub rst: &'static str,
}

impl Style {
    pub fn resolve(force_off: bool) -> Self {
        let on = !force_off && util::stdout_is_tty() && std::env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                red: "\u{1b}[31m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Style { bold: "", dim: "", red: "", grn: "", ylw: "", cyn: "", rst: "" }
        }
    }

    #[cfg(test)]
    fn plain() -> Self {
        Self::resolve(true)
    }
}

/// Color for a ring tag: amber for state changes, dim for read-only/info.
fn ring_color<'a>(ring: Ring, style: &'a Style) -> &'a str {
    match ring {
        Ring::ChangesState => style.ylw,
        Ring::ReadOnly | Ring::Info => style.dim,
    }
}

/// The glyph for a step's status: ▸ current is decided by the TUI, so the
/// one-shot renderer marks every pending step with ○.
fn status_glyph(status: StepStatus) -> char {
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
    }
}

/// One step block: "  ○ N  <title>  [← annotation]" then the dim command line
/// with its ring tag.
fn render_step(out: &mut String, n: usize, step: &Step, style: &Style) {
    let glyph = status_glyph(step.status);
    out.push_str(&format!("  {glyph} {n}  {}", step.title));
    if let Some(note) = &step.annotation {
        out.push_str(&format!("  {}← {}{}", style.cyn, note, style.rst));
    }
    out.push('\n');
    if let Some(cmd) = &step.command {
        out.push_str(&format!(
            "       {dim}{cmd}{rst}   {rc}{tag}{rst}\n",
            dim = style.dim,
            cmd = cmd,
            rc = ring_color(step.ring, style),
            tag = step.ring.tag(),
            rst = style.rst,
        ));
    }
}

/// The `status` verb: situation + ordered plan, or the healthy message.
pub fn print_status(plan: &Plan, _built_at: Option<&str>, style: &Style) -> String {
    if plan.is_empty() {
        return format!(
            "{grn}nothing to conduct{rst}\nthe suite is healthy and every feed is current\n",
            grn = style.grn,
            rst = style.rst,
        );
    }
    let mut out = String::new();
    if !plan.situation.is_empty() {
        out.push_str(&format!("{}the situation{}\n", style.bold, style.rst));
        for line in &plan.situation {
            out.push_str(&format!("  {line}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{}the plan{}   {} steps\n",
        style.bold,
        style.rst,
        plan.steps.len()
    ));
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, style);
    }
    out
}

/// The `plan` verb: just the ordered steps, no situation prose.
pub fn print_plan(plan: &Plan, style: &Style) -> String {
    if plan.is_empty() {
        return format!("{}nothing to conduct{}\n", style.grn, style.rst);
    }
    let mut out = String::new();
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, style);
    }
    out
}

/// A freshness word for the health view.
fn freshness_word(f: Freshness) -> &'static str {
    match f {
        Freshness::Current => "current",
        Freshness::Stale => "stale",
        Freshness::Unavailable => "unavailable",
    }
}

/// The `health` verb: per-feed and per-binary readiness as conductor sees it.
pub fn print_health(state: &SuiteState, style: &Style) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}feeds{}\n", style.bold, style.rst));
    if state.feeds.is_empty() {
        out.push_str(&format!("  {}none readable{}\n", style.dim, style.rst));
    }
    for f in &state.feeds {
        let color = match f.freshness {
            Freshness::Current => style.grn,
            Freshness::Stale => style.ylw,
            Freshness::Unavailable => style.red,
        };
        out.push_str(&format!(
            "  {:<10} {}{}{}\n",
            f.name,
            color,
            freshness_word(f.freshness),
            style.rst
        ));
    }
    out.push_str(&format!("\n{}tools on PATH{}\n", style.bold, style.rst));
    for b in &state.binaries {
        let (mark, color) = if b.present { ("present", style.grn) } else { ("missing", style.dim) };
        out.push_str(&format!("  {:<12} {}{}{}\n", b.name, color, mark, style.rst));
    }
    out
}

/// Phase 1 keeps `health` informational: it always exits 0. A non-zero policy
/// (e.g. exit 1 on any unavailable feed) is deferred so cron users don't get
/// surprised before the policy is designed. Documented here so the reserved
/// behaviour is explicit.
pub fn health_exit_code(_state: &SuiteState) -> u8 {
    0
}

// ── JSON envelopes ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StepOut {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    ring: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation: Option<String>,
}

impl StepOut {
    fn of(step: &Step) -> Self {
        StepOut {
            title: step.title.clone(),
            command: step.command.clone(),
            ring: step.ring.tag(),
            annotation: step.annotation.clone(),
        }
    }
}

#[derive(Serialize)]
struct StatusEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    built_at: Option<String>,
    situation: Vec<String>,
    step_count: usize,
    steps: Vec<StepOut>,
}

/// The `status`/`plan` JSON envelope.
pub fn status_json(plan: &Plan, built_at: Option<&str>) -> String {
    let env = StatusEnvelope {
        schema_version: 1,
        source_tool: "conductor",
        built_at: built_at.map(|s| s.to_string()),
        situation: plan.situation.clone(),
        step_count: plan.steps.len(),
        steps: plan.steps.iter().map(StepOut::of).collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Serialize)]
struct FeedOut {
    name: &'static str,
    freshness: &'static str,
}

#[derive(Serialize)]
struct BinaryOut {
    name: &'static str,
    present: bool,
}

#[derive(Serialize)]
struct HealthEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    feeds: Vec<FeedOut>,
    tools: Vec<BinaryOut>,
}

/// The `health` JSON envelope.
pub fn health_json(state: &SuiteState) -> String {
    let env = HealthEnvelope {
        schema_version: 1,
        source_tool: "conductor",
        feeds: state
            .feeds
            .iter()
            .map(|f| FeedOut { name: f.name, freshness: freshness_word(f.freshness) })
            .collect(),
        tools: state
            .binaries
            .iter()
            .map(|b| BinaryOut { name: b.name, present: b.present })
            .collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{BinaryStatus, FeedStatus, Finding, Freshness, Severity, SuiteState};

    fn plan_with_findings() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "AWS key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    #[test]
    fn empty_plan_status_says_nothing_to_conduct() {
        let plan = Plan::default();
        let out = print_status(&plan, None, &Style::plain());
        assert!(out.contains("nothing to conduct"));
        // no step chrome on the healthy message
        assert!(!out.contains("the plan"));
    }

    #[test]
    fn status_shows_situation_then_plan_with_commands_and_tags() {
        let out = print_status(&plan_with_findings(), Some("2026-06-14T12:00:00Z"), &Style::plain());
        assert!(out.contains("the situation"));
        assert!(out.contains("the plan"));
        assert!(out.contains("workstate snapshot")); // refresh command shown
        assert!(out.contains("changes state")); // ring tag shown
        assert!(out.contains("bulwark show deploy-prod.sh"));
        assert!(out.contains("read-only"));
    }

    #[test]
    fn plan_verb_omits_situation_prose() {
        let out = print_plan(&plan_with_findings(), &Style::plain());
        assert!(!out.contains("the situation"));
        assert!(out.contains("workstate snapshot"));
    }

    #[test]
    fn health_lists_feeds_and_tools() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.binaries.push(BinaryStatus { name: "pulse", present: true });
        s.binaries.push(BinaryStatus { name: "rewind", present: false });
        let out = print_health(&s, &Style::plain());
        assert!(out.contains("tools"));
        assert!(out.contains("stale"));
        assert!(out.contains("pulse"));
        assert!(out.contains("present"));
        assert!(out.contains("rewind"));
        assert!(out.contains("missing"));
    }

    #[test]
    fn status_json_is_the_suite_envelope() {
        let json = status_json(&plan_with_findings(), Some("2026-06-14T12:00:00Z"));
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"source_tool\": \"conductor\""));
        assert!(json.contains("\"ring\": \"changes state\""));
        assert!(json.contains("deploy-prod.sh"));
        // valid JSON round-trips
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["step_count"], 3);
    }

    #[test]
    fn health_json_is_the_suite_envelope() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Current });
        s.binaries.push(BinaryStatus { name: "pulse", present: true });
        let json = health_json(&s);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "conductor");
        assert_eq!(v["feeds"][0]["freshness"], "current");
        assert_eq!(v["tools"][0]["present"], true);
    }

    #[test]
    fn no_color_output_has_no_escape_codes() {
        let out = print_status(&plan_with_findings(), None, &Style::plain());
        assert!(!out.contains('\u{1b}'), "plain style must emit no ANSI escapes");
    }
}
```

- [ ] **Step 2: Wire the module + re-exports**

Modify `crates/conductor/src/lib.rs`, add after `pub mod plan;`:

```rust
pub mod report;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p conductor report::`
Expected: PASS — all rendering + envelope tests, including the no-ANSI guarantee.

- [ ] **Step 4: Commit**

```bash
git add crates/conductor/src/report.rs crates/conductor/src/lib.rs
git commit -m "feat(conductor): report.rs — human + JSON rendering for status/health/plan"
```

---

## Task 9: main.rs — the thin CLI (status / health / plan) and an integration test

**Files:**
- Create: `crates/conductor/src/main.rs`
- Create: `crates/conductor/tests/cli.rs` (integration test against the built binary)

**Interfaces:**
- Consumes: the whole library (`conductor::{load_state, sources::DataDir, plan, report, ConductorError}`).
- Produces: the `conductor` binary with subcommands `status` (default), `health`, `plan`, global flags `--json`, `--no-color`, `--data-dir`, `-v/--verbose`, `-h`. Exit codes `0` / `3`.

- [ ] **Step 1: Write the binary**

Create `crates/conductor/src/main.rs`:

```rust
//! conductor CLI. Thin shell: parse flags, dispatch to a read-only subcommand,
//! render human or JSON, exit with a structured code (0 ok / 3 conductor itself
//! could not run). All the work lives in the library; `main` only chooses what to
//! run and how to print it — the same shape as rewind's and pulse's main.
//!
//! Phase 1 surface: `status` (default), `health`, `plan`. The interactive TUI
//! (`conductor` bare) and `orchestrate` arrive in Phases 2–3; until then, bare
//! `conductor` prints `status`, which keeps it useful and scriptable.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use conductor::report::{self, Style};
use conductor::sources::DataDir;
use conductor::{load_state, plan, ConductorError};

/// Given the suite's current state, what should I do — and in what order?
///
/// Conductor reads the suite's own state files, derives a short ordered runbook,
/// and (in later phases) walks you through it. Read-only by default: it never
/// writes a live file itself. With no subcommand, prints the situation + plan.
#[derive(Parser)]
#[command(name = "conductor", version, about, verbatim_doc_comment)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Emit the JSON envelope instead of human output.
    #[arg(long, global = true)]
    json: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long, global = true)]
    no_color: bool,

    /// Show extra detail (reserved for later phases).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Read suite contracts from this directory instead of the XDG default.
    #[arg(long, value_name = "DIR", global = true)]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the situation and the ordered plan (same as no subcommand).
    Status,
    /// Print the suite's readiness as conductor sees it (feeds + tools).
    Health,
    /// Print just the ordered steps, no situation prose.
    Plan,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    let result = match &cli.command {
        None | Some(Cmd::Status) => run_status(&cli, &style),
        Some(Cmd::Health) => run_health(&cli, &style),
        Some(Cmd::Plan) => run_plan(&cli, &style),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("conductor: {err}");
            ExitCode::from(3)
        }
    }
}

/// Resolve the data dir from `--data-dir` or the environment.
fn data_dir(cli: &Cli) -> Result<DataDir, ConductorError> {
    match &cli.data_dir {
        Some(p) => Ok(DataDir::new(p.clone())),
        None => DataDir::from_env(),
    }
}

fn run_status(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    if cli.json {
        println!("{}", report::status_json(&plan, state.built_at.as_deref()));
    } else {
        print!("{}", report::print_status(&plan, state.built_at.as_deref(), style));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_plan(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    if cli.json {
        println!("{}", report::status_json(&plan, state.built_at.as_deref()));
    } else {
        print!("{}", report::print_plan(&plan, style));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_health(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    if cli.json {
        println!("{}", report::health_json(&state));
    } else {
        print!("{}", report::print_health(&state, style));
    }
    Ok(ExitCode::from(report::health_exit_code(&state)))
}
```

- [ ] **Step 2: Write the failing integration test**

Create `crates/conductor/tests/cli.rs`:

```rust
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
        std::fs::File::create(p).unwrap().write_all(body.as_bytes()).unwrap();
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn run(root: &TempRoot, args: &[&str]) -> std::process::Output {
    Command::new(bin())
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
fn json_status_is_the_suite_envelope() {
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
```

Note: `crates/conductor/tests/cli.rs` needs `serde_json` at test time. It is already a normal dependency of the crate, which integration tests can use; no manifest change required.

- [ ] **Step 3: Run the integration tests to verify they pass**

Run: `cargo test -p conductor --test cli`
Expected: PASS — all four CLI tests; binary builds and behaves.

- [ ] **Step 4: Run the full crate test suite**

Run: `cargo test -p conductor`
Expected: PASS — unit + integration tests all green.

- [ ] **Step 5: Commit**

```bash
git add crates/conductor/src/main.rs crates/conductor/tests/cli.rs
git commit -m "feat(conductor): thin CLI (status/health/plan) + end-to-end tests"
```

---

## Task 10: README, installer registration, and a clean full-workspace check

**Files:**
- Create: `crates/conductor/README.md`
- Modify: `install.sh` — add `"conductor:conductor"` to `WORKSPACE_TOOLS`

**Interfaces:** none (docs + packaging).

- [ ] **Step 1: Write the crate README**

Create `crates/conductor/README.md`:

```markdown
# conductor

The Linux Ops Suite's **guided operator**. Conductor reads the suite's own state
files, derives a short **ordered runbook** — *do these things, in this order* —
and (in later phases) walks you through it, delegating each step to the tool that
owns it. It never writes a live file itself.

> Phase 1 (this build) is the read-only foundation: `status`, `health`, `plan`.
> The interactive TUI and the guided `orchestrate` runner land in later phases.

## What it does

- Reads every suite contract (Workstate snapshot, RexOps aggregate, Bulwark
  feed, Proto sessions) fault-tolerantly — a missing or malformed feed becomes
  "unavailable", never a crash.
- Derives a deterministic, ordered plan from built-in rules: refresh stale data
  first, capture a safety point before changes, investigate the worst findings
  (drift-correlated ones first), review failed jobs.
- Prints that plan as calm human text or a JSON envelope.

## Safety

Conductor never mutates state with its own code. Every step is classified by a
**ring**: `read-only` (Ring 1), `changes state` (Ring 2, confirmed before it can
run — Phase 3), or `info` (Ring 0, shows a fix command, runs nothing). Phase 1 is
entirely Ring 0: it reads and renders, and runs nothing.

## Usage

    conductor              print the situation + ordered plan (default)
    conductor status       same as above
    conductor plan         just the ordered steps, no prose
    conductor health       per-feed and per-tool readiness
    conductor --json …     emit the JSON envelope (schema_version + source_tool)
    conductor --no-color   force monochrome
    conductor --data-dir D read suite state from D instead of the XDG default

Exit codes: `0` ok (including "nothing to conduct"); `3` conductor itself could
not run (no data dir). `1`/`2` are reserved for the guided runner.

See `CONDUCTOR_DESIGN.md` at the repo root for the full design.
```

- [ ] **Step 2: Register in the installer**

Modify `install.sh`, the `WORKSPACE_TOOLS` array (around line 58):

```sh
WORKSPACE_TOOLS=(
  "toolbox-bridge:toolbox-bridge"
  "rex-check:rex-check"
  "conductor:conductor"
)
```

- [ ] **Step 3: Verify the installer parses (dry run) and lists conductor**

Run: `bash install.sh --dry-run --local 2>&1 | grep -i conductor`
Expected: at least one line mentioning building/installing `conductor` (proves it's in the build set). If `--local`/`--dry-run` combination differs on this machine, fall back to: `bash -n install.sh` (syntax check) and confirm the array edit by eye.

- [ ] **Step 4: Full workspace build, test, and lint**

Run: `cargo build -p conductor --release`
Expected: builds clean.

Run: `cargo test -p conductor`
Expected: all tests pass.

Run: `cargo clippy -p conductor --all-targets -- -D warnings`
Expected: no warnings. (Fix any clippy nits inline — e.g. needless clones — keeping behaviour identical.)

Run: `cargo fmt -p conductor`
Expected: no diff (or apply formatting).

- [ ] **Step 5: Confirm the whole workspace still builds (no regression from the new member)**

Run: `cargo build --workspace`
Expected: the full suite builds with conductor as a member.

- [ ] **Step 6: Commit**

```bash
git add crates/conductor/README.md install.sh
git commit -m "feat(conductor): README + installer registration; Phase 1 complete"
```

---

## Done — Phase 1 acceptance

When all ten tasks are committed, Phase 1 is complete and:
- `conductor`, `conductor status`, `conductor plan`, `conductor health` work, with `--json`, `--no-color`, `--data-dir`, `-v`.
- All output is Ring 0 (read-only): no subprocess, no file writes, no TUI.
- The rule engine is deterministic and covered by dense unit tests; readers are fault-tolerant and covered by temp-dir tests; the CLI is covered end-to-end.
- The crate is a workspace member and registered in the installer (bare `conductor` binary on `$PATH`, no wrapper/alias).
- Exit codes: `0` ok / `3` can't-run; `1`/`2` reserved for Phase 3.

Phase 2 (the TUI + Ring 1 spawns) and Phase 3 (the `orchestrate` driver + Ring 2 confirm) follow as separate plans.
