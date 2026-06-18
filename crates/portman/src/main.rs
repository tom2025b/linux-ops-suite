//! portman CLI. Thin shell: parse flags, dispatch to a subcommand, render human
//! or JSON, exit with a structured code (0 ok / 1 diff-found / 3 portman itself
//! could not run). All the work lives in the library; `main` only chooses what
//! to run and how to print it.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use portman::report::{self, Style};
use portman::PortmanError;

/// What is listening on this machine, and why.
///
/// Lists every listening socket and resolves the full ownership chain behind it
/// — socket -> PID -> process -> systemd unit -> package. Read-only. Works
/// without root (owners of other users' sockets show as `?`); `sudo` fills in
/// the rest. With no subcommand, shows the current view.
#[derive(Parser)]
#[command(name = "portman", version, about, verbatim_doc_comment)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Emit the JSON envelope instead of human output.
    #[arg(long, global = true)]
    json: bool,

    /// Force monochrome output (also auto-off when stdout isn't a TTY).
    #[arg(long, global = true)]
    no_color: bool,

    /// Show the exe + package columns in the table (current view only).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Use this baseline file instead of the suite's default XDG path.
    #[arg(long, value_name = "PATH", global = true)]
    baseline_file: Option<PathBuf>,
}

/// portman's subcommands. Absent = the current view.
#[derive(Subcommand)]
enum Cmd {
    /// Record the current listeners as the baseline to diff against later.
    Baseline,
    /// Show what changed since the recorded baseline.
    Diff,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = report::Style::resolve(cli.no_color);

    let result = match cli.command {
        None => run_current(&cli, &style),
        Some(Cmd::Baseline) => run_baseline(&cli),
        Some(Cmd::Diff) => run_diff(&cli, &style),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("portman: {err}");
            ExitCode::from(3)
        }
    }
}

/// Default command: enumerate listeners and print the table or JSON.
fn run_current(cli: &Cli, style: &Style) -> Result<ExitCode, PortmanError> {
    let listeners = portman::current()?;
    if cli.json {
        println!("{}", report::listeners_json(&listeners));
    } else {
        report::print_listeners(&listeners, style, cli.verbose);
    }
    Ok(ExitCode::SUCCESS)
}

/// `portman baseline`: record the current listeners.
fn run_baseline(cli: &Cli) -> Result<ExitCode, PortmanError> {
    let path = portman::save_baseline(cli.baseline_file.clone())?;
    if cli.json {
        println!(
            "{{\"source_tool\":\"portman\",\"action\":\"baseline\",\"path\":{}}}",
            json_string(&path.to_string_lossy())
        );
    } else {
        println!("Baseline recorded → {}", path.display());
        println!("Run `portman diff` later to see what changed.");
    }
    Ok(ExitCode::SUCCESS)
}

/// `portman diff`: compare live vs baseline. Exit 1 when anything changed, so
/// the command is usable as a tripwire in scripts/cron.
fn run_diff(cli: &Cli, style: &Style) -> Result<ExitCode, PortmanError> {
    let (diff, _path) = portman::diff_against_baseline(cli.baseline_file.clone())?;
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

/// Minimal JSON string escaper for the one-line baseline confirmation envelope
/// (avoids pulling the whole serde machinery in for a single field).
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
