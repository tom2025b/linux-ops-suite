use std::path::PathBuf;

use toolfoundry_core::{
    install::{apply_install, check_install_drift, plan_install},
    manifest::load_manifest,
};

use crate::cli::output::{print_drift_report, print_install_apply_report, print_install_plan};

pub(super) fn report_drift(path: PathBuf, json: bool) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    let report = check_install_drift(&manifest)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_drift_report(&report);
    }

    if !report.is_current() {
        anyhow::bail!("install state drift detected");
    }

    Ok(())
}

pub(super) fn report_install_plan(path: PathBuf, json: bool) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    let plan = plan_install(&manifest)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_install_plan(&plan);
    }

    if !plan.is_ready() {
        anyhow::bail!("install plan is blocked");
    }

    Ok(())
}

pub(super) fn apply_install_command(path: PathBuf, yes: bool, json: bool) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!("install-apply requires --yes to make filesystem changes");
    }

    let manifest = load_manifest(&path)?;
    let report = apply_install(&manifest)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_install_apply_report(&report);
    }

    if !report.final_drift.is_current() {
        anyhow::bail!("install apply completed but drift remains");
    }

    Ok(())
}
