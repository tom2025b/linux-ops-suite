//! Human-readable terminal table rendering for Bulwark.
//!
//! This module owns the public human-table API while delegating focused details
//! to sibling modules:
//! - `widths`: data-driven column sizing and header construction.
//! - `style`: color-mode resolution and ANSI risk styling.
//! - `tests`: behavior coverage kept out of the production renderer file.

mod style;
#[cfg(test)]
mod tests;
mod widths;

use crate::core::engine::ClassifiedEntry;

use super::format::{human_size, truncate_desc, truncate_path};
pub use style::ColorChoice;
use style::color_risk;
use widths::{ColumnWidths, build_header, compute_column_widths};

/// Render a clean, professional human-readable table from classified inventory
/// as a pure [`String`].
///
/// The renderer measures all rows before writing output, pads the risk column
/// before applying ANSI color, and emits the summary in deterministic risk order.
///
/// Input is `&[crate::core::model::ClassifiedEntry]` (or the engine type).
/// This pure function is the model for how a TUI can also render a static
/// snapshot if it ever wants the exact same layout algorithm.
pub fn render_human_table(entries: &[ClassifiedEntry], color: ColorChoice) -> String {
    if entries.is_empty() {
        return "No items found.\n".to_string();
    }

    let use_color = color.use_color();
    let widths = compute_column_widths(entries);

    let header = build_header(&widths);
    let separator = "═".repeat(header.chars().count());
    let thin_separator = "─".repeat(header.chars().count());

    let mut output = String::new();

    output.push_str("Bulwark Scan Results\n");
    output.push_str(&separator);
    output.push('\n');
    output.push_str(&header);
    output.push('\n');
    output.push_str(&thin_separator);
    output.push('\n');

    for entry in entries {
        output.push_str(&render_row(entry, &widths, use_color));
        output.push('\n');
    }

    output.push_str(&separator);
    output.push('\n');

    let breakdown = risk_breakdown(entries).join(", ");
    output.push_str(&format!(
        "Scanned {} items (sorted by path) — {}.\n",
        entries.len(),
        breakdown
    ));

    output
}

/// Render one data row using precomputed widths.
///
/// The risk cell is padded before styling so ANSI escape bytes never affect
/// visible table alignment.
fn render_row(entry: &ClassifiedEntry, widths: &ColumnWidths, use_color: bool) -> String {
    let path = entry.entry.discovered.path.display().to_string();
    let language = entry.entry.language.as_str();
    let risk = format!("{:?}", entry.classification.risk);
    let size = human_size(entry.entry.discovered.size);
    let description = entry
        .entry
        .description
        .as_deref()
        .map(|desc| truncate_desc(desc, widths.desc))
        .unwrap_or_default();

    let risk_padded = format!("{:<width$}", risk, width = widths.risk);
    let risk_cell = color_risk(&risk_padded, use_color);

    format!(
        "{:<path_w$} {:<lang_w$} {} {:<cat_w$} {:<own_w$} {:>size_w$}  {}",
        truncate_path(&path, widths.path),
        language,
        risk_cell,
        entry.classification.category,
        entry.classification.owner,
        size,
        description,
        path_w = widths.path,
        lang_w = widths.lang,
        cat_w = widths.category,
        own_w = widths.owner,
        size_w = widths.size,
    )
}

/// Build per-risk summary fragments in fixed severity order, omitting zeroes.
fn risk_breakdown(entries: &[ClassifiedEntry]) -> Vec<String> {
    use crate::core::rules::RiskLevel;

    let order = [
        ("low", RiskLevel::Low),
        ("medium", RiskLevel::Medium),
        ("high", RiskLevel::High),
        ("critical", RiskLevel::Critical),
    ];

    order
        .iter()
        .filter_map(|(label, level)| {
            let count = entries
                .iter()
                .filter(|entry| entry.classification.risk == *level)
                .count();
            (count > 0).then(|| format!("{count} {label}"))
        })
        .collect()
}
