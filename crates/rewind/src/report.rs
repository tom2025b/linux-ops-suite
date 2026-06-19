//! Rendering. Turns the timeline (a list of manifests), one capture's sources,
//! or a capture confirmation into either human output or the suite's JSON
//! envelope. Color follows the suite rule (TTY + `NO_COLOR`, force-off via
//! `--no-color`); tables are aligned by hand so they read the same with color
//! stripped. Structure mirrors tripwire's report.rs. The library does the work;
//! these functions only present it.

use serde::Serialize;

use crate::model::{Manifest, SnapshotState};
use crate::set::CaptureSet;
use crate::util;

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as tripwire/portman.
pub struct Style {
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub grn: &'static str,
    pub ylw: &'static str,
    pub cyn: &'static str,
    pub rst: &'static str,
}

impl Style {
    /// Color on only when stdout is a TTY and `NO_COLOR` is unset, unless forced
    /// off (`--no-color`).
    pub fn resolve(force_off: bool) -> Self {
        let on = !force_off && util::stdout_is_tty() && std::env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                red: "\u{1b}[31m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Style {
                bold: "",
                dim: "",
                red: "",
                grn: "",
                ylw: "",
                cyn: "",
                rst: "",
            }
        }
    }
}

/// Human-readable byte size: `563 B`, `2.1 KB`, `3.4 MB`. Dependency-free.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    format!("{val:.1} {}", UNITS[unit])
}

/// A short id prefix for the timeline, matching the on-disk filename prefix.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Trim the `Z`/seconds-precision RFC3339 timestamp to `YYYY-MM-DD HH:MM` for the
/// timeline's WHEN column. Falls back to the raw string if it's an odd shape.
fn short_when(captured_at: &str) -> String {
    // "2026-06-19T14:22:05Z" -> "2026-06-19 14:22"
    if captured_at.len() >= 16 && captured_at.as_bytes().get(10) == Some(&b'T') {
        let date = &captured_at[..10];
        let time = &captured_at[11..16];
        format!("{date} {time}")
    } else {
        captured_at.to_string()
    }
}

// ---- Timeline view (`rewind` / `rewind log`) ------------------------------

/// Print the capture timeline, newest first, plus a store-stats footer.
pub fn print_timeline(manifests: &[Manifest], store_bytes: u64, store_path: &str, style: &Style) {
    println!(
        "{}{}rewind{} {}— capture history (newest first){}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    if manifests.is_empty() {
        println!(
            "{}No captures yet. Run `rewind capture` to record one.{}",
            style.dim, style.rst
        );
        return;
    }

    print_timeline_table(manifests, style);

    println!();
    println!(
        "{}{} {} · {} on disk (deduped) · store: {}{}",
        style.dim,
        manifests.len(),
        plural(manifests.len(), "capture", "captures"),
        human_size(store_bytes),
        store_path,
        style.rst
    );
}

/// The aligned capture table.
fn print_timeline_table(manifests: &[Manifest], style: &Style) {
    let rows: Vec<TimelineRow> = manifests.iter().map(TimelineRow::of).collect();

    let w_id = col_width(&rows, |r| &r.id, 8);
    let w_when = col_width(&rows, |r| &r.when, 4);
    let w_label = col_width(&rows, |r| &r.label, 5);
    let w_paths = col_width(&rows, |r| &r.paths, 5);
    let w_size = col_width(&rows, |r| &r.size, 4);

    println!(
        "{}{:<wi$}  {:<ww$}  {:<wl$}  {:>wp$}  {:>ws$}  NOTE{}",
        style.bold,
        "ID",
        "WHEN",
        "LABEL",
        "PATHS",
        "SIZE",
        style.rst,
        wi = w_id,
        ww = w_when,
        wl = w_label,
        wp = w_paths,
        ws = w_size,
    );

    for (m, r) in manifests.iter().zip(&rows) {
        let (note_txt, note_col) = note_cell(m, style);
        println!(
            "{}{:<wi$}{}  {}{:<ww$}{}  {:<wl$}  {:>wp$}  {:>ws$}  {}{}{}",
            style.dim,
            r.id,
            style.rst,
            style.dim,
            r.when,
            style.rst,
            r.label,
            r.paths,
            r.size,
            note_col,
            note_txt,
            style.rst,
            wi = w_id,
            ww = w_when,
            wl = w_label,
            wp = w_paths,
            ws = w_size,
        );
    }
}

/// A pre-rendered timeline row.
struct TimelineRow {
    id: String,
    when: String,
    label: String,
    paths: String,
    size: String,
}

impl TimelineRow {
    fn of(m: &Manifest) -> Self {
        TimelineRow {
            id: short_id(&m.id),
            when: short_when(&m.captured_at),
            label: m.label.clone().unwrap_or_else(|| "(none)".into()),
            paths: m.path_count().to_string(),
            size: human_size(m.total_bytes()),
        }
    }
}

/// The NOTE cell: the snapshot health word, colored only when it warns.
fn note_cell<'a>(m: &Manifest, style: &'a Style) -> (&'static str, &'a str) {
    match m.snapshot_state() {
        SnapshotState::Good => ("good", style.grn),
        SnapshotState::Invalid => ("snapshot invalid", style.red),
        SnapshotState::Absent => ("—", style.dim),
    }
}

/// Width of a column = widest cell, floored at a minimum.
fn col_width(rows: &[TimelineRow], field: impl Fn(&TimelineRow) -> &String, min: usize) -> usize {
    rows.iter()
        .map(|r| field(r).len())
        .max()
        .unwrap_or(0)
        .max(min)
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

// ---- Sources view (`rewind sources`) --------------------------------------

/// Print the resolved capture set and where it came from, plus store stats — the
/// analogue of `tripwire watch`. Lets the operator see exactly what a capture
/// will cover before running one.
pub fn print_sources(
    set: &CaptureSet,
    store_bytes: u64,
    capture_count: usize,
    store_path: &str,
    style: &Style,
) {
    println!(
        "{}{}rewind sources{} {}— the capture set ({} source){}",
        style.bold,
        style.cyn,
        style.rst,
        style.dim,
        set.source.tag(),
        style.rst
    );
    println!();
    for spec in &set.specs {
        let mut notes = Vec::new();
        if !spec.recursive {
            notes.push("non-recursive".to_string());
        }
        if spec.follow_symlinks {
            notes.push("follow-symlinks".to_string());
        }
        if !spec.exclude.is_empty() {
            notes.push(format!("exclude {}", spec.exclude.join(",")));
        }
        let suffix = if notes.is_empty() {
            String::new()
        } else {
            format!("  {}({}){}", style.dim, notes.join(", "), style.rst)
        };
        println!("  {}{}", spec.path.display(), suffix);
    }
    println!();
    println!(
        "{}{} {} · source: {} · {} stored · {} on disk · store: {}{}",
        style.dim,
        set.specs.len(),
        plural(set.specs.len(), "path", "paths"),
        set.source.tag(),
        capture_count,
        human_size(store_bytes),
        store_path,
        style.rst
    );
}

// ---- JSON envelopes -------------------------------------------------------

/// One timeline entry in the JSON envelope.
#[derive(Serialize)]
struct CaptureSummary {
    id: String,
    captured_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    set_source: String,
    snapshot_valid: bool,
    path_count: usize,
    bytes: u64,
}

impl CaptureSummary {
    fn of(m: &Manifest) -> Self {
        CaptureSummary {
            id: m.id.clone(),
            captured_at: m.captured_at.clone(),
            label: m.label.clone(),
            set_source: m.set_source.clone(),
            snapshot_valid: m.has_valid_snapshot(),
            path_count: m.path_count(),
            bytes: m.total_bytes(),
        }
    }
}

/// The `rewind` / `rewind log` JSON envelope.
#[derive(Serialize)]
struct TimelineEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    store: String,
    capture_count: usize,
    store_bytes: u64,
    captures: Vec<CaptureSummary>,
}

/// Render the timeline as the suite JSON envelope.
pub fn timeline_json(manifests: &[Manifest], store_bytes: u64, store_path: &str) -> String {
    let env = TimelineEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        store: store_path.to_string(),
        capture_count: manifests.len(),
        store_bytes,
        captures: manifests.iter().map(CaptureSummary::of).collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// One spec in the sources JSON envelope.
#[derive(Serialize)]
struct SpecOut {
    path: String,
    recursive: bool,
    #[serde(skip_serializing_if = "is_false")]
    follow_symlinks: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The `rewind sources` JSON envelope.
#[derive(Serialize)]
struct SourcesEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    set_source: &'static str,
    store: String,
    capture_count: usize,
    store_bytes: u64,
    specs: Vec<SpecOut>,
}

/// Render the resolved capture set as the suite JSON envelope.
pub fn sources_json(
    set: &CaptureSet,
    store_bytes: u64,
    capture_count: usize,
    store_path: &str,
) -> String {
    let env = SourcesEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        set_source: set.source.tag(),
        store: store_path.to_string(),
        capture_count,
        store_bytes,
        specs: set
            .specs
            .iter()
            .map(|s| SpecOut {
                path: s.path.to_string_lossy().into_owned(),
                recursive: s.recursive,
                follow_symlinks: s.follow_symlinks,
                exclude: s.exclude.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// The one-line capture-confirmation JSON envelope, shaped like tripwire's
/// `{"source_tool":...,"action":"baseline",...}` confirmation.
pub fn capture_json(m: &Manifest) -> String {
    let env = serde_json::json!({
        "schema_version": 1,
        "source_tool": "rewind",
        "action": "capture",
        "id": m.id,
        "captured_at": m.captured_at,
        "label": m.label,
        "set_source": m.set_source,
        "path_count": m.path_count(),
        "bytes": m.total_bytes(),
    });
    serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CaptureEntry, EntryKind, MANIFEST_SCHEMA_VERSION};

    fn manifest(id: &str, at: &str, tool: Option<&str>) -> Manifest {
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source_tool: "rewind".into(),
            id: id.into(),
            captured_at: at.into(),
            label: Some("pre-upgrade".into()),
            set_source: "builtin".into(),
            entries: vec![CaptureEntry {
                path: "/d/workstate.snapshot.json".into(),
                kind: EntryKind::File,
                size: Some(8192),
                mode: Some("0644".into()),
                uid: Some(1000),
                gid: Some(1000),
                mtime: None,
                hash: Some("a17b".into()),
                target: None,
                envelope_tool: tool.map(str::to_string),
                envelope_schema_version: tool.map(|_| 4),
                unreadable: false,
            }],
        }
    }

    #[test]
    fn human_size_formats_units() {
        assert_eq!(human_size(563), "563 B");
        assert_eq!(human_size(2150), "2.1 KB");
    }

    #[test]
    fn short_when_trims_to_minute() {
        assert_eq!(short_when("2026-06-19T14:22:05Z"), "2026-06-19 14:22");
        assert_eq!(short_when("weird"), "weird");
    }

    #[test]
    fn timeline_json_has_envelope_shape() {
        let ms = vec![manifest(
            "abc12345",
            "2026-06-19T14:22:05Z",
            Some("workstate"),
        )];
        let json = timeline_json(&ms, 8192, "/store");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["capture_count"], 1);
        assert_eq!(v["captures"][0]["snapshot_valid"], true);
        assert_eq!(v["captures"][0]["path_count"], 1);
        assert_eq!(v["captures"][0]["bytes"], 8192);
    }

    #[test]
    fn timeline_json_marks_invalid_snapshot() {
        let ms = vec![manifest("def", "2026-06-18T02:00:00Z", None)];
        let json = timeline_json(&ms, 100, "/store");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["captures"][0]["snapshot_valid"], false);
    }

    #[test]
    fn capture_json_is_a_confirmation_envelope() {
        let json = capture_json(&manifest("abc", "2026-06-19T14:22:05Z", Some("workstate")));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["action"], "capture");
        assert_eq!(v["id"], "abc");
        assert_eq!(v["path_count"], 1);
    }
}
