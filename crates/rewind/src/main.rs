//! rewind CLI. Thin shell: parse flags, dispatch to a subcommand, render human
//! or JSON, exit with a structured code (0 ok / 3 rewind itself could not run).
//! All the work lives in the library; `main` only chooses what to run and how to
//! print it — the same shape as tripwire's and portman's main.
//!
//! Phase 1 surface: the timeline view (default / `log`), `capture`, and
//! `sources`. `show`, `diff`, `restore`, and `prune` arrive in later phases.

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use clap::{Parser, Subcommand};

use rewind::report::{self, Style};
use rewind::RewindError;

/// What did the suite's state look like before, and how do I get back safely.
///
/// Records the suite's own state files (the compiled Workstate snapshot, the
/// producer feeds, tripwire's baseline) into a content-addressed store, and lets
/// you list, compare, and restore those captures. Read-only by default; restore
/// (a later phase) is dry-run-by-default and only touches rewind's own captures.
/// With no subcommand, shows the capture timeline, newest first.
#[derive(Parser)]
#[command(name = "rewind", version, about, verbatim_doc_comment)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Emit the JSON envelope instead of human output.
    #[arg(long, global = true)]
    json: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long, global = true)]
    no_color: bool,

    /// Show extra columns (reserved for later phases).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Use this store directory instead of the suite's default XDG path.
    #[arg(long, value_name = "PATH", global = true)]
    store: Option<PathBuf>,

    /// Capture this path instead of the config/built-in set (repeatable).
    #[arg(long = "path", value_name = "PATH", global = true)]
    paths: Vec<PathBuf>,

    /// Read the capture set from this config file instead of the default.
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,
}

/// rewind's subcommands. Absent = the timeline view.
#[derive(Subcommand)]
enum Cmd {
    /// Show the capture timeline, newest first (same as no subcommand).
    Log,
    /// Record the current capture set as a new immutable capture.
    Capture {
        /// Attach a human label to this capture, e.g. `pre-upgrade`.
        #[arg(long, value_name = "LABEL")]
        label: Option<String>,
    },
    /// Show the resolved capture set, its source, and store stats.
    Sources,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    let result = match &cli.command {
        None | Some(Cmd::Log) => run_timeline(&cli, &style),
        Some(Cmd::Capture { label }) => run_capture(&cli, label.as_deref()),
        Some(Cmd::Sources) => run_sources(&cli, &style),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("rewind: {err}");
            ExitCode::from(3)
        }
    }
}

/// The config override as a borrowed path, if given.
fn config_ref(cli: &Cli) -> Option<&std::path::Path> {
    cli.config.as_deref()
}

/// Default / `log`: list the capture timeline.
fn run_timeline(cli: &Cli, style: &Style) -> Result<ExitCode, RewindError> {
    let (manifests, store_dir) = rewind::list_captures(cli.store.clone())?;
    let store_path = store_dir.to_string_lossy();
    let store_bytes = rewind::store_stats(cli.store.clone())?.0;

    if cli.json {
        println!(
            "{}",
            report::timeline_json(&manifests, store_bytes, &store_path)
        );
    } else {
        report::print_timeline(&manifests, store_bytes, &store_path, style);
    }
    Ok(ExitCode::SUCCESS)
}

/// `rewind capture`: record the current capture set.
fn run_capture(cli: &Cli, label: Option<&str>) -> Result<ExitCode, RewindError> {
    let captured_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let manifest = rewind::record_capture(
        &cli.paths,
        config_ref(cli),
        cli.store.clone(),
        &captured_at,
        label,
    )?;

    if cli.json {
        println!("{}", report::capture_json(&manifest));
    } else {
        let short: String = manifest.id.chars().take(8).collect();
        println!(
            "Captured {} ({} paths) → {}",
            short,
            manifest.path_count(),
            manifest.captured_at
        );
        if let Some(l) = &manifest.label {
            println!("Label: {l}");
        }
        println!("Run `rewind` to see the timeline.");
    }
    Ok(ExitCode::SUCCESS)
}

/// `rewind sources`: show the resolved capture set and store stats.
fn run_sources(cli: &Cli, style: &Style) -> Result<ExitCode, RewindError> {
    let set = rewind::capture_set(&cli.paths, config_ref(cli))?;
    let (store_bytes, capture_count, store_dir) = rewind::store_stats(cli.store.clone())?;
    let store_path = store_dir.to_string_lossy();

    if cli.json {
        println!(
            "{}",
            report::sources_json(&set, store_bytes, capture_count, &store_path)
        );
    } else {
        report::print_sources(&set, store_bytes, capture_count, &store_path, style);
    }
    Ok(ExitCode::SUCCESS)
}
