//! The check groups. Each submodule owns one `Category` and exposes
//! `run() -> Vec<Check>`. `all()` runs every group in display order; the CLI
//! filters the result by id/category for `--only`/`--skip`.

pub mod bin;
pub mod env;

use crate::model::{Category, Check};

/// Run one category's checks.
pub fn run_category(cat: Category) -> Vec<Check> {
    match cat {
        Category::Env => env::run(),
        Category::Bin => bin::run(),
    }
}

/// Run every category, in display order.
pub fn run_all() -> Vec<Check> {
    let mut out = Vec::new();
    for cat in Category::all() {
        out.extend(run_category(*cat));
    }
    out
}
