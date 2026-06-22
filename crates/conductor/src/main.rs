//! conductor CLI. Thin shell: parse flags, dispatch to a read-only subcommand,
//! render human or JSON, exit with a structured code (0 ok / 3 conductor itself
//! could not run). All the work lives in the library; `main` only chooses what to
//! run and how to print it — the same shape as rewind's and pulse's main.
//!
//! Phase 2: bare `conductor` opens the interactive TUI on a real TTY; falls back
//! to `status` when piped / in CI so scripts keep working. `--dump-view` renders
//! one frame deterministically for snapshot tests.

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

    /// Render one TUI frame once and exit (no event loop): plan | healthy |
    /// compact | help. For deterministic snapshot tests; hidden from help.
    #[arg(long, value_name = "VIEW", global = true, hide = true)]
    dump_view: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the situation and the ordered plan (same as no subcommand).
    Status,
    /// Print the suite's readiness as conductor sees it (feeds + tools).
    Health,
    /// Print just the ordered steps, no situation prose.
    Plan,
    /// Walk the plan interactively, confirming each changes-state step (the
    /// driver). Same as bare `conductor` on a terminal; falls back to `status`
    /// when not a TTY or with --json.
    Orchestrate,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::resolve(cli.no_color);

    // Deterministic frame dump for tests: build the real plan, render one frame.
    if let Some(view) = &cli.dump_view {
        return run_dump_view(&cli, view);
    }

    let result = match &cli.command {
        None => run_bare(&cli, &style),
        Some(Cmd::Status) => run_status(&cli, &style),
        Some(Cmd::Health) => run_health(&cli, &style),
        Some(Cmd::Plan) => run_plan(&cli, &style),
        Some(Cmd::Orchestrate) => run_bare(&cli, &style),
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

/// Bare `conductor` (and `orchestrate`): open the interactive TUI on a real
/// terminal; otherwise fall back to the scriptable `status` output so pipes/CI
/// still work. The guided run's outcome becomes the exit code: 1 a step failed,
/// 2 quit with steps still pending/skipped, 0 clean / all done / nothing to do.
fn run_bare(cli: &Cli, style: &Style) -> Result<ExitCode, ConductorError> {
    if cli.json || !conductor::tui::should_run_interactive() {
        return run_status(cli, style);
    }
    let dir = data_dir(cli)?;
    let state = load_state(&dir);
    let plan = plan::build(&state);
    let report =
        conductor::tui::run(plan, cli.no_color).map_err(|e| ConductorError::Tui(e.to_string()))?;
    Ok(ExitCode::from(report.exit_code()))
}

/// Render exactly one TUI frame (no event loop) and exit 0 — the test backbone.
fn run_dump_view(cli: &Cli, view: &str) -> ExitCode {
    let dir = match data_dir(cli) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("conductor: {e}");
            return ExitCode::from(3);
        }
    };
    let state = load_state(&dir);
    let plan = plan::build(&state);
    // Dumps are monochrome and rendered into an off-screen buffer by the shared
    // suite-ui render path, so the snapshot matches what the live TUI draws.
    match conductor::tui::dump_view(&plan, view, true) {
        Some(frame) => {
            print!("{frame}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "conductor: --dump-view needs one of: plan healthy compact help confirm (got {view})"
            );
            ExitCode::from(3)
        }
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
