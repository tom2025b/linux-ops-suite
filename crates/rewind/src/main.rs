//! rewind CLI. Thin shell: parse flags, dispatch to a subcommand, render human
//! or JSON, exit with a structured code (0 ok / 1 diff found a difference / 2
//! restore --apply partially failed / 3 rewind itself could not run). All the
//! work lives in the library; `main` only chooses what to run and how to print it
//! — the same shape as tripwire's and portman's main. The exit-code *policy*
//! lives here, never in the library.
//!
//! Surface: the timeline view (default / `log`), `capture`, `sources`, `show`,
//! `diff`, the guarded `restore`, and `prune`.

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

    /// Show extra columns (hash prefix, mode, uid/gid, mtime) in `show`.
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
    /// Show one capture's manifest (paths, sizes, hashes, schema versions).
    Show {
        /// Which capture: an id, a unique id prefix, `latest`, `latest-good`,
        /// or a relative index like `~1` (one before latest).
        #[arg(value_name = "CAPTURE")]
        capture: String,
    },
    /// Compare two captures, or a capture against the live files. Exits 1 on any
    /// difference (a cron drift check), 0 when identical.
    Diff {
        /// The first capture (id / prefix / `latest` / `latest-good` / `~N`).
        #[arg(value_name = "A")]
        a: String,
        /// The second capture. Omit to compare A against the current live files.
        #[arg(value_name = "B")]
        b: Option<String>,
    },
    /// Restore a capture's files. DRY-RUN by default (prints the plan, writes
    /// nothing); `--apply` performs the writes after taking a safety capture.
    Restore {
        /// Which capture to restore (id / prefix / `latest` / `~N`). Required
        /// unless `--latest-good` is given.
        #[arg(value_name = "CAPTURE", required_unless_present = "latest_good")]
        capture: Option<String>,
        /// Actually perform the restore (without it, restore is a dry run).
        #[arg(long)]
        apply: bool,
        /// Skip the automatic pre-restore safety capture (default: take it).
        #[arg(long = "no-safety-capture")]
        no_safety_capture: bool,
        /// Restore the most recent capture whose snapshot is a valid envelope.
        #[arg(long = "latest-good")]
        latest_good: bool,
    },
    /// Remove old captures by count/age, and optionally garbage-collect objects.
    /// Nothing is auto-pruned; deletion is immediate (no dry run).
    Prune {
        /// Keep only the newest N captures; remove the rest.
        #[arg(long = "keep-last", value_name = "N")]
        keep_last: Option<usize>,
        /// Remove captures older than this duration, e.g. `30d`, `12h`.
        #[arg(long = "older-than", value_name = "DUR")]
        older_than: Option<String>,
        /// Also delete objects no surviving capture references (mark-and-sweep).
        #[arg(long)]
        gc: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    let result = match &cli.command {
        None | Some(Cmd::Log) => run_timeline(&cli, &style),
        Some(Cmd::Capture { label }) => run_capture(&cli, label.as_deref()),
        Some(Cmd::Sources) => run_sources(&cli, &style),
        Some(Cmd::Show { capture }) => run_show(&cli, capture, &style),
        Some(Cmd::Diff { a, b }) => run_diff(&cli, a, b.as_deref(), &style),
        Some(Cmd::Restore {
            capture,
            apply,
            no_safety_capture,
            latest_good,
        }) => run_restore(
            &cli,
            capture.as_deref(),
            *apply,
            *no_safety_capture,
            *latest_good,
            &style,
        ),
        Some(Cmd::Prune {
            keep_last,
            older_than,
            gc,
        }) => run_prune(&cli, *keep_last, older_than.as_deref(), *gc, &style),
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

/// `rewind show <capture>`: render one capture's manifest.
fn run_show(cli: &Cli, selector: &str, style: &Style) -> Result<ExitCode, RewindError> {
    let manifest = rewind::show_capture(cli.store.clone(), selector)?;
    if cli.json {
        println!("{}", report::show_json(&manifest));
    } else {
        report::print_show(&manifest, cli.verbose, style);
    }
    Ok(ExitCode::SUCCESS)
}

/// `rewind diff <a> [<b>]`: compare two captures, or A against the live files.
/// Exit 1 when they differ (the cron drift check), 0 when identical — the
/// exit-code policy lives here, not in the library.
fn run_diff(cli: &Cli, a: &str, b: Option<&str>, style: &Style) -> Result<ExitCode, RewindError> {
    let diff = match b {
        Some(b) => rewind::diff_captures(cli.store.clone(), a, b)?,
        None => rewind::diff_capture_vs_live(cli.store.clone(), &cli.paths, config_ref(cli), a)?,
    };

    if cli.json {
        println!("{}", report::diff_json(&diff));
    } else {
        report::print_diff(&diff, style);
    }

    Ok(if diff.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `rewind restore <capture>`: dry-run by default; `--apply` writes. Exit 2 on a
/// partial restore (some paths failed) — exit policy lives here, not the library.
fn run_restore(
    cli: &Cli,
    capture: Option<&str>,
    apply: bool,
    no_safety_capture: bool,
    latest_good: bool,
    style: &Style,
) -> Result<ExitCode, RewindError> {
    // clap guarantees `capture` is present unless `--latest-good` was given.
    let selector = if latest_good {
        "latest-good"
    } else {
        capture.expect("clap requires a capture unless --latest-good")
    };

    if !apply {
        let plan = rewind::plan_restore(cli.store.clone(), selector)?;
        if cli.json {
            println!("{}", report::restore_plan_json(&plan));
        } else {
            report::print_restore_plan(&plan, style);
        }
        return Ok(ExitCode::SUCCESS); // dry-run rendered = success
    }

    let captured_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let outcome = rewind::apply_restore(
        cli.store.clone(),
        selector,
        !no_safety_capture,
        &captured_at,
    )?;
    if cli.json {
        println!("{}", report::restore_outcome_json(&outcome));
    } else {
        report::print_restore_outcome(&outcome, style);
    }
    Ok(if outcome.has_failure() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}

/// `rewind prune`: remove captures by count/age; `--gc` reclaims objects.
fn run_prune(
    cli: &Cli,
    keep_last: Option<usize>,
    older_than: Option<&str>,
    gc: bool,
    style: &Style,
) -> Result<ExitCode, RewindError> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let outcome = rewind::prune(cli.store.clone(), keep_last, older_than, gc, &now)?;
    if cli.json {
        println!("{}", report::prune_json(&outcome));
    } else {
        report::print_prune(&outcome, style);
    }
    Ok(ExitCode::SUCCESS)
}
