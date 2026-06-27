use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use toolfoundry_core::manifest::LifecycleState;

#[derive(Debug, Parser)]
#[command(name = "toolfoundry")]
#[command(version)]
#[command(about = "Lifecycle and ownership tooling for the Personal Linux Ops Suite")]
pub struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Load and validate a tool manifest.
    Validate {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,
    },
    /// Run the health checks declared by a tool manifest.
    Health {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Emit a machine-readable JSON health report.
        #[arg(long)]
        json: bool,
    },
    /// Report lifecycle state, review status, and allowed transitions.
    Lifecycle {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Evaluate lifecycle review status as of this date.
        #[arg(long, value_parser = parse_date)]
        as_of: Option<NaiveDate>,

        /// Emit a machine-readable JSON lifecycle report.
        #[arg(long)]
        json: bool,
    },
    /// Check whether a lifecycle transition is allowed without editing the manifest.
    LifecycleTransition {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Target lifecycle state.
        #[arg(long, value_parser = parse_lifecycle_state)]
        to: LifecycleState,

        /// Emit a machine-readable JSON lifecycle transition report.
        #[arg(long)]
        json: bool,
    },
    /// Report install and desired-link drift without changing files.
    Drift {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Emit a machine-readable JSON drift report.
        #[arg(long)]
        json: bool,
    },
    /// Plan installer actions without changing files.
    InstallPlan {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Emit a machine-readable JSON install plan.
        #[arg(long)]
        json: bool,
    },
    /// Apply safe installer actions after explicit confirmation.
    InstallApply {
        /// Path to the tool manifest YAML file.
        manifest: PathBuf,

        /// Explicitly confirm filesystem changes.
        #[arg(long)]
        yes: bool,

        /// Emit a machine-readable JSON install apply report.
        #[arg(long)]
        json: bool,
    },
    /// List validated tool manifests from one manifest directory.
    Catalog {
        /// Directory containing human-authored tool manifests.
        directory: Option<PathBuf>,

        /// Config file used to resolve the manifest directory when omitted.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Emit a machine-readable JSON catalog.
        #[arg(long)]
        json: bool,
    },
    /// Render a terminal catalog dashboard from validated manifests.
    TuiCatalog {
        /// Directory containing human-authored tool manifests.
        directory: Option<PathBuf>,

        /// Config file used to resolve the manifest directory when omitted.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Emit a machine-readable JSON catalog dashboard.
        #[arg(long)]
        json: bool,
    },
    /// Export ToolFoundry's neutral Workstate feed.
    WorkstateFeed {
        /// Directory containing human-authored tool manifests.
        directory: Option<PathBuf>,

        /// Config file used to resolve the manifest directory when omitted.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Evaluate review status as of this date.
        #[arg(long, value_parser = parse_date)]
        as_of: Option<NaiveDate>,

        /// Timestamp to stamp on the generated feed.
        #[arg(long, value_parser = parse_datetime)]
        generated_at: Option<DateTime<Utc>>,

        /// Write JSON to this path instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Inspect ToolFoundry configuration paths and manifest defaults.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Create a default ToolFoundry config file.
    Init {
        /// Explicit config file path to create.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Manifest directory to record in the config file.
        #[arg(long)]
        manifest_directory: Option<PathBuf>,

        /// Overwrite an existing config file.
        #[arg(long)]
        force: bool,

        /// Emit a machine-readable JSON config init report.
        #[arg(long)]
        json: bool,
    },
    /// Report config path, data directory, and manifest directory.
    Inspect {
        /// Explicit config file path to inspect.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Emit a machine-readable JSON config report.
        #[arg(long)]
        json: bool,
    },
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|error| format!("expected YYYY-MM-DD date: {error}"))
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| format!("expected RFC3339 timestamp: {error}"))
}

fn parse_lifecycle_state(value: &str) -> Result<LifecycleState, String> {
    match value {
        "experimental" => Ok(LifecycleState::Experimental),
        "active" => Ok(LifecycleState::Active),
        "stale" => Ok(LifecycleState::Stale),
        "risky" => Ok(LifecycleState::Risky),
        "broken" => Ok(LifecycleState::Broken),
        "deprecated" => Ok(LifecycleState::Deprecated),
        "archived" => Ok(LifecycleState::Archived),
        _ => Err(
            "expected one of: experimental, active, stale, risky, broken, deprecated, archived"
                .to_string(),
        ),
    }
}
