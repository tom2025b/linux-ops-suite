//! Core diagnostic types. One [`Check`] is the result of one test; a slice of
//! them rolls up into a [`Verdict`] and a [`Summary`]. Everything the human and
//! JSON renderers print is derived from these — the check functions never print.

use std::fmt;

use serde::Serialize;

/// The outcome of a single check. Ordered worst-last so a slice can be reduced
/// to the worst status with a simple `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Not applicable, or a required input was absent (e.g. a repo isn't
    /// checked out). Never a failure — the suite degrades gracefully.
    Skip,
    /// The check passed.
    Pass,
    /// A non-fatal problem the operator should know about (stale, version
    /// skew). The suite still works.
    Warn,
    /// A real fault that breaks the suite or a contract.
    Fail,
}

impl Status {
    /// Short uppercase tag used in human output (`PASS`/`WARN`/`FAIL`/`SKIP`).
    pub fn tag(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// Which group a check belongs to. The `id` prefix always matches this, so
/// `--only env` / `--skip bin` filter on the same word the output groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Env,
    Bin,
}

impl Category {
    /// The lowercase prefix used in check ids and on the CLI (`env`, `bin`).
    pub fn prefix(self) -> &'static str {
        match self {
            Category::Env => "env",
            Category::Bin => "bin",
        }
    }

    /// Human-readable section heading.
    pub fn title(self) -> &'static str {
        match self {
            Category::Env => "Environment & PATH",
            Category::Bin => "Binaries & versions",
        }
    }

    /// Every category, in display order. The single source of truth for
    /// iteration (output grouping, `--list`, default run order).
    pub fn all() -> &'static [Category] {
        &[Category::Env, Category::Bin]
    }
}

/// One diagnostic result. `fix` is a literal command the operator can run; it
/// is `None` when there is nothing to do (a PASS) or no single obvious fix.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// Stable `category.name` identifier, e.g. `bin.present`. The contract
    /// every other surface (output, `--only`, JSON, docs) refers to.
    pub id: &'static str,
    pub category: Category,
    pub status: Status,
    /// One-line human-readable reason for the status.
    pub detail: String,
    /// A literal command that resolves the finding, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Check {
    /// A passing check with no fix needed.
    pub fn pass(id: &'static str, category: Category, detail: impl Into<String>) -> Self {
        Check {
            id,
            category,
            status: Status::Pass,
            detail: detail.into(),
            fix: None,
        }
    }

    /// A skipped (not-applicable) check.
    pub fn skip(id: &'static str, category: Category, detail: impl Into<String>) -> Self {
        Check {
            id,
            category,
            status: Status::Skip,
            detail: detail.into(),
            fix: None,
        }
    }

    /// A warning with the command that clears it.
    pub fn warn(
        id: &'static str,
        category: Category,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Check {
            id,
            category,
            status: Status::Warn,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }

    /// A failure with the command that fixes it.
    pub fn fail(
        id: &'static str,
        category: Category,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Check {
            id,
            category,
            status: Status::Fail,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
}

/// Counts per status plus the overall verdict — the bottom-line of a run.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Summary {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
}

impl Summary {
    /// Tally a slice of checks.
    pub fn of(checks: &[Check]) -> Self {
        let mut s = Summary {
            pass: 0,
            warn: 0,
            fail: 0,
            skip: 0,
        };
        for c in checks {
            match c.status {
                Status::Pass => s.pass += 1,
                Status::Warn => s.warn += 1,
                Status::Fail => s.fail += 1,
                Status::Skip => s.skip += 1,
            }
        }
        s
    }

    /// The overall verdict: the worst status present. An all-skip (or empty)
    /// run is a Pass — nothing was found to be wrong.
    pub fn verdict(&self) -> Status {
        if self.fail > 0 {
            Status::Fail
        } else if self.warn > 0 {
            Status::Warn
        } else {
            Status::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_orders_worst_last() {
        assert!(Status::Pass > Status::Skip);
        assert!(Status::Warn > Status::Pass);
        assert!(Status::Fail > Status::Warn);
    }

    #[test]
    fn category_prefix_matches_check_id_convention() {
        // The grouping word and the id prefix must agree, or `--only` lies.
        for cat in Category::all() {
            assert!(!cat.prefix().is_empty());
            assert!(!cat.title().is_empty());
        }
        assert_eq!(Category::Env.prefix(), "env");
        assert_eq!(Category::Bin.prefix(), "bin");
    }

    #[test]
    fn summary_tally_and_verdict() {
        let checks = vec![
            Check::pass("a", Category::Env, "ok"),
            Check::warn("b", Category::Bin, "skew", "reinstall"),
            Check::skip("c", Category::Bin, "n/a"),
        ];
        let s = Summary::of(&checks);
        assert_eq!((s.pass, s.warn, s.fail, s.skip), (1, 1, 0, 1));
        assert_eq!(s.verdict(), Status::Warn);
    }

    #[test]
    fn empty_and_all_skip_runs_are_pass() {
        assert_eq!(Summary::of(&[]).verdict(), Status::Pass);
        let skips = vec![Check::skip("x", Category::Env, "n/a")];
        assert_eq!(Summary::of(&skips).verdict(), Status::Pass);
    }

    #[test]
    fn fail_dominates_warn_in_verdict() {
        let checks = vec![
            Check::warn("w", Category::Env, "d", "f"),
            Check::fail("f", Category::Bin, "d", "f"),
        ];
        assert_eq!(Summary::of(&checks).verdict(), Status::Fail);
    }
}
