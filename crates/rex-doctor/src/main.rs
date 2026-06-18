//! rex-doctor CLI. Thin shell: parse flags, resolve a [`Selection`], run, then
//! render human or JSON and exit with a structured code (0 clean / 1 warn /
//! 2 fail / 3 the doctor itself could not run). The check logic lives in the
//! library; `main` only chooses what to run and how to print it.

use std::process::ExitCode;

use clap::Parser;

use rex_doctor::model::{Category, Status};
use rex_doctor::{catalog, report, run, Selection};

/// Diagnostics & health checks for the Linux Ops Suite.
///
/// Verifies the installed suite is wired up end-to-end. This release covers the
/// environment (PATH, XDG data dir, writability, aliases) and the suite
/// binaries (present, executable, actually run, version skew, PATH shadowing).
/// Read-only and offline by default.
#[derive(Parser)]
#[command(name = "rex-doctor", version, about, verbatim_doc_comment)]
struct Cli {
    /// Run only these checks or categories (e.g. `env` `bin.present`).
    /// Repeatable; comma-separated values also accepted.
    #[arg(
        long,
        value_name = "ID|CAT",
        value_delimiter = ',',
        conflicts_with = "skip"
    )]
    only: Vec<String>,

    /// Run everything except these checks or categories.
    #[arg(long, value_name = "ID|CAT", value_delimiter = ',')]
    skip: Vec<String>,

    /// Fast subset for shell hooks: env.install-dirs + bin.present only.
    #[arg(long, conflicts_with_all = ["only", "skip", "list"])]
    quick: bool,

    /// Emit the JSON report envelope instead of human output.
    #[arg(long)]
    json: bool,

    /// Show PASS lines too (default hides them; only WARN/FAIL are shown).
    #[arg(short, long)]
    verbose: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long)]
    no_color: bool,

    /// Exit non-zero starting at this severity (default: fail).
    #[arg(long, value_name = "warn|fail", default_value = "fail")]
    fail_on: FailOn,

    /// List every check id and category, then exit.
    #[arg(long)]
    list: bool,
}

/// Severity threshold for a non-zero exit.
#[derive(Clone, Copy, clap::ValueEnum)]
enum FailOn {
    Warn,
    Fail,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.list {
        print_list();
        return ExitCode::SUCCESS;
    }

    let selection = if cli.quick {
        // The wired-up-at-all subset: are install dirs on PATH and is the suite
        // even installed? Cheap enough for a prompt/precommand hook.
        Selection::Only(vec!["env.install-dirs".into(), "bin.present".into()])
    } else if !cli.only.is_empty() {
        Selection::Only(cli.only.clone())
    } else if !cli.skip.is_empty() {
        Selection::Skip(cli.skip.clone())
    } else {
        Selection::All
    };

    let checks = match run(&selection) {
        Ok(checks) => checks,
        Err(err) => {
            eprintln!("rex-doctor: {err}");
            return ExitCode::from(3);
        }
    };

    if cli.json {
        println!("{}", report::to_json(&checks));
    } else {
        let style = report::Style::resolve(cli.no_color);
        report::print_human(&checks, &style, cli.verbose);
    }

    exit_code(&checks, cli.fail_on)
}

/// Map the run's worst status to a process exit code, honoring `--fail-on`.
fn exit_code(checks: &[rex_doctor::model::Check], fail_on: FailOn) -> ExitCode {
    let verdict = rex_doctor::model::Summary::of(checks).verdict();
    match verdict {
        Status::Fail => ExitCode::from(2),
        Status::Warn => match fail_on {
            FailOn::Warn => ExitCode::from(2),
            FailOn::Fail => ExitCode::from(1),
        },
        // Pass or all-skip.
        _ => ExitCode::SUCCESS,
    }
}

/// `--list`: every check id grouped by category. Reads the live catalog so it
/// can't drift from what actually runs.
fn print_list() {
    let catalog = catalog();
    for cat in Category::all() {
        println!("{}:", cat.title());
        for (id, _) in catalog.iter().filter(|(_, c)| c == cat) {
            println!("  {id}");
        }
    }
}
