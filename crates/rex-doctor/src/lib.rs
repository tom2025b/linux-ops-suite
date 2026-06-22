//! rex-doctor — diagnostics & health checks for the Linux Ops Suite.
//!
//! Where rex-check answers "are my *repos* healthy?" and `rexops status` shows
//! the live cockpit, rex-doctor answers the third question: "is the *installed
//! suite* wired up and working end-to-end?" This first cut ships the two
//! foundational groups — `env.*` (PATH, XDG, writability, aliases) and `bin.*`
//! (present, executable, actually-runs, version compatibility, PATH shadowing) — with
//! the model, selection, and reporting plumbing the later groups
//! (contracts, data-flow, tool self-checks) will slot straight into.
//!
//! Everything is read-only and offline. Checks never print; they return
//! [`model::Check`] values that [`report`] renders as grouped human output or a
//! JSON envelope identical in shape to the suite's other feeds.

pub mod checks;
pub mod error;
pub mod model;
pub mod report;
pub mod util;

use error::DoctorError;
use model::{Category, Check};

/// Which checks to run, resolved from the CLI selectors.
pub enum Selection {
    /// Every check in every category (the default).
    All,
    /// Only checks whose id or category prefix is in this list.
    Only(Vec<String>),
    /// Every check except those whose id or category prefix is in this list.
    Skip(Vec<String>),
}

/// Run the selected checks and return them in display order.
///
/// Selectors may be a full check id (`bin.present`) or a category prefix
/// (`env`). An unrecognized selector is a hard [`DoctorError`] — a typo that
/// silently ran everything (or nothing) would be a worse failure than stopping.
pub fn run(selection: &Selection) -> Result<Vec<Check>, DoctorError> {
    let all = checks::run_all();
    match selection {
        Selection::All => Ok(all),
        Selection::Only(sel) => {
            validate_selectors(sel, &all)?;
            Ok(all.into_iter().filter(|c| matches(c, sel)).collect())
        }
        Selection::Skip(sel) => {
            validate_selectors(sel, &all)?;
            Ok(all.into_iter().filter(|c| !matches(c, sel)).collect())
        }
    }
}

/// Whether a check is named by any selector — by exact id or by category prefix.
fn matches(check: &Check, selectors: &[String]) -> bool {
    selectors
        .iter()
        .any(|s| s == check.id || s == check.category.prefix())
}

/// Reject any selector that names neither a known check id nor a known category.
fn validate_selectors(selectors: &[String], all: &[Check]) -> Result<(), DoctorError> {
    for s in selectors {
        let is_category = Category::all().iter().any(|c| c.prefix() == s);
        let is_id = all.iter().any(|c| c.id == s);
        if !is_category && !is_id {
            return Err(DoctorError::UnknownSelector { value: s.clone() });
        }
    }
    Ok(())
}

/// Every check id with its category and one-line detail-less description, for
/// `--list`. Built by running the checks once and reading their ids — so the
/// list can never drift from what actually runs.
pub fn catalog() -> Vec<(&'static str, Category)> {
    checks::run_all()
        .iter()
        .map(|c| (c.id, c.category))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_runs_both_categories() {
        let checks = run(&Selection::All).expect("all selection is valid");
        assert!(checks.iter().any(|c| c.category == Category::Env));
        assert!(checks.iter().any(|c| c.category == Category::Bin));
    }

    #[test]
    fn only_category_filters_to_it() {
        let checks = run(&Selection::Only(vec!["env".into()])).expect("valid");
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.category == Category::Env));
    }

    #[test]
    fn only_single_id_filters_to_one() {
        let checks = run(&Selection::Only(vec!["bin.present".into()])).expect("valid");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].id, "bin.present");
    }

    #[test]
    fn skip_category_removes_it() {
        let checks = run(&Selection::Skip(vec!["bin".into()])).expect("valid");
        assert!(checks.iter().all(|c| c.category != Category::Bin));
    }

    #[test]
    fn unknown_selector_is_an_error() {
        let err = run(&Selection::Only(vec!["nope.bogus".into()]));
        assert!(matches!(err, Err(DoctorError::UnknownSelector { .. })));
    }

    #[test]
    fn catalog_lists_known_ids() {
        let cat = catalog();
        assert!(cat.iter().any(|(id, _)| *id == "env.install-dirs"));
        assert!(cat.iter().any(|(id, _)| *id == "bin.present"));
    }
}
