//! tripwire CLI. Thin shell: parse flags, dispatch to a subcommand, render human
//! or JSON, exit with a structured code (0 ok / 1 drift-found / 3 tripwire
//! itself could not run). All the work lives in the library; `main` only chooses
//! what to run and how to print it — the same shape as portman's main.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use tripwire::report::{self, Style};
use tripwire::TripwireError;

/// What changed on disk since I last looked.
///
/// Records a baseline (SHA-256 + metadata) of a watched set of files and
/// directories, then reports what drifted — added, removed, modified, or
/// re-permissioned. Read-only: the only file it ever writes is its own baseline.
/// With no subcommand, shows the current state of the watch set. `tripwire diff`
/// exits 1 on any change, so it drops straight into cron.
#[derive(Parser)]
#[command(name = "tripwire", version, about, verbatim_doc_comment)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Emit the JSON envelope instead of human output.
    #[arg(long, global = true)]
    json: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long, global = true)]
    no_color: bool,

    /// Show extra columns (hash prefix, owner, mtime) in the current view.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Use this baseline file instead of the suite's default XDG path.
    #[arg(long, value_name = "PATH", global = true)]
    baseline_file: Option<PathBuf>,

    /// Watch this path instead of the config/built-in set (repeatable).
    #[arg(long = "path", value_name = "PATH", global = true)]
    paths: Vec<PathBuf>,

    /// Read the watch set from this config file instead of the default.
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,
}

/// tripwire's subcommands. Absent = the current view.
#[derive(Subcommand)]
enum Cmd {
    /// Show the resolved watch set and where it came from.
    Watch,
    /// Record the current state as the baseline to diff against later.
    Baseline,
    /// Show what changed since the recorded baseline.
    Diff,
    /// Like `diff`, but print nothing when clean (cron-quiet). Exits 1 on drift.
    Verify,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    let result = match cli.command {
        None => run_current(&cli, &style),
        Some(Cmd::Watch) => run_watch(&cli, &style),
        Some(Cmd::Baseline) => run_baseline(&cli),
        Some(Cmd::Diff) => run_diff(&cli, &style, false),
        Some(Cmd::Verify) => run_diff(&cli, &style, true),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("tripwire: {err}");
            ExitCode::from(3)
        }
    }
}

/// The config override as a borrowed path, if given.
fn config_ref(cli: &Cli) -> Option<&std::path::Path> {
    cli.config.as_deref()
}

/// Default command: scan the watch set and print the table or JSON.
fn run_current(cli: &Cli, style: &Style) -> Result<ExitCode, TripwireError> {
    let scan = tripwire::current(&cli.paths, config_ref(cli), None)?;
    if cli.json {
        println!("{}", report::scan_json(&scan));
    } else {
        report::print_scan(&scan, style, cli.verbose);
    }
    Ok(ExitCode::SUCCESS)
}

/// `tripwire watch`: show the resolved watch set without scanning.
fn run_watch(cli: &Cli, style: &Style) -> Result<ExitCode, TripwireError> {
    let set = tripwire::watch_set(&cli.paths, config_ref(cli))?;
    if cli.json {
        println!("{}", report::watch_json(&set));
    } else {
        report::print_watch_set(&set, style);
    }
    Ok(ExitCode::SUCCESS)
}

/// `tripwire baseline`: record the current state.
fn run_baseline(cli: &Cli) -> Result<ExitCode, TripwireError> {
    let (path, count) =
        tripwire::save_baseline(&cli.paths, config_ref(cli), cli.baseline_file.clone())?;
    if cli.json {
        println!(
            "{{\"source_tool\":\"tripwire\",\"action\":\"baseline\",\"path\":{},\"count\":{}}}",
            json_string(&path.to_string_lossy()),
            count
        );
    } else {
        println!("Baseline recorded → {} ({count} paths)", path.display());
        println!("Run `tripwire diff` later to see what changed.");
    }
    Ok(ExitCode::SUCCESS)
}

/// `tripwire diff` / `tripwire verify`: compare live vs baseline. Exit 1 when
/// anything changed, so the command is usable as a tripwire in scripts/cron.
/// When `quiet` (verify), a clean result prints nothing at all.
fn run_diff(cli: &Cli, style: &Style, quiet: bool) -> Result<ExitCode, TripwireError> {
    let (diff, _path) =
        tripwire::diff_against_baseline(&cli.paths, config_ref(cli), cli.baseline_file.clone())?;

    if cli.json {
        println!("{}", report::diff_json(&diff));
    } else if !(quiet && diff.is_clean()) {
        report::print_diff(&diff, style);
    }

    Ok(if diff.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// Minimal JSON string escaper for the one-line baseline confirmation envelope
/// (avoids re-deriving Serialize for a single field) — same helper as portman.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
