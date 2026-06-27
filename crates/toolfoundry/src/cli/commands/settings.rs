use std::path::PathBuf;

use toolfoundry_core::config::{init_config, inspect_config};

use crate::cli::output::{print_config_init_report, print_config_report};

pub(super) fn init_config_command(
    config: Option<PathBuf>,
    manifest_directory: Option<PathBuf>,
    force: bool,
    json: bool,
) -> anyhow::Result<()> {
    let report = init_config(config.as_ref(), manifest_directory.as_ref(), force)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_config_init_report(&report);
    }

    Ok(())
}

pub(super) fn inspect_config_command(config: Option<PathBuf>, json: bool) -> anyhow::Result<()> {
    let report = inspect_config(config.as_ref())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_config_report(&report);
    }

    Ok(())
}
