use std::path::PathBuf;

use chrono::{Local, NaiveDate};
use toolfoundry_core::{
    lifecycle::{evaluate_lifecycle, evaluate_transition},
    manifest::{LifecycleState, load_manifest},
};

use crate::cli::output::{print_lifecycle_report, print_lifecycle_transition_report};

pub(super) fn report_lifecycle(
    path: PathBuf,
    as_of: Option<NaiveDate>,
    json: bool,
) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    let as_of = as_of.unwrap_or_else(|| Local::now().date_naive());
    let report = evaluate_lifecycle(&manifest, as_of);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_lifecycle_report(&report);
    }

    Ok(())
}

pub(super) fn report_lifecycle_transition(
    path: PathBuf,
    to: LifecycleState,
    json: bool,
) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    let report = evaluate_transition(&manifest, to);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_lifecycle_transition_report(&report);
    }

    if !report.allowed {
        anyhow::bail!("lifecycle transition is not allowed");
    }

    Ok(())
}
