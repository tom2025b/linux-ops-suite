use scriptvault_core::SearchResult;

use crate::cli::match_label;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColorChoice {
    enabled: bool,
}

impl ColorChoice {
    pub(crate) fn from_stdout(stdout_is_terminal: bool) -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Self {
            enabled: stdout_is_terminal && !no_color,
        }
    }

    #[cfg(test)]
    fn enabled(enabled: bool) -> Self {
        Self { enabled }
    }
}

#[derive(Debug, Clone, Copy)]
struct Widths {
    name: usize,
    lang: usize,
    matched: usize,
    path: usize,
    summary: usize,
}

pub(crate) fn render_results_table(results: &[SearchResult], color: ColorChoice) -> String {
    if results.is_empty() {
        return String::new();
    }

    let widths = compute_widths(results, terminal_width());
    let header = build_header(widths);
    let divider = "─".repeat(table_width(widths));

    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&divider);
    out.push('\n');

    for result in results {
        out.push_str(&render_row(result, widths, color));
        out.push('\n');
    }

    out
}

fn compute_widths(results: &[SearchResult], terminal_width: Option<usize>) -> Widths {
    let mut widths = Widths {
        name: "NAME".len(),
        lang: "LANG".len(),
        matched: "MATCH".len(),
        path: "PATH".len(),
        summary: "SUMMARY".len(),
    };

    for result in results {
        widths.name = widths.name.max(display_width(result.entry.display_name()));
        widths.lang = widths.lang.max(display_width(result.entry.lang.label()));
        widths.matched = widths
            .matched
            .max(display_width(result_match_label(result)));
        widths.path = widths
            .path
            .max(display_width(&result.entry.path.display().to_string()));
        widths.summary = widths.summary.max(display_width(&summary(result)));
    }

    widths.name = widths.name.min(MAX_NAME);
    widths.lang = widths.lang.min(MAX_LANG);
    widths.matched = widths.matched.min(MAX_MATCH);
    widths.path = widths.path.min(MAX_PATH);
    widths.summary = widths.summary.min(MAX_SUMMARY);

    fit_widths_to_terminal(&mut widths, terminal_width);
    widths
}

const MAX_NAME: usize = 28;
const MAX_LANG: usize = 7;
const MAX_MATCH: usize = 5;
const MAX_PATH: usize = 48;
const MAX_SUMMARY: usize = 56;

const MIN_NAME: usize = 12;
const MIN_PATH: usize = 18;
const MIN_SUMMARY: usize = 0;
fn terminal_width() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 50)
}

fn fit_widths_to_terminal(widths: &mut Widths, terminal_width: Option<usize>) {
    let Some(target) = terminal_width else {
        return;
    };

    while table_width(*widths) > target {
        if shrink(&mut widths.summary, MIN_SUMMARY) {
            continue;
        }
        if shrink(&mut widths.path, MIN_PATH) {
            continue;
        }
        if shrink(&mut widths.name, MIN_NAME) {
            continue;
        }
        break;
    }
}

fn shrink(width: &mut usize, min: usize) -> bool {
    if *width > min {
        *width -= 1;
        true
    } else {
        false
    }
}

fn table_width(widths: Widths) -> usize {
    let summary_gap = if widths.summary > 0 { 2 } else { 0 };
    widths.name + widths.lang + widths.matched + widths.path + widths.summary + 3 + summary_gap
}

fn build_header(widths: Widths) -> String {
    let mut header = format!(
        "{:<name_w$} {:<lang_w$} {:<match_w$} {:<path_w$}",
        "NAME",
        "LANG",
        "MATCH",
        "PATH",
        name_w = widths.name,
        lang_w = widths.lang,
        match_w = widths.matched,
        path_w = widths.path,
    );

    if widths.summary > 0 {
        header.push_str("  ");
        header.push_str(&format!(
            "{:<summary_w$}",
            "SUMMARY",
            summary_w = widths.summary
        ));
    }

    header
}

fn render_row(result: &SearchResult, widths: Widths, color: ColorChoice) -> String {
    let matched = pad_right(result_match_label(result), widths.matched);
    let matched = color_match(&matched, color);

    let mut row = format!(
        "{:<name_w$} {:<lang_w$} {} {:<path_w$}",
        truncate(result.entry.display_name(), widths.name),
        truncate(result.entry.lang.label(), widths.lang),
        matched,
        truncate(&result.entry.path.display().to_string(), widths.path),
        name_w = widths.name,
        lang_w = widths.lang,
        path_w = widths.path,
    );

    if widths.summary > 0 {
        row.push_str("  ");
        row.push_str(&pad_right(
            &truncate(&summary(result), widths.summary),
            widths.summary,
        ));
    }

    row
}

fn summary(result: &SearchResult) -> String {
    let desc = result.entry.meta.desc.as_deref().unwrap_or("");
    let tags = if result.entry.meta.tags.is_empty() {
        String::new()
    } else {
        format!("[{}]", result.entry.meta.tags.join(", "))
    };

    match (desc.is_empty(), tags.is_empty()) {
        (true, true) => String::new(),
        (false, true) => desc.to_string(),
        (true, false) => tags,
        (false, false) => format!("{desc} {tags}"),
    }
}

fn result_match_label(result: &SearchResult) -> &'static str {
    if result.score == 0 && result.matched_indices.is_empty() {
        "all"
    } else {
        match_label(result.matched_field)
    }
}

fn pad_right(value: &str, width: usize) -> String {
    let len = display_width(value);
    if len >= width {
        return value.to_string();
    }

    let padding = " ".repeat(width - len);
    format!("{value}{padding}")
}

fn truncate(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    if display_width(value) <= width {
        return value.to_string();
    }

    if width == 1 {
        return "…".to_string();
    }

    let keep = width.saturating_sub(1);
    let mut out: String = value.chars().take(keep).collect();
    out.push('…');
    out
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn color_match(value: &str, color: ColorChoice) -> String {
    if !color.enabled {
        return value.to_string();
    }

    match value.trim() {
        "name" => format!("\x1b[32m{value}\x1b[0m"),
        "tags" => format!("\x1b[36m{value}\x1b[0m"),
        "desc" => format!("\x1b[33m{value}\x1b[0m"),
        "file" => format!("\x1b[35m{value}\x1b[0m"),
        "all" => value.to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scriptvault_core::{Language, MatchField, MetaSource, ScriptEntry, ScriptMetadata};
    use std::path::PathBuf;

    fn result(name: &str, desc: &str, path: &str, matched_field: MatchField) -> SearchResult {
        SearchResult {
            entry: ScriptEntry {
                path: PathBuf::from(path),
                filename: "deploy.sh".to_string(),
                lang: Language::Bash,
                meta: ScriptMetadata {
                    name: Some(name.to_string()),
                    desc: (!desc.is_empty()).then(|| desc.to_string()),
                    tags: vec!["ops".to_string(), "deploy".to_string()],
                    ..Default::default()
                },
                source: MetaSource::Header,
            },
            score: 100,
            matched_field,
            matched_indices: Vec::new(),
        }
    }

    #[test]
    fn table_fits_terminal_width() {
        let results = [result(
            "deploy production database",
            "Ship a database migration to production",
            "/home/tom/projects/scripts/deploy/production/database/deploy.sh",
            MatchField::Name,
        )];

        let widths = compute_widths(&results, Some(80));
        assert!(table_width(widths) <= 80);

        let table = render_results_table(&results, ColorChoice::enabled(false));
        assert!(table.contains("NAME"));
        assert!(table.contains("MATCH"));
    }

    #[test]
    fn color_does_not_change_visible_row() {
        let results = [result(
            "deploy",
            "Ship it",
            "/scripts/deploy.sh",
            MatchField::Tags,
        )];
        let widths = compute_widths(&results, Some(120));
        let plain = render_row(&results[0], widths, ColorChoice::enabled(false));
        let colored = render_row(&results[0], widths, ColorChoice::enabled(true));

        assert!(colored.len() > plain.len());
        assert_eq!(strip_ansi(&colored), plain);
    }

    #[test]
    fn very_narrow_width_omits_summary_before_breaking_columns() {
        let results = [result(
            "long deploy name",
            "A long description",
            "/a/long/path/to/deploy.sh",
            MatchField::Desc,
        )];

        let widths = compute_widths(&results, Some(50));
        assert_eq!(widths.summary, 0);
        assert!(table_width(widths) <= 50);
    }

    #[test]
    fn empty_query_rows_render_as_all_not_internal_placeholder() {
        let results = [SearchResult {
            score: 0,
            matched_indices: Vec::new(),
            ..result("deploy", "", "/scripts/deploy.sh", MatchField::Name)
        }];

        let table = render_results_table(&results, ColorChoice::enabled(false));
        assert!(table.contains(" all "));
        assert!(!table.contains(" name "));
    }

    fn strip_ansi(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut chars = value.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
