use super::*;
use crate::core::engine::ClassifiedEntry;
use crate::core::entry::{Language, ScriptEntry};
use crate::core::rules::{Classification, RiskLevel};
use crate::core::scanner::DiscoveredFile;
use std::path::PathBuf;

fn entry_with_risk_and_path(risk: RiskLevel, path: &str, desc: Option<&str>) -> ClassifiedEntry {
    ClassifiedEntry {
        entry: ScriptEntry {
            discovered: DiscoveredFile {
                path: PathBuf::from(path),
                size: 1234,
                is_executable: true,
            },
            language: Language::Bash,
            description: desc.map(str::to_string),
            sidecar: None,
            sidecar_warning: None,
        },
        classification: Classification {
            risk,
            category: "script".into(),
            owner: "user".into(),
        },
    }
}

fn entry_with_risk(risk: RiskLevel) -> ClassifiedEntry {
    entry_with_risk_and_path(risk, "/x/tool.sh", None)
}

#[test]
fn risk_breakdown_is_ordered_and_omits_zeros() {
    let entries = [
        entry_with_risk(RiskLevel::High),
        entry_with_risk(RiskLevel::Low),
        entry_with_risk(RiskLevel::Low),
    ];
    assert_eq!(risk_breakdown(&entries), vec!["2 low", "1 high"]);

    let all_low = [entry_with_risk(RiskLevel::Low)];
    assert_eq!(risk_breakdown(&all_low), vec!["1 low"]);

    assert!(risk_breakdown(&[]).is_empty());
}

#[test]
fn color_choice_explicit_modes_are_deterministic() {
    assert!(ColorChoice::Always.use_color());
    assert!(!ColorChoice::Never.use_color());
}

#[test]
fn dynamic_widths_respect_caps_and_produce_aligned_output() {
    let long_path = format!("/home/user/projects/{}", "x".repeat(80));
    let long_desc = "This description is deliberately longer than fifty characters so we can prove truncation and width capping work together.";
    let entries = [entry_with_risk_and_path(
        RiskLevel::Low,
        &long_path,
        Some(long_desc),
    )];

    let widths = widths::compute_column_widths(&entries);
    assert!(widths.path <= 72, "path must be capped");
    assert!(widths.desc <= 52, "desc must be capped");

    let header = widths::build_header(&widths);
    let row = render_row(&entries[0], &widths, false);

    assert!(row.contains('…'));
    assert!(header.contains("PATH"));
}

#[test]
fn render_row_visible_width_is_color_independent() {
    let entry = entry_with_risk(RiskLevel::High);
    let widths = widths::compute_column_widths(&[entry]);
    let plain_entry = entry_with_risk(RiskLevel::High);
    let colored_entry = entry_with_risk(RiskLevel::High);

    let plain = render_row(&plain_entry, &widths, false);
    let colored = render_row(&colored_entry, &widths, true);

    assert!(colored.len() > plain.len());
    assert_eq!(strip_ansi(&colored), plain);
}

#[test]
fn truncation_respects_allocated_column_width() {
    let measured = entry_with_risk_and_path(
        RiskLevel::Low,
        "/a/very/long/path/that/will/definitely/be/truncated/tool.sh",
        Some("This description will also be truncated to the computed desc width"),
    );
    let rendered = entry_with_risk_and_path(
        RiskLevel::Low,
        "/a/very/long/path/that/will/definitely/be/truncated/tool.sh",
        Some("This description will also be truncated to the computed desc width"),
    );
    let widths = widths::compute_column_widths(&[measured]);

    let row = render_row(&rendered, &widths, false);
    let first_field = row.split_whitespace().next().unwrap_or_default();

    assert!(first_field.chars().count() <= widths.path + 1);
}

/// Minimal ANSI escape stripper for width-invariance tests.
fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for ch in chars.by_ref() {
                if ch == 'm' {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}
