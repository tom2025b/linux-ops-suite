//! clap argument definitions.
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "rex-forge", version, about = "TUI-first scaffolder for Rust and Go")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a new project (interactive if --base is omitted).
    New(NewArgs),
    /// List available bases and components.
    List,
}

#[derive(clap::Args, Debug)]
pub struct NewArgs {
    /// Target directory / project name.
    pub name: Option<String>,
    /// Base: rust-bin | rust-lib | go-bin | go-lib.
    #[arg(long)]
    pub base: Option<String>,
    /// Comma-separated component names.
    #[arg(long)]
    pub with: Option<String>,
    #[arg(long)]
    pub force: bool,
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    #[arg(long)]
    pub git: bool,
    #[arg(long)]
    pub license: Option<String>,
    #[arg(long)]
    pub author: Option<String>,
}
