use std::path::PathBuf;

use toolfoundry_core::{health::run_health_checks, manifest::load_manifest};

use crate::cli::output::print_health_report;

pub(super) fn validate_manifest(path: PathBuf) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    println!(
        "valid manifest: {} ({})",
        manifest.identity.id, manifest.lifecycle.state
    );
    Ok(())
}

pub(super) fn check_health(path: PathBuf, json: bool) -> anyhow::Result<()> {
    let manifest = load_manifest(&path)?;
    let report = run_health_checks(&manifest)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_health_report(&report);
    }

    if !report.is_healthy() {
        anyhow::bail!("one or more health checks failed");
    }

    Ok(())
}
