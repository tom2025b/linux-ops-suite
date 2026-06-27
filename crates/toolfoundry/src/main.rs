#![deny(
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]

use clap::Parser;

use toolfoundry::cli::Cli;

fn main() -> anyhow::Result<()> {
    Cli::parse().run()
}
