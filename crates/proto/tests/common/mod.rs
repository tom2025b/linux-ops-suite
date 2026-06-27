// Each integration-test binary that does `mod common;` compiles this file
// SEPARATELY and only links the helpers it actually calls. A binary that uses,
// say, only MINIMAL_PROTOCOL would otherwise warn that TempDir is "never used".
// `#![allow(dead_code)]` on the module says "this is a shared toolbox; not every
// consumer uses every tool" — the standard idiom for tests/common.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

// -----------------------------------------------------------------------------
// TempDir — a self-cleaning temporary directory.
// -----------------------------------------------------------------------------
// Holds a unique path under the OS temp dir and removes it (recursively) when it
// goes out of scope, so tests don't leak directories. The uniqueness scheme —
// process id + a per-call tag + an atomic counter — keeps concurrent tests in
// the SAME binary from racing on one path (they share a PID), while the PID
// separates this run from other test binaries and earlier runs.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    // Create a fresh, empty temp directory tagged with `tag` for readability.
    pub fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        // A monotonically increasing counter, unique per process. `static` means
        // one shared instance across all calls in this binary.
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed); // unique per call

        let mut path = std::env::temp_dir();
        path.push(format!("proto_test_{}_{}_{}", tag, std::process::id(), n));

        // create_dir_all is fine: the unique name means it won't already exist,
        // but this also creates any missing parent components.
        std::fs::create_dir_all(&path).expect("temp dir must be creatable");
        TempDir { path }
    }

    // Borrow the directory path for passing to loader functions.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // Write a file `name` with `contents` inside this temp dir; return its path.
    // Used to drop protocol YAML (or deliberately-broken files) into the dir.
    pub fn write_file(&self, name: &str, contents: &str) -> PathBuf {
        let file = self.path.join(name);
        std::fs::write(&file, contents).expect("temp file must be writable");
        file
    }
}

// `Drop` is Rust's destructor hook: when a TempDir value goes out of scope at
// the end of a test, this runs and deletes the directory tree. Best-effort —
// if an assertion panics mid-test the unwind still drops locals, so cleanup
// still happens; we ignore the result because a failed cleanup must not mask the
// real test failure.
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// -----------------------------------------------------------------------------
// A minimal valid protocol, as YAML text. Tests that need "any good protocol"
// use this so each test isn't repeating boilerplate.
// -----------------------------------------------------------------------------
pub const MINIMAL_PROTOCOL: &str = "\
id: sample
title: Sample Protocol
steps:
  - id: first
    title: Do the first thing
  - id: second
    title: Do the second thing
    kind: info
";
