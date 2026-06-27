mod catalog;
mod install;
mod lifecycle;
mod manifest;
mod settings;
mod workstate;

use std::path::PathBuf;

use toolfoundry_core::config::inspect_config;

use crate::cli::args::{Command, ConfigCommand};

pub(crate) fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Validate { manifest } => manifest::validate_manifest(manifest),
        Command::Health { manifest, json } => manifest::check_health(manifest, json),
        Command::Lifecycle {
            manifest,
            as_of,
            json,
        } => lifecycle::report_lifecycle(manifest, as_of, json),
        Command::LifecycleTransition { manifest, to, json } => {
            lifecycle::report_lifecycle_transition(manifest, to, json)
        }
        Command::Drift { manifest, json } => install::report_drift(manifest, json),
        Command::InstallPlan { manifest, json } => install::report_install_plan(manifest, json),
        Command::InstallApply {
            manifest,
            yes,
            json,
        } => install::apply_install_command(manifest, yes, json),
        Command::Catalog {
            directory,
            config,
            json,
        } => catalog::report_catalog(directory, config, json),
        Command::TuiCatalog {
            directory,
            config,
            json,
        } => catalog::report_tui_catalog(directory, config, json),
        Command::WorkstateFeed {
            directory,
            config,
            as_of,
            generated_at,
            output,
        } => workstate::export_workstate_feed(directory, config, as_of, generated_at, output),
        Command::Config { command } => match command {
            ConfigCommand::Init {
                config,
                manifest_directory,
                force,
                json,
            } => settings::init_config_command(config, manifest_directory, force, json),
            ConfigCommand::Inspect { config, json } => {
                settings::inspect_config_command(config, json)
            }
        },
    }
}

pub(super) fn resolve_manifest_directory(
    directory: Option<PathBuf>,
    config: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    if let Some(directory) = directory {
        return Ok(directory);
    }

    Ok(inspect_config(config.as_ref())?.manifest_directory)
}
