//! conductor CLI. Thin shell: parse flags, dispatch to a read-only subcommand,
//! render human or JSON, exit with a structured code (0 ok / 3 conductor itself
//! could not run). All the work lives in the library; `main` only chooses what to
//! run and how to print it — the same shape as rewind's and pulse's main.
//!
//! Phase 1 surface: `status` (default), `health`, `plan`. The interactive TUI
//! (`conductor` bare) and `orchestrate` arrive in Phases 2–3; until then, bare
//! `conductor` prints `status`, which keeps it useful and scriptable.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use conductor::report::{self, Style};
use conductor::sources::DataDir;
use conductor::{load_state, plan, ConductorError};

/// Given the suite's current state, what should I do — and in what order?
///
/// Conductor reads the suite's own state files, derives a short ordered runbook,
/// and (in later phases) walks you through it. Read-only by default: it never
/// writes a live file itself. With no subcommand, prints the situation + plan.
#[derive(Parser)]
#[command(name = "conductor", version, about, verbatim_doc_comment)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Emit the JSON envelope instead of human output.
    #[arg(long, global = true)]
    json: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long, global = true)]
    no_color: bool,

    /// Show extra detail (reserved for later phases).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Read suite contracts from this directory instead of the XDG default.
    #[arg(long, value_name = "DIR", global = true)]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the situation and the ordered plan (same as no subcommand).
    Status,
    /// Print the suite's readiness as conductor sees it (feeds + tools).
    Health,
    /// Print just the ordered steps, no situation prose.
    Plan,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    let result = match &cli.command {
        None | Some(Cmd::Status) => run_status(&cli, &style),
        Some(Cmd::Health) => run_health(&cli, &style),
        Some(Cmd::Plan) => run_plan(&cli, &style),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("conductor: {err}");
            ExitCode::from(3)
        }
    }
}

/// Resolve the data dir from `--data-dir` or the environment.
fn data_dir(cli: &Cli) -> Result<DataDir, ConductorError> {
    match &cli.data_dir {
        Some(p) => Ok(DataDir::new(p.clone())),
        None => DataDir::from_env(),
    }
}

fn run_status(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    if cli.json {
        println!("{}", report::status_json(&plan, state.built_at.as_deref()));
    } else {
        print!(
            "{}",
            report::print_status(&plan, state.built_at.as_deref(), style)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_plan(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    if cli.json {
        println!("{}", report::status_json(&plan, state.built_at.as_deref()));
    } else {
        print!("{}", report::print_plan(&plan, style));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_health(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    if cli.json {
        println!("{}", report::health_json(&state));
    } else {
        print!("{}", report::print_health(&state, style));
    }
    Ok(ExitCode::from(report::health_exit_code(&state)))
}
