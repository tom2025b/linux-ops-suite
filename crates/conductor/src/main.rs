//! conductor — CLI entry point: parse args, build config, dispatch, set exit code.

mod cli;
mod commands;
mod core;
mod ui;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::core::config::Config;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = Config::new(cli.data_dir, cli.json, cli.no_color);

    let result = match cli.command {
        Command::Workstate(cmd) => commands::workstate::run(cmd, &config),
        Command::Rewind(cmd) => commands::rewind::run(cmd, &config),
        Command::Bulwark(cmd) => commands::bulwark::run(cmd, &config),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("conductor: {err}");
            ExitCode::FAILURE
        }
    }
}
