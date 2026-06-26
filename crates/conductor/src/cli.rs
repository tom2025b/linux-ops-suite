//! Command-line surface: the argument tree and help text.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// conductor — drive the suite's workstate, rewind, and bulwark tools.
#[derive(Parser)]
#[command(name = "conductor", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Emit JSON instead of human output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable coloured output.
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Read and write suite state under this directory.
    #[arg(long, value_name = "DIR", global = true)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Manage the workstate snapshot.
    #[command(subcommand)]
    Workstate(WorkstateCmd),

    /// Capture and restore state snapshots.
    #[command(subcommand)]
    Rewind(RewindCmd),

    /// Inspect findings and drift.
    #[command(subcommand)]
    Bulwark(BulwarkCmd),
}

#[derive(Subcommand)]
pub enum WorkstateCmd {
    /// Write a fresh snapshot of the current state.
    Snapshot,
    /// Re-stamp the snapshot as current.
    Refresh,
    /// Show snapshot freshness and counts.
    Status,
}

#[derive(Subcommand)]
pub enum RewindCmd {
    /// Save the current snapshot as a restore point.
    Capture,
    /// Restore a saved point by id.
    Restore {
        /// The restore-point id (see `rewind list`).
        id: String,
    },
    /// List saved restore points.
    List,
}

#[derive(Subcommand)]
pub enum BulwarkCmd {
    /// Show one finding by id.
    Show {
        /// The finding id.
        id: String,
    },
    /// Summarise all findings.
    Check,
    /// Report high-severity drift.
    Tripwire,
}
