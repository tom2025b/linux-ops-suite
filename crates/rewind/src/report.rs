//! Rendering. Turns the timeline (a list of manifests), one capture's sources,
//! or a capture confirmation into either human output or the suite's JSON
//! envelope. Color follows the suite rule (TTY + `NO_COLOR`, force-off via
//! `--no-color`); tables are aligned by hand so they read the same with color
//! stripped. Structure mirrors tripwire's report.rs. The library does the work;
//! these functions only present it.

use serde::Serialize;

use crate::diff::{Change, ChangeKind, Diff};
use crate::model::{CaptureEntry, EntryKind, Manifest, SnapshotState};
use crate::prune::PruneOutcome;
use crate::restore::{
    RestoreAction, RestoreItem, RestoreOutcome, RestoreOutcomeKind, RestorePlan, RestoreResult,
};
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

// ---- Restore view (`rewind restore`) --------------------------------------

/// Print the restore dry-run plan: the amber DRY RUN banner, a row per path
/// (what *would* happen), any schema-downgrade warnings, the safety-capture
/// notice, and a zero-suppressed footer. Writes nothing — this is R2's screen.
pub fn print_restore_plan(p: &RestorePlan, style: &Style) {
    println!(
        "{}{}rewind restore{} {}{}(DRY RUN){} {}— would restore from {} ({}){}",
        style.bold,
        style.cyn,
        style.rst,
        style.bold,
        style.ylw,
        style.rst,
        style.dim,
        p.from,
        short_when(&p.captured_at),
        style.rst
    );
    println!(
        "{}nothing has been written. re-run with --apply to perform the restore.{}",
        style.dim, style.rst
    );
    println!();

    if p.items.is_empty() {
        println!("{}(this capture has no paths){}", style.dim, style.rst);
        return;
    }

    for item in &p.items {
        let (verb, col) = plan_verb(item.action, style);
        let detail = plan_detail(item, style);
        println!(
            "  {}{}{} {}{:<15}{} {}{}",
            col,
            restore_marker(item.action),
            style.rst,
            col,
            verb,
            style.rst,
            item.path,
            detail,
        );
    }

    // R5: a loud per-path schema-downgrade warning, carried by the word.
    for item in p.items.iter().filter(|i| i.schema_downgrade) {
        println!(
            "{}! schema downgrade: {} — an older schema would go under a newer live consumer{}",
            style.ylw, item.path, style.rst
        );
    }

    if p.has_work() {
        println!(
            "{}a safety capture of the current state will be taken before any write.{}",
            style.ylw, style.rst
        );
    }

    println!();
    println!("{}", plan_footer(p));
}

/// Print the apply outcome: the safety-capture id first (R3), a row per path with
/// its past-tense verb, then a zero-suppressed footer that always names failures.
pub fn print_restore_outcome(o: &RestoreOutcome, style: &Style) {
    match &o.safety_capture {
        Some(id) => println!(
            "{}{}rewind restore{} {}— safety capture taken: {}{}",
            style.bold,
            style.cyn,
            style.rst,
            style.dim,
            short_id(id),
            style.rst
        ),
        None => {
            println!("{}{}rewind restore{}", style.bold, style.cyn, style.rst);
            println!(
                "{}! no safety capture taken (--no-safety-capture){}",
                style.ylw, style.rst
            );
        }
    }
    println!();

    for r in &o.results {
        let (verb, col) = outcome_verb(r.outcome, style);
        let detail = outcome_detail(r, style);
        println!(
            "  {}{}{} {}{:<9}{} {}{}",
            col,
            outcome_marker(r.outcome),
            style.rst,
            col,
            verb,
            style.rst,
            r.path,
            detail,
        );
    }

    println!();
    println!("{}", outcome_footer(o, style));
}

/// Marker glyph for a planned action — suite vocabulary plus `!` for a skip.
fn restore_marker(a: RestoreAction) -> char {
    match a {
        RestoreAction::WouldOverwrite => '~',
        RestoreAction::WouldCreate => '+',
        RestoreAction::Unchanged => '=',
        RestoreAction::Skipped => '!',
    }
}

/// Verb word + color for a planned action.
fn plan_verb(a: RestoreAction, style: &Style) -> (&'static str, &str) {
    match a {
        RestoreAction::WouldOverwrite => ("would OVERWRITE", style.ylw),
        RestoreAction::WouldCreate => ("would CREATE", style.grn),
        RestoreAction::Unchanged => ("unchanged", style.dim),
        RestoreAction::Skipped => ("SKIPPED", style.ylw),
    }
}

/// Trailing detail for a plan row: size transition, or the skip reason.
fn plan_detail(item: &RestoreItem, style: &Style) -> String {
    match item.action {
        RestoreAction::WouldOverwrite => size_transition(item.was_bytes, item.now_bytes),
        RestoreAction::WouldCreate => match item.now_bytes {
            Some(n) => format!("  ({})", human_size(n)),
            None => String::new(),
        },
        RestoreAction::Unchanged => String::new(),
        RestoreAction::Skipped => match item.reason {
            Some(r) => format!("  {}({}){}", style.dim, r.word(), style.rst),
            None => String::new(),
        },
    }
}

/// `live X → captured Y` size transition for an overwrite. Empty when neither
/// side has a size (a dir).
fn size_transition(was: Option<u64>, now: Option<u64>) -> String {
    match (was, now) {
        (Some(a), Some(b)) => format!("  live {} → captured {}", human_size(a), human_size(b)),
        _ => String::new(),
    }
}

/// Zero-suppressed plan footer.
fn plan_footer(p: &RestorePlan) -> String {
    let mut parts = Vec::new();
    if p.would_change > 0 {
        parts.push(format!("{} would change", p.would_change));
    }
    if p.unchanged > 0 {
        parts.push(format!("{} unchanged", p.unchanged));
    }
    if p.skipped > 0 {
        parts.push(format!("{} skipped", p.skipped));
    }
    if p.schema_downgrades > 0 {
        parts.push(format!("{} schema downgrade(s)", p.schema_downgrades));
    }
    if parts.is_empty() {
        "nothing to restore".to_string()
    } else {
        parts.join(" · ")
    }
}

/// Marker glyph for an apply outcome.
fn outcome_marker(k: RestoreOutcomeKind) -> char {
    match k {
        RestoreOutcomeKind::Restored => '~',
        RestoreOutcomeKind::Created => '+',
        RestoreOutcomeKind::Unchanged => '=',
        RestoreOutcomeKind::Skipped => '!',
        RestoreOutcomeKind::Failed => '!',
    }
}

/// Verb word + color for an apply outcome.
fn outcome_verb(k: RestoreOutcomeKind, style: &Style) -> (&'static str, &str) {
    match k {
        RestoreOutcomeKind::Restored => ("RESTORED", style.grn),
        RestoreOutcomeKind::Created => ("CREATED", style.grn),
        RestoreOutcomeKind::Unchanged => ("unchanged", style.dim),
        RestoreOutcomeKind::Skipped => ("SKIPPED", style.ylw),
        RestoreOutcomeKind::Failed => ("FAILED", style.red),
    }
}

/// Trailing detail for an outcome row: size transition, failure reason, skip
/// reason, plus an owner-not-set note.
fn outcome_detail(r: &RestoreResult, style: &Style) -> String {
    let mut s = match r.outcome {
        RestoreOutcomeKind::Restored => size_transition(r.was_bytes, r.now_bytes),
        RestoreOutcomeKind::Created => match r.now_bytes {
            Some(n) => format!("  ({})", human_size(n)),
            None => String::new(),
        },
        RestoreOutcomeKind::Failed | RestoreOutcomeKind::Skipped => match &r.reason {
            Some(reason) => format!("  ({reason})"),
            None => String::new(),
        },
        RestoreOutcomeKind::Unchanged => String::new(),
    };
    if r.owner_unset {
        s.push_str(&format!("  {}(owner not set){}", style.dim, style.rst));
    }
    s
}

/// Apply footer: zero-suppressed, but `failed` is ALWAYS shown (it's a write
/// path — honesty about failures, R6), and the safety id trails when present.
fn outcome_footer(o: &RestoreOutcome, style: &Style) -> String {
    let mut parts = Vec::new();
    if o.restored > 0 {
        parts.push(format!("{} restored", o.restored));
    }
    parts.push(format!("{} failed", o.failed));
    if o.unchanged > 0 {
        parts.push(format!("{} unchanged", o.unchanged));
    }
    if o.skipped > 0 {
        parts.push(format!("{} skipped", o.skipped));
    }
    let mut line = parts.join(" · ");
    if let Some(id) = &o.safety_capture {
        line.push_str(&format!(
            " · {}safety capture {}{}",
            style.dim,
            short_id(id),
            style.rst
        ));
    }
    line
}

// ---- Prune view (`rewind prune`) ------------------------------------------

/// Print what a prune removed: the removed captures, then a zero-suppressed
/// footer (object/byte reclaim shown only when `--gc` ran).
pub fn print_prune(o: &PruneOutcome, style: &Style) {
    println!("{}{}rewind prune{}", style.bold, style.cyn, style.rst);
    println!();

    if o.removed.is_empty() && o.objects_removed == 0 {
        println!(
            "{}no captures matched — nothing removed.{}",
            style.dim, style.rst
        );
        return;
    }

    for c in &o.removed {
        println!(
            "  {}- {}  {}{}",
            style.dim,
            short_id(&c.id),
            short_when(&c.captured_at),
            style.rst
        );
    }

    println!();
    let mut parts = vec![format!(
        "{} {} removed",
        o.removed_count,
        plural(o.removed_count, "capture", "captures")
    )];
    if o.gc {
        parts.push(format!(
            "{} {} reclaimed",
            o.objects_removed,
            plural(o.objects_removed, "object", "objects")
        ));
        parts.push(format!("{} freed", human_size(o.bytes_reclaimed)));
    }
    parts.push(format!("{} remaining", o.remaining_captures));
    println!("{}{}{}", style.dim, parts.join(" · "), style.rst);
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

/// One restore-plan path in the JSON envelope. `outcome` is the dry-run verb
/// (`would_overwrite`/`would_create`/`unchanged`/`skipped`).
#[derive(Serialize)]
struct PlanResultOut<'a> {
    path: &'a str,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now_bytes: Option<u64>,
    #[serde(skip_serializing_if = "is_false")]
    schema_downgrade: bool,
}

#[derive(Serialize)]
struct RestorePlanEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    action: &'static str,
    from: &'a str,
    applied: bool,
    dry_run: bool,
    would_change: usize,
    unchanged: usize,
    skipped: usize,
    schema_downgrades: usize,
    results: Vec<PlanResultOut<'a>>,
}

/// The `rewind restore` dry-run JSON envelope.
pub fn restore_plan_json(p: &RestorePlan) -> String {
    let results = p
        .items
        .iter()
        .map(|i| PlanResultOut {
            path: &i.path,
            outcome: plan_action_tag(i.action),
            reason: i.reason.map(skip_reason_tag),
            was_bytes: i.was_bytes,
            now_bytes: i.now_bytes,
            schema_downgrade: i.schema_downgrade,
        })
        .collect();
    let env = RestorePlanEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        action: "restore",
        from: &p.from,
        applied: false,
        dry_run: true,
        would_change: p.would_change,
        unchanged: p.unchanged,
        skipped: p.skipped,
        schema_downgrades: p.schema_downgrades,
        results,
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// One restore-apply path in the JSON envelope.
#[derive(Serialize)]
struct OutcomeResultOut<'a> {
    path: &'a str,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now_bytes: Option<u64>,
    #[serde(skip_serializing_if = "is_false")]
    owner_unset: bool,
}

#[derive(Serialize)]
struct RestoreOutcomeEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    action: &'static str,
    from: &'a str,
    applied: bool,
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_capture: &'a Option<String>,
    restored: usize,
    failed: usize,
    unchanged: usize,
    skipped: usize,
    results: Vec<OutcomeResultOut<'a>>,
}

/// The `rewind restore --apply` JSON envelope.
pub fn restore_outcome_json(o: &RestoreOutcome) -> String {
    let results = o
        .results
        .iter()
        .map(|r| OutcomeResultOut {
            path: &r.path,
            outcome: outcome_tag(r.outcome),
            reason: &r.reason,
            was_bytes: r.was_bytes,
            now_bytes: r.now_bytes,
            owner_unset: r.owner_unset,
        })
        .collect();
    let env = RestoreOutcomeEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        action: "restore",
        from: &o.from,
        applied: true,
        dry_run: false,
        safety_capture: &o.safety_capture,
        restored: o.restored,
        failed: o.failed,
        unchanged: o.unchanged,
        skipped: o.skipped,
        results,
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// The `rewind prune` JSON envelope. `PruneOutcome` already serializes cleanly;
/// wrap it with the suite header fields.
#[derive(Serialize)]
struct PruneEnvelope<'a> {
    schema_version: u32,
    source_tool: &'static str,
    action: &'static str,
    removed: &'a [crate::prune::PrunedCapture],
    removed_count: usize,
    gc: bool,
    objects_removed: usize,
    bytes_reclaimed: u64,
    remaining_captures: usize,
}

/// Render a prune outcome as the suite JSON envelope.
pub fn prune_json(o: &PruneOutcome) -> String {
    let env = PruneEnvelope {
        schema_version: 1,
        source_tool: "rewind",
        action: "prune",
        removed: &o.removed,
        removed_count: o.removed_count,
        gc: o.gc,
        objects_removed: o.objects_removed,
        bytes_reclaimed: o.bytes_reclaimed,
        remaining_captures: o.remaining_captures,
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

/// JSON tag for a plan action (dry-run verbs).
fn plan_action_tag(a: RestoreAction) -> &'static str {
    match a {
        RestoreAction::WouldOverwrite => "would_overwrite",
        RestoreAction::WouldCreate => "would_create",
        RestoreAction::Unchanged => "unchanged",
        RestoreAction::Skipped => "skipped",
    }
}

/// JSON tag for an apply outcome.
fn outcome_tag(k: RestoreOutcomeKind) -> &'static str {
    match k {
        RestoreOutcomeKind::Restored => "restored",
        RestoreOutcomeKind::Created => "created",
        RestoreOutcomeKind::Unchanged => "unchanged",
        RestoreOutcomeKind::Skipped => "skipped",
        RestoreOutcomeKind::Failed => "failed",
    }
}

/// JSON tag for a skip reason.
fn skip_reason_tag(r: crate::restore::SkipReason) -> &'static str {
    match r {
        crate::restore::SkipReason::UnreadableInCapture => "unreadable_in_capture",
        crate::restore::SkipReason::MissingObject => "missing_object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CaptureEntry, EntryKind, MANIFEST_SCHEMA_VERSION};
    use crate::restore::SkipReason;

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

    // ---- Phase 3: restore / prune renderers -------------------------------

    fn plan_with(items: Vec<RestoreItem>) -> RestorePlan {
        let would_change = items
            .iter()
            .filter(|i| {
                matches!(
                    i.action,
                    RestoreAction::WouldOverwrite | RestoreAction::WouldCreate
                )
            })
            .count();
        let unchanged = items
            .iter()
            .filter(|i| i.action == RestoreAction::Unchanged)
            .count();
        let skipped = items
            .iter()
            .filter(|i| i.action == RestoreAction::Skipped)
            .count();
        let schema_downgrades = items.iter().filter(|i| i.schema_downgrade).count();
        RestorePlan {
            from: "a17b2222".into(),
            captured_at: "2026-06-19T14:22:05Z".into(),
            items,
            would_change,
            unchanged,
            skipped,
            schema_downgrades,
        }
    }

    fn plan_item(path: &str, action: RestoreAction, reason: Option<SkipReason>) -> RestoreItem {
        RestoreItem {
            path: path.into(),
            action,
            reason,
            was_bytes: Some(100),
            now_bytes: Some(200),
            schema_downgrade: false,
        }
    }

    #[test]
    fn restore_plan_json_is_a_dry_run_envelope_with_action_tags() {
        let p = plan_with(vec![
            plan_item("/etc/a", RestoreAction::WouldOverwrite, None),
            plan_item("/etc/b", RestoreAction::WouldCreate, None),
            plan_item("/etc/c", RestoreAction::Unchanged, None),
            plan_item(
                "/etc/d",
                RestoreAction::Skipped,
                Some(SkipReason::MissingObject),
            ),
        ]);
        let v: serde_json::Value = serde_json::from_str(&restore_plan_json(&p)).unwrap();
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["action"], "restore");
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["applied"], false);
        assert_eq!(v["would_change"], 2);
        assert_eq!(v["unchanged"], 1);
        assert_eq!(v["skipped"], 1);
        assert_eq!(v["results"][0]["outcome"], "would_overwrite");
        assert_eq!(v["results"][1]["outcome"], "would_create");
        assert_eq!(v["results"][2]["outcome"], "unchanged");
        assert_eq!(v["results"][3]["outcome"], "skipped");
        assert_eq!(v["results"][3]["reason"], "missing_object");
    }

    #[test]
    fn restore_plan_json_omits_false_schema_downgrade() {
        // "absence is the signal": a non-downgrade item carries no key.
        let p = plan_with(vec![plan_item(
            "/etc/a",
            RestoreAction::WouldOverwrite,
            None,
        )]);
        let v: serde_json::Value = serde_json::from_str(&restore_plan_json(&p)).unwrap();
        assert!(v["results"][0].get("schema_downgrade").is_none());
        assert!(v["results"][0].get("reason").is_none());
    }

    #[test]
    fn restore_outcome_json_carries_safety_capture_failed_and_owner_unset() {
        let o = RestoreOutcome {
            from: "a17b2222".into(),
            applied: true,
            safety_capture: Some("ffff0000".into()),
            results: vec![
                RestoreResult {
                    path: "/etc/a".into(),
                    outcome: RestoreOutcomeKind::Restored,
                    reason: None,
                    was_bytes: Some(10),
                    now_bytes: Some(20),
                    owner_unset: true,
                },
                RestoreResult {
                    path: "/etc/b".into(),
                    outcome: RestoreOutcomeKind::Failed,
                    reason: Some("permission denied".into()),
                    was_bytes: None,
                    now_bytes: None,
                    owner_unset: false,
                },
            ],
            restored: 1,
            failed: 1,
            unchanged: 0,
            skipped: 0,
        };
        let v: serde_json::Value = serde_json::from_str(&restore_outcome_json(&o)).unwrap();
        assert_eq!(v["action"], "restore");
        assert_eq!(v["dry_run"], false);
        assert_eq!(v["applied"], true);
        assert_eq!(v["safety_capture"], "ffff0000");
        assert_eq!(v["restored"], 1);
        assert_eq!(v["failed"], 1);
        assert_eq!(v["results"][0]["outcome"], "restored");
        assert_eq!(v["results"][0]["owner_unset"], true);
        assert_eq!(v["results"][1]["outcome"], "failed");
        assert_eq!(v["results"][1]["reason"], "permission denied");
        // The owner_unset false on the second result is omitted, not null.
        assert!(v["results"][1].get("owner_unset").is_none());
    }

    #[test]
    fn restore_outcome_json_omits_absent_safety_capture() {
        let o = RestoreOutcome {
            from: "a17b2222".into(),
            applied: true,
            safety_capture: None,
            results: vec![],
            restored: 0,
            failed: 0,
            unchanged: 1,
            skipped: 0,
        };
        let v: serde_json::Value = serde_json::from_str(&restore_outcome_json(&o)).unwrap();
        assert!(
            v.get("safety_capture").is_none(),
            "a skipped safety capture is omitted, not null"
        );
    }

    #[test]
    fn prune_json_is_a_prune_envelope() {
        let o = PruneOutcome {
            removed: vec![crate::prune::PrunedCapture {
                id: "old11111".into(),
                captured_at: "2026-06-01T00:00:00Z".into(),
            }],
            removed_count: 1,
            gc: true,
            objects_removed: 2,
            bytes_reclaimed: 4096,
            remaining_captures: 3,
        };
        let v: serde_json::Value = serde_json::from_str(&prune_json(&o)).unwrap();
        assert_eq!(v["source_tool"], "rewind");
        assert_eq!(v["action"], "prune");
        assert_eq!(v["removed_count"], 1);
        assert_eq!(v["gc"], true);
        assert_eq!(v["objects_removed"], 2);
        assert_eq!(v["bytes_reclaimed"], 4096);
        assert_eq!(v["remaining_captures"], 3);
        assert_eq!(v["removed"][0]["id"], "old11111");
    }

    #[test]
    fn human_renderers_do_not_panic_monochrome() {
        // Smoke: the human printers run clean over representative inputs. Color
        // off so the assertions below can match the plain text.
        let style = Style::resolve(true);
        let p = plan_with(vec![
            plan_item("/etc/a", RestoreAction::WouldOverwrite, None),
            plan_item(
                "/etc/b",
                RestoreAction::Skipped,
                Some(SkipReason::UnreadableInCapture),
            ),
        ]);
        print_restore_plan(&p, &style);

        let o = RestoreOutcome {
            from: "a17b2222".into(),
            applied: true,
            safety_capture: Some("ffff0000".into()),
            results: vec![RestoreResult {
                path: "/etc/a".into(),
                outcome: RestoreOutcomeKind::Restored,
                reason: None,
                was_bytes: Some(10),
                now_bytes: Some(20),
                owner_unset: false,
            }],
            restored: 1,
            failed: 0,
            unchanged: 0,
            skipped: 0,
        };
        print_restore_outcome(&o, &style);

        let pr = PruneOutcome {
            removed: vec![],
            removed_count: 0,
            gc: false,
            objects_removed: 0,
            bytes_reclaimed: 0,
            remaining_captures: 2,
        };
        print_prune(&pr, &style);
    }
}
