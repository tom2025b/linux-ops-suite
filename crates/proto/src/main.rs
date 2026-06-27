use std::process::ExitCode; // richer than returning () — lets us pick the code

use clap::Parser; // brings `Cli::parse()` into scope

// Pull in the library crate by name. `proto` here is OUR library (named in
// Cargo.toml), reached from the binary as an external crate.
use proto::cli::{self, Cli};

fn main() -> ExitCode {
    // Parse argv. clap handles --help/--version/usage errors itself, exiting
    // before this returns if the input is malformed.
    let cli = Cli::parse();

    // Dispatch to the library. On success we exit 0; on error we print the full
    // context chain to stderr (anyhow's `{:#}` shows causes) and exit non-zero
    // so scripts and CI can detect failure.
    match cli::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}"); // {:#} = message + chained causes
            ExitCode::FAILURE
        }
    }
}
