//! Bulwark binary entry point.
//!
//! This file is deliberately tiny (a classic "thin main" pattern).
//!
//! All real work lives in the library crate (`bulwark::*`). The binary is
//! responsible only for:
//! - CLI argument parsing (using `clap` derive)
//! - Dispatching to the library
//! - Turning library errors into nice user messages on stderr
//!
//! This separation is intentional: it makes the core logic easy to test
//! and reuse, and it keeps the binary as a simple, replaceable adapter.

use std::{path::PathBuf, process::ExitCode};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use bulwark::{
    ColorChoice, Config, VERSION, print_human_table_classified, print_json_classified,
    print_markdown_table_classified,
};

#[cfg(feature = "tui")]
use bulwark::tui;

mod commands;

/// CLI-facing color mode. Mirrors `bulwark::ColorChoice` but lives here because
/// the `clap::ValueEnum` derive belongs to the binary layer, not the library.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
enum ColorMode {
    /// Color only when writing to a terminal and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always colorize.
    Always,
    /// Never colorize.
    Never,
}

impl From<ColorMode> for ColorChoice {
    fn from(m: ColorMode) -> Self {
        match m {
            ColorMode::Auto => ColorChoice::Auto,
            ColorMode::Always => ColorChoice::Always,
            ColorMode::Never => ColorChoice::Never,
        }
    }
}

/// Bulwark — read-only, YAML-driven safety and inventory for personal tools.
///
/// Running with no subcommand (the most common case) starts the interactive TUI.
/// Explicit subcommands exist for scripting and automation.
#[derive(Parser, Debug)]
#[command(
    name = "bulwark",
    version = VERSION,
    about,
    long_about = "Bulwark is a read-only inventory tool for your personal scripts and tools.\n\n\
                  The default action (no subcommand) launches the keyboard-driven TUI.\n\
                  Use `bulwark scan`, `bulwark scan --json`, or `bulwark config-check` for\n\
                  non-interactive / scripted usage."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan configured directories and produce a report
    Scan {
        /// Emit machine-readable JSON instead of the table
        #[arg(long, conflicts_with = "markdown")]
        json: bool,
        /// Emit a Markdown table instead of the terminal table
        #[arg(long)]
        markdown: bool,

        /// When to colorize the terminal table (auto = only on a TTY)
        #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
        color: ColorMode,

        /// Optional paths to scan (overrides config paths for this run)
        #[arg(value_name = "PATHS")]
        paths: Vec<String>,
    },

    /// Export Bulwark's versioned Workstate findings feed
    ///
    /// Emits the producer-owned v1 Workstate feed envelope
    /// (`schema_version`, `source_tool`, `generated_at`, `item_count`,
    /// `items`) for Workstate to consume. This is the preferred Workstate
    /// input; `bulwark scan --json` remains the general inventory report and
    /// is not the same shape.
    ///
    /// By default the JSON is written to stdout (pipe-friendly). Use
    /// `--output` to publish it atomically to a file instead.
    ///
    /// Examples:
    ///   # Print the feed to stdout
    ///   bulwark workstate-feed
    ///
    ///   # Publish to the path Workstate reads
    ///   bulwark workstate-feed --output ~/.local/share/workstate/feeds/bulwark.json
    ///
    ///   # Scan specific paths with a fixed timestamp (reproducible output)
    ///   bulwark workstate-feed ~/bin ~/scripts --generated-at 2026-06-06T12:00:00Z
    #[command(visible_alias = "ws-feed")]
    WorkstateFeed {
        /// Paths to scan for this run, overriding the configured scan paths
        /// (omit to use the paths from your bulwark config)
        #[arg(value_name = "PATHS")]
        paths: Vec<String>,

        /// RFC 3339 timestamp to stamp on the feed's `generated_at` field
        /// (defaults to the current UTC time; set this for reproducible output)
        #[arg(long, value_name = "RFC3339")]
        generated_at: Option<String>,

        /// Write the feed JSON to this file (created atomically) instead of
        /// printing it to stdout
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Validate and display the effective configuration
    ConfigCheck,

    /// Launch the interactive terminal cockpit (Ratatui)
    ///
    /// Provides a keyboard-driven dashboard, live-filterable results table,
    /// details pane for the selected entry (including sidecar metadata),
    /// help overlay, and rescan. All data comes from the same core engine
    /// used by `scan`, so output is consistent and read-only.
    Tui {
        /// Optional paths to scan (overrides config paths for this run)
        #[arg(value_name = "PATHS")]
        paths: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Some(Commands::Scan {
            json,
            markdown,
            color,
            paths,
        }) => run_scan(json, markdown, color, paths),
        Some(Commands::WorkstateFeed {
            paths,
            generated_at,
            output,
        }) => commands::workstate_feed::run(paths, generated_at, output),
        Some(Commands::ConfigCheck) => run_config_check(),
        Some(Commands::Tui { paths }) => run_tui(paths),
        None => run_tui(vec![]), // `bulwark` alone → TUI (default experience)
    };

    if let Err(e) = result {
        // Print the full anyhow context chain.
        eprintln!("error: {e}");
        for cause in e.chain().skip(1) {
            eprintln!("  caused by: {cause}");
        }
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn run_scan(json: bool, markdown: bool, color: ColorMode, cli_paths: Vec<String>) -> Result<()> {
    let mut config = Config::load().context("failed to load configuration")?;

    // Allow CLI to override scan paths (very useful for quick one-off scans)
    if !cli_paths.is_empty() {
        config.scan.paths = cli_paths;
    }

    warn_missing_scan_paths(&config)?;

    let classified =
        bulwark::collect_classified_inventory(&config).context("failed to scan and classify")?;

    // Surface any non-fatal scan problems (unreadable dirs, etc.) on stderr so
    // stdout/JSON stays clean for piping, and the user knows the inventory is
    // partial rather than silently incomplete.
    print_scan_warnings(&classified.warnings);

    // Dispatch to the correct output format.
    // Note that only the human table receives the color choice — JSON and
    // Markdown are deliberately color-free.
    if json {
        print_json_classified(&classified.entries).context("failed to render JSON output")?;
    } else if markdown {
        print_markdown_table_classified(&classified.entries);
    } else {
        print_human_table_classified(&classified.entries, color.into());
    }

    Ok(())
}

/// Warn on configured roots that the scanner will skip.
///
/// The scanner intentionally treats missing configured directories as optional,
/// but surfacing them on stderr keeps scripted JSON output clean while making a
/// typo visible to humans.
pub(crate) fn warn_missing_scan_paths(config: &Config) -> Result<()> {
    for root in config
        .missing_scan_paths()
        .context("failed to resolve scan paths")?
    {
        eprintln!(
            "warning: scan path does not exist, skipping: {}",
            root.display()
        );
    }

    Ok(())
}

/// Print collected scan warnings to stderr, one per line, in a consistent format.
///
/// Kept separate so `scan` and `tui` (and any future command) report the same
/// way. stderr keeps stdout/JSON clean for piping.
pub(crate) fn print_scan_warnings(warnings: &[bulwark::ScanWarning]) {
    for w in warnings {
        match &w.path {
            Some(p) => eprintln!("warning: {}: {}", p.display(), w.message),
            None => eprintln!("warning: {}", w.message),
        }
    }
}

fn run_config_check() -> Result<()> {
    let config = Config::load().context("failed to load configuration")?;

    // Also load the rule engine here. This is intentional:
    // A malformed user `rules.yaml` turns into a clear, actionable error
    // exactly when the user runs `bulwark config-check` — the natural place
    // to debug configuration problems — instead of failing mysteriously later
    // during a real scan.
    let engine = bulwark::RuleEngine::load().context("failed to load classification rules")?;

    println!("Bulwark effective configuration");
    println!("version: {}", config.version);
    println!("scan:");
    println!("  max_depth: {}", config.scan.max_depth);
    println!("  follow_symlinks: {}", config.scan.follow_symlinks);
    println!("  paths:");
    for p in &config.scan.paths {
        println!("    - {}", p);
    }
    println!("ignore:");
    println!("  names: {:?}", config.ignore.names);
    println!("rules:");
    println!(
        "  loaded: {} (built-in defaults plus any user rules)",
        engine.rule_count()
    );
    println!(
        "\nConfig location: $XDG_CONFIG_HOME/bulwark/config.yaml (or ~/.config/bulwark/config.yaml)"
    );
    println!(
        "Rules location:  $XDG_CONFIG_HOME/bulwark/rules.yaml  (or ~/.config/bulwark/rules.yaml)"
    );

    Ok(())
}

/// Launch the TUI (feature "tui").
///
/// We perform the exact same config/path prep and classification as `run_scan`
/// so that `bulwark tui` and `bulwark scan` are consistent. The TUI then owns
/// the interactive display of the already-classified data.
#[cfg(feature = "tui")]
fn run_tui(cli_paths: Vec<String>) -> Result<()> {
    let mut config = Config::load().context("failed to load configuration")?;

    if !cli_paths.is_empty() {
        config.scan.paths = cli_paths.clone();
    }

    let mut warnings = config
        .missing_scan_path_warnings()
        .context("failed to resolve scan paths")?;

    let classified =
        bulwark::collect_classified_inventory(&config).context("failed to scan and classify")?;
    warnings.extend(classified.warnings);

    // Hand off to the TUI. It is responsible for terminal setup, event loop,
    // drawing, and clean restore on exit (even on panic or error). We pass the
    // warnings along so the cockpit can show a count in its status bar without
    // printing raw stderr during launcher handoff.
    // Pass the CLI path overrides along so the in-TUI rescan re-applies them
    // after reloading the config (otherwise rescan would silently revert to
    // the config-file paths).
    tui::run(classified.entries, warnings, cli_paths).context("interactive TUI session failed")
}

/// Fallback when the binary was built without the "tui" feature.
#[cfg(not(feature = "tui"))]
fn run_tui(_cli_paths: Vec<String>) -> Result<()> {
    anyhow::bail!(
        "this bulwark binary was built without TUI support (the 'tui' feature was disabled).\n\
         Rebuild with default features or --features tui to enable the interactive cockpit."
    )
}
