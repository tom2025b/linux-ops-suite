// ============================================================================
// crates/scriptvault/src/main.rs
// ============================================================================
// Entry point + clap dispatch. No subcommand now launches the real TUI.
// ============================================================================

mod cli;
mod logging;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// ScriptVault — a personal search engine for your scripts and tools.
///
/// With no subcommand, ScriptVault opens its interactive TUI. Use
/// `scriptvault search <query>` for a headless, scriptable search.
// `pub(crate)` so the `gen` subcommand (cli/generate.rs) can rebuild this
// command tree via `crate::Cli::command()` to drive completion/man-page output.
#[derive(Debug, Parser)]
#[command(name = "scriptvault", version, about)]
pub(crate) struct Cli {
    /// When to use colour in the TUI: `auto` (default; honours `NO_COLOR`),
    /// `always` (force colour, even under `NO_COLOR`), or `never`.
    /// Top-level only (the TUI launch); `scriptvault search` does its own
    /// terminal-aware colour, so the flag is intentionally NOT global — that
    /// avoids `search --color always` silently parsing yet doing nothing.
    #[arg(long, value_enum, default_value_t, value_name = "WHEN")]
    color: tui::ColorChoice,

    /// Accent colour theme for the TUI: `cyan` (default) or `amber`. Only the
    /// accent hue changes; `NO_COLOR` / `--color never` still drops all colour.
    /// Top-level only (the TUI launch), like `--color`.
    #[arg(long, value_enum, default_value_t, value_name = "NAME")]
    theme: tui::ThemeChoice,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Search indexed scripts and print the results (headless / scriptable).
    Search(cli::SearchArgs),

    /// Emit ScriptVault's versioned Workstate feed (JSON) from the live index.
    /// This is the preferred Workstate input; Workstate spawns it and reads
    /// stdout. `search --format json` remains the general scan and is a different
    /// shape.
    #[command(name = "workstate-feed", visible_alias = "ws-feed")]
    WorkstateFeed(cli::WorkstateFeedArgs),

    /// Generate shell completions or a man page (for packagers/install scripts).
    /// Hidden from `--help`: end users never need it, but `scriptvault gen zsh`
    /// (etc.) gives a stable, scriptable way to emit the artifacts on demand.
    #[command(hide = true)]
    Gen(cli::GenArgs),
}

fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Some(Commands::Search(search_args)) => cli::run_search(search_args),
        Some(Commands::WorkstateFeed(args)) => cli::run_workstate_feed(args),
        Some(Commands::Gen(gen_args)) => cli::run_gen(gen_args),
        // No subcommand => launch the interactive TUI, honouring --color/--theme.
        None => tui::run(args.color, args.theme),
    }
}

// TUI is default; search is the scriptable path. Both use the same core facade.
