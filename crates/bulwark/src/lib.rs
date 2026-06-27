//! Binary-facing facade for the `bulwark` package.
//!
//! Reusable inventory logic lives in `bulwark-core`. This crate keeps the
//! historical `bulwark::...` import paths available for the binary and any
//! transitional consumers, while owning CLI/TUI presentation side effects.

pub use bulwark_core::{
    BulwarkError, Classification, ClassifiedEntry, ClassifiedInventory, ColorChoice, Config,
    DiscoveredFile, Inventory, Language, MatchSpec, RiskLevel, Rule, RuleEngine, ScanOutcome,
    ScanWarning, ScriptEntry, SidecarMetadata, app, collect_classified_inventory,
    collect_inventory, core, error, model, render_human_table, render_json_classified,
    render_markdown_table_classified, render_workstate_feed,
};

/// Current version of the Bulwark CLI package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print the human terminal table to stdout.
pub fn print_human_table_classified(entries: &[ClassifiedEntry], color: ColorChoice) {
    print!("{}", render_human_table(entries, color));
}

/// Print the JSON report to stdout.
pub fn print_json_classified(entries: &[ClassifiedEntry]) -> Result<(), BulwarkError> {
    let json = render_json_classified(entries)?;
    println!("{json}");
    Ok(())
}

/// Print the Markdown table to stdout.
pub fn print_markdown_table_classified(entries: &[ClassifiedEntry]) {
    print!("{}", render_markdown_table_classified(entries));
}

/// Interactive terminal UI. Presentation-only and feature-gated.
#[cfg(feature = "tui")]
pub mod tui;
