//! Rendering. Turns the timeline (a list of manifests), one capture's sources,
//! or a capture confirmation into either human output or the suite's JSON
//! envelope. Color follows the suite rule (TTY + `NO_COLOR`, force-off via
//! `--no-color`); tables are aligned by hand so they read the same with color
//! stripped. Structure mirrors tripwire's report.rs. The library does the work;
//! these functions only present it.

use serde::Serialize;

use crate::diff::{Change, ChangeKind, Diff};
use crate::model::{CaptureEntry, EntryKind, Manifest, SnapshotState};
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

/// The aligned capture table. The newest capture (row 0) is flagged `latest` in
/// a leading marker column so the addressable head is obvious at a glance.
fn print_timeline_table(manifests: &[Manifest], style: &Style) {
    let rows: Vec<TimelineRow> = manifests
        .iter()
        .enumerate()
        .map(|(i, m)| TimelineRow::of(m, i == 0))
        .collect();

    let w_mark = col_width(&rows, |r| &r.mark, 6);
    let w_id = col_width(&rows, |r| &r.id, 8);
    let w_when = col_width(&rows, |r| &r.when, 4);
    let w_label = col_width(&rows, |r| &r.label, 5);
    let w_paths = col_width(&rows, |r| &r.paths, 5);
    let w_size = col_width(&rows, |r| &r.size, 4);

    println!(
        "{}{:<wm$}  {:<wi$}  {:<ww$}  {:<wl$}  {:>wp$}  {:>ws$}  NOTE{}",
        style.bold,
        "",
        "ID",
        "WHEN",
        "LABEL",
        "PATHS",
        "SIZE",
        style.rst,
        wm = w_mark,
        wi = w_id,
        ww = w_when,
        wl = w_label,
        wp = w_paths,
        ws = w_size,
    );

    for (m, r) in manifests.iter().zip(&rows) {
        let (note_txt, note_col) = note_cell(m, style);
        println!(
            "{}{:<wm$}{}  {}{:<wi$}{}  {}{:<ww$}{}  {:<wl$}  {:>wp$}  {:>ws$}  {}{}{}",
            style.cyn,
            r.mark,
            style.rst,
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
            wm = w_mark,
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
    mark: String,
    id: String,
    when: String,
    label: String,
    paths: String,
    size: String,
}

impl TimelineRow {
    fn of(m: &Manifest, is_latest: bool) -> Self {
        TimelineRow {
            mark: if is_latest {
                "latest".into()
            } else {
                String::new()
            },
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

/// Width of a column = widest cell, floored at a minimum. Generic over the row
/// type so the timeline, `show`, and `diff` tables share one aligner.
fn col_width<R>(rows: &[R], field: impl Fn(&R) -> &String, min: usize) -> usize {
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

// ---- Show view (`rewind show <capture>`) ----------------------------------

/// Print one capture's manifest: header line, then a row per captured path.
/// Default columns are kind/size/note/path; `-v` adds mode, uid/gid, mtime, and
/// the hash prefix. The per-path note carries unreadable/symlink/envelope state
/// in a word, never color alone.
pub fn print_show(m: &Manifest, verbose: bool, style: &Style) {
    println!(
        "{}{}rewind show{} {}— {} ({}){}",
        style.bold,
        style.cyn,
        style.rst,
        style.dim,
        short_id(&m.id),
        short_when(&m.captured_at),
        style.rst
    );
    let label = m.label.as_deref().unwrap_or("(none)");
    println!(
        "{}label: {} · set: {} · {} {} · {}{}",
        style.dim,
        label,
        m.set_source,
        m.path_count(),
        plural(m.path_count(), "path", "paths"),
        human_size(m.total_bytes()),
        style.rst
    );
    println!();

    if m.entries.is_empty() {
        println!("{}(no paths captured){}", style.dim, style.rst);
        return;
    }

    let rows: Vec<ShowRow> = m.entries.iter().map(ShowRow::of).collect();
    let w_kind = col_width(&rows, |r| &r.kind, 4);
    let w_size = col_width(&rows, |r| &r.size, 4);

    if verbose {
        let w_mode = col_width(&rows, |r| &r.mode, 4);
        let w_owner = col_width(&rows, |r| &r.owner, 5);
        let w_hash = col_width(&rows, |r| &r.hash, 7);
        let w_mtime = col_width(&rows, |r| &r.mtime, 5);
        println!(
            "{}{:<wk$}  {:>ws$}  {:<wm$}  {:<wo$}  {:<wh$}  {:<wt$}  PATH{}",
            style.bold,
            "KIND",
            "SIZE",
            "MODE",
            "OWNER",
            "HASH",
            "MTIME",
            style.rst,
            wk = w_kind,
            ws = w_size,
            wm = w_mode,
            wo = w_owner,
            wh = w_hash,
            wt = w_mtime,
        );
        for r in &rows {
            println!(
                "{:<wk$}  {:>ws$}  {:<wm$}  {:<wo$}  {}{:<wh$}{}  {}{:<wt$}{}  {}{}",
                r.kind,
                r.size,
                r.mode,
                r.owner,
                style.dim,
                r.hash,
                style.rst,
                style.dim,
                r.mtime,
                style.rst,
                r.path,
                note_suffix(&r.note, style),
                wk = w_kind,
                ws = w_size,
                wm = w_mode,
                wo = w_owner,
                wh = w_hash,
                wt = w_mtime,
            );
        }
    } else {
        println!(
            "{}{:<wk$}  {:>ws$}  PATH{}",
            style.bold,
            "KIND",
            "SIZE",
            style.rst,
            wk = w_kind,
            ws = w_size,
        );
        for r in &rows {
            println!(
                "{:<wk$}  {:>ws$}  {}{}",
                r.kind,
                r.size,
                r.path,
                note_suffix(&r.note, style),
                wk = w_kind,
                ws = w_size,
            );
        }
    }
}

/// A pre-rendered show row. Cells are `—` when the value doesn't apply (a dir has
/// no size/hash), so columns stay aligned and nothing reads as a fake zero.
struct ShowRow {
    kind: String,
    size: String,
    mode: String,
    owner: String,
    hash: String,
    mtime: String,
    path: String,
    note: String,
}

impl ShowRow {
    fn of(e: &CaptureEntry) -> Self {
        ShowRow {
            kind: e.kind.tag().to_string(),
            size: e.size.map(human_size).unwrap_or_else(|| "—".into()),
            mode: e.mode.clone().unwrap_or_else(|| "—".into()),
            owner: match (e.uid, e.gid) {
                (Some(u), Some(g)) => format!("{u}:{g}"),
                _ => "—".into(),
            },
            hash: e
                .hash
                .as_deref()
                .map(|h| h.chars().take(7).collect())
                .unwrap_or_else(|| "—".into()),
            mtime: e.mtime.clone().unwrap_or_else(|| "—".into()),
            path: e.path.clone(),
            note: entry_note(e),
        }
    }
}

/// The per-entry note word, in priority order: unreadable wins (it's not
/// restorable), then a symlink shows its target, then a recognized envelope, else
/// blank. Carries state by word so `show` reads the same with color stripped.
fn entry_note(e: &CaptureEntry) -> String {
    if e.unreadable {
        return "unreadable".into();
    }
    if e.kind == EntryKind::Symlink {
        return match &e.target {
            Some(t) => format!("→ {t}"),
            None => "symlink".into(),
        };
    }
    match (&e.envelope_tool, e.envelope_schema_version) {
        (Some(tool), Some(v)) => format!("{tool} v{v}"),
        (Some(tool), None) => tool.clone(),
        _ => String::new(),
    }
}

/// Render a note as a dim trailing `  (note)`, or nothing when blank.
fn note_suffix(note: &str, style: &Style) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!("  {}({}){}", style.dim, note, style.rst)
    }
}

// ---- Diff view (`rewind diff <a> [<b>]`) ----------------------------------

/// Print a diff: a header naming both points, a row per changed/added/removed
/// path (unchanged paths are summarized, not listed), and a zero-suppressed
/// footer. Markers reuse suite vocabulary (`+ - ~ =`) and each line carries a
/// verb word so the state survives `NO_COLOR`.
pub fn print_diff(d: &Diff, style: &Style) {
    println!(
        "{}{}rewind diff{} {}— {} → {}{}",
        style.bold, style.cyn, style.rst, style.dim, d.from, d.to, style.rst
    );
    println!();

    if d.is_clean() {
        println!(
            "{}no differences — {} {} unchanged{}",
            style.dim,
            d.unchanged,
            plural(d.unchanged, "path", "paths"),
            style.rst
        );
        return;
    }

    // Only list the paths that actually differ; unchanged are counted, not shown.
    for c in d.changes.iter().filter(|c| c.kind != ChangeKind::Unchanged) {
        let (verb, col) = diff_verb(c.kind, style);
        let detail = diff_detail(c);
        println!(
            "  {}{}{} {}{:<9}{} {}{}{}",
            col,
            c.kind.marker(),
            style.rst,
            col,
            verb,
            style.rst,
            c.path,
            if detail.is_empty() { "" } else { "   " },
            detail,
        );
    }

    println!();
    println!("{}", diff_footer(d));
}

/// The verb word + color for a change kind.
fn diff_verb(kind: ChangeKind, style: &Style) -> (&'static str, &str) {
    match kind {
        ChangeKind::Added => ("added", style.grn),
        ChangeKind::Removed => ("removed", style.red),
        ChangeKind::Changed => ("changed", style.ylw),
        ChangeKind::Unchanged => ("unchanged", style.dim),
    }
}

/// The trailing detail for a change row: size transition and, for a recognized
/// envelope, the schema transition. Empty for added/removed where only one side
/// has data worth a transition arrow.
fn diff_detail(c: &Change) -> String {
    let mut parts = Vec::new();
    match c.kind {
        ChangeKind::Changed => {
            if let (Some(a), Some(b)) = (c.was_bytes, c.now_bytes) {
                parts.push(format!("{} → {}", human_size(a), human_size(b)));
            }
            match (c.was_schema, c.now_schema) {
                (Some(a), Some(b)) if a != b => parts.push(format!("schema {a} → {b}")),
                (Some(a), Some(b)) => parts.push(format!("schema {a} = {b}")),
                _ => {}
            }
        }
        ChangeKind::Added => {
            if let Some(b) = c.now_bytes {
                parts.push(human_size(b));
            }
        }
        ChangeKind::Removed => {
            if let Some(a) = c.was_bytes {
                parts.push(format!("was {}", human_size(a)));
            }
        }
        ChangeKind::Unchanged => {}
    }
    parts.join("   ")
}

/// The zero-suppressed footer: only non-zero categories, e.g.
/// `1 changed · 1 added · 1 unchanged`. Pure (no stdout) so it's unit-testable.
fn diff_footer(d: &Diff) -> String {
    let mut parts = Vec::new();
    if d.changed > 0 {
        parts.push(format!("{} changed", d.changed));
    }
    if d.added > 0 {
        parts.push(format!("{} added", d.added));
    }
    if d.removed > 0 {
        parts.push(format!("{} removed", d.removed));
    }
    if d.unchanged > 0 {
        parts.push(format!("{} unchanged", d.unchanged));
    }
    if parts.is_empty() {
        "no differences".to_string()
    } else {
        parts.join(" · ")
    }
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
/// `{"source_tool":...,"action":"baseline",...}` confirmation. A typed struct
/// (not an ad-hoc `json!`) so an absent `label` is *omitted*, never `null` —
/// "the absence is the signal," matching every other envelope in the suite.
#[derive(Serialize)]
struct CaptureEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    action: &'static str,
    id: &'a str,
    captured_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: &'a Option<String>,
    set_source: &'a str,
    path_count: usize,
    bytes: u64,
}

pub fn capture_json(m: &Manifest) -> String {
    let env = CaptureEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        action: "capture",
        id: &m.id,
        captured_at: &m.captured_at,
        label: &m.label,
        set_source: &m.set_source,
        path_count: m.path_count(),
        bytes: m.total_bytes(),
    };
    serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string())
}

/// The `rewind show` JSON envelope: the capture's metadata plus its full
/// per-path manifest entries (each entry already serializes optional fields away
/// when absent, per [`CaptureEntry`]).
#[derive(Serialize)]
struct ShowEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    capture: CaptureOut<'a>,
}

#[derive(Serialize)]
struct CaptureOut<'a> {
    id: &'a str,
    captured_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: &'a Option<String>,
    set_source: &'a str,
    entries: &'a [CaptureEntry],
}

/// Render one capture as the suite `show` JSON envelope.
pub fn show_json(m: &Manifest) -> String {
    let env = ShowEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        capture: CaptureOut {
            id: &m.id,
            captured_at: &m.captured_at,
            label: &m.label,
            set_source: &m.set_source,
            entries: &m.entries,
        },
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// The `rewind diff` JSON envelope. Each change carries both sides (absent fields
/// omitted by [`Change`]'s own serde skips) so a consumer renders without a
/// re-lookup. `to` is a capture id prefix, or the literal `"live"`.
#[derive(Serialize)]
struct DiffEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    from: &'a str,
    to: &'a str,
    clean: bool,
    changed: usize,
    added: usize,
    removed: usize,
    unchanged: usize,
    changes: &'a [Change],
}

/// Render a diff as the suite `diff` JSON envelope.
pub fn diff_json(d: &Diff) -> String {
    let env = DiffEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        from: &d.from,
        to: &d.to,
        clean: d.clean,
        changed: d.changed,
        added: d.added,
        removed: d.removed,
        unchanged: d.unchanged,
        changes: &d.changes,
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
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
        // A present label is carried.
        assert_eq!(v["label"], "pre-upgrade");
    }

    #[test]
    fn capture_json_omits_absent_label_not_null() {
        // "the absence is the signal": an unlabeled capture has no `label` key,
        // never `"label": null`.
        let mut m = manifest("abc", "2026-06-19T14:22:05Z", None);
        m.label = None;
        let v: serde_json::Value = serde_json::from_str(&capture_json(&m)).unwrap();
        assert!(v.get("label").is_none(), "absent label must be omitted");
    }

    // ---- Phase 2: timeline marker ----------------------------------------

    #[test]
    fn timeline_marker_only_on_newest_and_json_unchanged() {
        let newest = TimelineRow::of(&manifest("aaa", "2026-06-19T00:00:00Z", None), true);
        let older = TimelineRow::of(&manifest("bbb", "2026-06-18T00:00:00Z", None), false);
        assert_eq!(newest.mark, "latest");
        assert_eq!(older.mark, "");

        // The marker is a human-view affordance only; JSON carries no "latest".
        let json = timeline_json(
            &[manifest("aaa", "2026-06-19T00:00:00Z", None)],
            10,
            "/store",
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["captures"][0].get("latest").is_none());
    }

    // ---- Phase 2: show ----------------------------------------------------

    fn entry(path: &str, kind: EntryKind) -> CaptureEntry {
        CaptureEntry {
            path: path.into(),
            kind,
            size: matches!(kind, EntryKind::File).then_some(8192),
            mode: Some("0644".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: Some("2026-06-19T14:21:00Z".into()),
            hash: matches!(kind, EntryKind::File).then(|| "a17b9e00".to_string()),
            target: None,
            envelope_tool: None,
            envelope_schema_version: None,
            unreadable: false,
        }
    }

    #[test]
    fn show_json_has_capture_and_entries() {
        let mut m = manifest("3f9c1a", "2026-06-19T14:22:05Z", Some("workstate"));
        m.entries.push(entry("/d/extra.json", EntryKind::File));
        let json = show_json(&m);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["capture"]["id"], "3f9c1a");
        assert_eq!(v["capture"]["set_source"], "builtin");
        assert_eq!(v["capture"]["entries"].as_array().unwrap().len(), 2);
        // Optional absent fields are omitted by CaptureEntry's own serde skips.
        assert_eq!(v["capture"]["entries"][0]["envelope_tool"], "workstate");
    }

    #[test]
    fn entry_note_priority_unreadable_symlink_envelope() {
        // Unreadable wins over everything.
        let mut e = entry("/x", EntryKind::File);
        e.unreadable = true;
        assert_eq!(entry_note(&e), "unreadable");

        // Symlink shows its target.
        let mut link = entry("/l", EntryKind::Symlink);
        link.target = Some("/real".into());
        assert_eq!(entry_note(&link), "→ /real");

        // A recognized envelope is labeled tool + version.
        let mut snap = entry("/s", EntryKind::File);
        snap.envelope_tool = Some("workstate".into());
        snap.envelope_schema_version = Some(4);
        assert_eq!(entry_note(&snap), "workstate v4");

        // A plain file with nothing notable -> blank.
        assert_eq!(entry_note(&entry("/p", EntryKind::File)), "");
    }

    #[test]
    fn show_row_dir_size_is_emdash_not_zero() {
        let r = ShowRow::of(&entry("/d", EntryKind::Dir));
        assert_eq!(r.size, "—", "a dir has no size -> em-dash, not a fake 0");
        assert_eq!(r.hash, "—");
    }

    // ---- Phase 2: diff ----------------------------------------------------

    fn diff_with(changes: Vec<Change>) -> Diff {
        let (mut changed, mut added, mut removed, mut unchanged) = (0, 0, 0, 0);
        for c in &changes {
            match c.kind {
                ChangeKind::Changed => changed += 1,
                ChangeKind::Added => added += 1,
                ChangeKind::Removed => removed += 1,
                ChangeKind::Unchanged => unchanged += 1,
            }
        }
        Diff {
            from: "c0de".into(),
            to: "3f9c".into(),
            clean: changed + added + removed == 0,
            changed,
            added,
            removed,
            unchanged,
            changes,
        }
    }

    fn change(kind: ChangeKind, path: &str) -> Change {
        Change {
            kind,
            path: path.into(),
            was_hash: None,
            now_hash: None,
            was_bytes: Some(7900),
            now_bytes: Some(8281),
            was_schema: Some(4),
            now_schema: Some(4),
        }
    }

    #[test]
    fn diff_footer_zero_suppresses_categories() {
        let d = diff_with(vec![
            change(ChangeKind::Changed, "/a"),
            change(ChangeKind::Added, "/b"),
            change(ChangeKind::Unchanged, "/c"),
        ]);
        // No "removed" category (zero) appears.
        assert_eq!(diff_footer(&d), "1 changed · 1 added · 1 unchanged");

        // An all-unchanged (clean) diff.
        let clean = diff_with(vec![change(ChangeKind::Unchanged, "/c")]);
        assert_eq!(diff_footer(&clean), "1 unchanged");

        // A totally empty diff.
        let empty = diff_with(vec![]);
        assert_eq!(diff_footer(&empty), "no differences");
    }

    #[test]
    fn diff_verb_words_carry_state_without_color() {
        let plain = Style::resolve(true); // color forced off
        for (kind, word) in [
            (ChangeKind::Added, "added"),
            (ChangeKind::Removed, "removed"),
            (ChangeKind::Changed, "changed"),
            (ChangeKind::Unchanged, "unchanged"),
        ] {
            assert_eq!(diff_verb(kind, &plain).0, word);
        }
    }

    #[test]
    fn diff_detail_renders_size_and_schema_transitions() {
        let mut c = change(ChangeKind::Changed, "/x");
        c.was_bytes = Some(7900);
        c.now_bytes = Some(8281);
        c.was_schema = Some(4);
        c.now_schema = Some(5);
        let detail = diff_detail(&c);
        assert!(detail.contains("→"));
        assert!(detail.contains("schema 4 → 5"));

        // Same schema renders as `=`.
        c.now_schema = Some(4);
        assert!(diff_detail(&c).contains("schema 4 = 4"));
    }

    #[test]
    fn diff_json_carries_both_sides_and_clean_flag() {
        let d = diff_with(vec![change(ChangeKind::Changed, "/x")]);
        let json = diff_json(&d);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["clean"], false);
        assert_eq!(v["changed"], 1);
        assert_eq!(v["changes"][0]["kind"], "changed");
        assert_eq!(v["changes"][0]["was_bytes"], 7900);
        assert_eq!(v["changes"][0]["now_bytes"], 8281);
    }

    #[test]
    fn diff_json_to_live_label() {
        let mut d = diff_with(vec![]);
        d.to = "live".into();
        let v: serde_json::Value = serde_json::from_str(&diff_json(&d)).unwrap();
        assert_eq!(v["to"], "live");
        assert_eq!(v["clean"], true);
    }
}
