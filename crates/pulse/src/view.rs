//! The ratatui draw layer: a pure function of `(App, Theme, frame.area())` that
//! paints the current view using suite-ui chrome. No state, no I/O — the event
//! loop in [`crate::app`] owns those; this file only describes pixels.
//!
//! Every interactive view is drawn here in real ratatui through the [`Theme`]:
//! the calm verdict screen ([`draw_verdict`]) and the drill-downs — Attention
//! ([`SeverityBadge`] rows / [`EmptyState`]), Feeds ([`HealthStrip`]), Details,
//! Help ([`HelpSheet`] overlay), and Search ([`SearchBar`]). The transient status
//! line is overlaid on the bottom row whichever view drew. The legacy string
//! renderer in `main.rs`/`app.rs` now serves only `--dump-view`/`--state` and the
//! navigation tests, and is removed in T9/T10.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{
    pane_titled, truncate_desc, EmptyState, Health, HealthStrip, HelpSheet, KeyHints, SearchBar,
    Severity, SeverityBadge, Theme,
};

use crate::app::{App, View};
use crate::verdict::{State, Verdict};
use crate::{count_summary, incomplete_summary, verdict_text, MIN_CENTER_HEIGHT};

/// Map Pulse's domain severity onto the suite's [`Severity`] axis, so the
/// drill-down rows can use [`SeverityBadge`] / [`Theme::severity`]. Kept here at
/// the draw boundary — the domain enum stays domain-side.
fn suite_severity(s: crate::sources::Severity) -> Severity {
    use crate::sources::Severity as PulseSeverity;
    match s {
        PulseSeverity::Critical => Severity::Critical,
        PulseSeverity::High => Severity::High,
        PulseSeverity::Medium => Severity::Medium,
        PulseSeverity::Low => Severity::Low,
    }
}

/// The footer hint pairs for the default screen — the suite-ui [`KeyHints`]
/// version of the old `hint_strip`. `q quit` is omitted to keep it narrow (quit
/// still works), matching the legacy strip.
const DEFAULT_HINTS: &[(&str, &str)] = &[
    ("enter", "details"),
    ("a", "attention"),
    ("f", "feeds"),
    ("/", "search"),
    ("r", "cockpit"),
    ("?", "help"),
];

/// Paint the current view into `f`. The single entry point the event loop calls.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let theme = app.theme();
    match app.view() {
        View::Default => draw_verdict(f, area, app.verdict(), theme),
        View::Attention => draw_attention(f, area, app, theme),
        View::Feeds => draw_feeds(f, area, app, theme),
        View::Details => draw_details(f, area, app.verdict(), theme),
        View::Help => draw_help(f, area, theme),
        View::Search => draw_search(f, area, app, theme),
    }
    // The transient status line overlays the bottom row whatever drew the view,
    // exactly as the string renderer's `overlay_status` did.
    if let Some(msg) = app.status() {
        draw_status_overlay(f, area, msg, theme);
    }
}

/// The calm default screen, in ratatui. Faithful to the string renderer's
/// geometry: a vertical anchor at ~40%, the verdict centered there, bottom-pinned
/// furniture on the busy states (sources · rule · hints · timestamp), and a
/// dim timestamp bottom-right always. Degrades to a top-left compact layout when
/// the terminal is too small to center.
fn draw_verdict(f: &mut Frame, area: Rect, v: &Verdict, theme: Theme) {
    let w = area.width;
    let h = area.height;
    if w < crate::MIN_CENTER_WIDTH || h < MIN_CENTER_HEIGHT {
        draw_verdict_compact(f, area, v, theme);
        return;
    }

    // Wordmark: dim, top-left, busy states only — the healthy screen stays bare.
    if v.state != State::Healthy {
        put_line(f, area, 0, Line::from(Span::styled(" pulse", theme.dim())));
    }

    let anchor = (h * 2) / 5;
    let busy = v.state != State::Healthy && h >= 8;
    let block_floor = if busy {
        h.saturating_sub(5)
    } else {
        h.saturating_sub(2)
    };

    // The center block: each state's ordered, centered lines from the anchor down,
    // stopping before the bottom furniture so it can never overrun it.
    let block = verdict_block(v, theme);
    for (off, line) in block.into_iter().enumerate() {
        let row = anchor + off as u16;
        if row >= block_floor {
            break;
        }
        put_centered(f, area, row, line);
    }

    if busy {
        put_centered(f, area, h - 4, source_line(v, theme));
        // A dim horizontal rule across the inner width.
        let rule = "─".repeat(w.saturating_sub(2) as usize);
        put_line(
            f,
            area,
            h - 3,
            Line::from(Span::styled(format!(" {rule}"), theme.dim())),
        );
        // The footer hint strip — suite-ui KeyHints, indented one column.
        let footer = Rect {
            x: area.x + 1,
            y: area.y + h - 2,
            width: w.saturating_sub(1),
            height: 1,
        };
        KeyHints {
            hints: DEFAULT_HINTS,
        }
        .render(f, footer, theme);
    }

    // Timestamp: always present, the dimmest mark on screen, bottom-right.
    put_right(f, area, h - 1, timestamp_text(v), theme.dim());
}

/// The compact top-left layout for a terminal too small to center: verdict,
/// optional one-line summary, timestamp. No furniture — nothing can clip.
fn draw_verdict_compact(f: &mut Frame, area: Rect, v: &Verdict, theme: Theme) {
    let mut row = 0u16;
    put_line(
        f,
        area,
        row,
        Line::from(Span::styled(
            verdict_text(v.state),
            verdict_style(v.state, theme),
        )),
    );
    row += 1;
    match v.state {
        State::Healthy => {}
        State::NeedsAttention => {
            let summary = count_summary(v);
            if !summary.is_empty() {
                put_line(f, area, row, Line::from(Span::styled(summary, theme.dim())));
                row += 1;
            }
            if v.confidence_reduced {
                put_line(
                    f,
                    area,
                    row,
                    Line::from(Span::styled(
                        "confidence reduced by stale feeds",
                        theme.working(),
                    )),
                );
                row += 1;
            }
        }
        State::Incomplete => {
            put_line(
                f,
                area,
                row,
                Line::from(Span::styled(incomplete_summary(v), theme.dim())),
            );
            row += 1;
        }
    }
    put_line(
        f,
        area,
        row,
        Line::from(Span::styled(timestamp_plain(v), theme.dim())),
    );
}

/// The ordered, centered content lines for a state's center block (verdict word
/// first), styled through the theme. Blank lines are empty `Line`s for spacing.
fn verdict_block(v: &Verdict, theme: Theme) -> Vec<Line<'static>> {
    let mut block = vec![Line::from(Span::styled(
        verdict_text(v.state),
        verdict_style(v.state, theme),
    ))];
    match v.state {
        State::Healthy => {}
        State::NeedsAttention => {
            block.push(Line::default());
            block.push(count_line(v, theme));
            if v.confidence_reduced {
                block.push(Line::default());
                block.push(Line::from(Span::styled(
                    "confidence reduced by stale feeds",
                    theme.working(),
                )));
            }
            block.push(Line::default());
            block.push(Line::default());
            for c in cause_lines(v, theme) {
                block.push(c);
            }
        }
        State::Incomplete => {
            block.push(Line::default());
            block.push(Line::from(Span::styled(
                incomplete_summary(v),
                theme.dim().add_modifier(ratatui::style::Modifier::BOLD),
            )));
            block.push(Line::default());
            block.push(Line::default());
            block.push(Line::from(Span::styled(
                "the suite view may be missing data",
                theme.dim(),
            )));
        }
    }
    block
}

/// The centered count line: critical portion in the critical severity style (the
/// design's one licensed use of red on the default screen), high portion bold.
fn count_line(v: &Verdict, theme: Theme) -> Line<'static> {
    use suite_ui::Severity;
    let bold = theme.dim().add_modifier(ratatui::style::Modifier::BOLD);
    match (v.critical, v.high) {
        (0, 0) => Line::default(),
        (c, 0) => Line::from(Span::styled(
            format!("{c} critical"),
            theme.severity(Severity::Critical),
        )),
        (0, h) => Line::from(Span::styled(format!("{h} high"), bold)),
        (c, h) => Line::from(vec![
            Span::styled(format!("{c} critical"), theme.severity(Severity::Critical)),
            Span::styled(" · ", theme.dim()),
            Span::styled(format!("{h} high"), bold),
        ]),
    }
}

/// Cause rows under the verdict: `what  why  source`, truncated to soft columns
/// (display-width-aware via suite-ui), left-indented to read as a column.
fn cause_lines(v: &Verdict, theme: Theme) -> Vec<Line<'static>> {
    v.causes
        .iter()
        .map(|c| {
            let what = truncate_desc(&c.what, 18);
            let why = truncate_desc(&c.why, 26);
            Line::from(Span::styled(
                format!("    {what:18}{why:26}{}", c.source),
                theme.dim(),
            ))
        })
        .collect()
}

/// The source-confidence line: `sources  ● workstate  ◐ vault …`. Markers carry
/// state by shape (●/◐/○); the per-source health hue is a bonus the theme gates.
fn source_line(v: &Verdict, theme: Theme) -> Line<'static> {
    use crate::verdict::Source;
    use suite_ui::Health;
    let mut spans = vec![Span::styled("sources  ", theme.dim())];
    for (n, m) in v.sources.iter().enumerate() {
        if n > 0 {
            spans.push(Span::styled("  ", theme.dim()));
        }
        let (glyph, health) = match m.freshness {
            Source::Current => ("●", Health::Healthy),
            Source::Stale => ("◐", Health::Degraded),
            Source::Missing => ("○", Health::Unknown),
        };
        spans.push(Span::styled(glyph.to_string(), theme.health(health)));
        spans.push(Span::styled(format!(" {}", m.name), theme.dim()));
    }
    Line::from(spans)
}

/// The dim timestamp text: bare relative age on healthy ("2m ago"), "updated …"
/// on the busy states so it isn't ambiguous next to other text.
fn timestamp_text(v: &Verdict) -> String {
    timestamp_plain(v)
}

fn timestamp_plain(v: &Verdict) -> String {
    match v.state {
        State::Healthy => v.age.clone(),
        _ => format!("updated {}", v.age),
    }
}

fn verdict_style(state: State, theme: Theme) -> ratatui::style::Style {
    use suite_ui::Health;
    match state {
        State::Healthy => theme.health(Health::Healthy),
        // Attention and Incomplete are both amber — a confidence/attention
        // problem, not a critical one (the working/degraded hue).
        State::NeedsAttention | State::Incomplete => theme.working(),
    }
}

// ── drill-down views ────────────────────────────────────────────────────────
// Each is a titled pane with a footer hint row inside it; the body fills the gap.
// The footer key advertises the view's own non-Esc close path (the design rule:
// every view closes without Esc), plus the shared Esc/q.

/// Draw a titled pane (`pulse · TITLE`) with the given footer `hints` pinned to
/// its bottom inner row, and return the remaining inner `Rect` for the caller to
/// fill with the view body.
fn framed_view<'a>(
    f: &mut Frame,
    area: Rect,
    title: &str,
    theme: Theme,
    hints: &'a [(&'a str, &'a str)],
) -> Rect {
    let block = pane_titled(
        Line::from(Span::styled(format!(" pulse · {title} "), theme.title())),
        theme,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return inner;
    }
    // Footer hint row pinned to the bottom of the inner area; body is the rest.
    let footer = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };
    KeyHints { hints }.render(f, footer, theme);
    Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height - 1,
    }
}

/// `a` — everything that needs action, one row per item: a [`SeverityBadge`], the
/// `what`, and a dim `— why (source)`. [`EmptyState`] when nothing needs action.
fn draw_attention(f: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let hints = &[("a", "back"), ("Esc", "back"), ("q", "quit")];
    let body = framed_view(f, area, "ATTENTION", theme, hints);
    let items = app.attention_items();
    if items.is_empty() {
        EmptyState {
            message: "Nothing needs attention.",
            hint: Some("The suite is clear."),
        }
        .render(f, body, theme);
        return;
    }
    let why_budget = (body.width as usize).saturating_sub(28);
    let lines: Vec<Line> = items
        .iter()
        .map(|a| {
            Line::from(vec![
                SeverityBadge {
                    severity: suite_severity(a.severity),
                }
                .span(theme),
                Span::styled(
                    format!("  {}", a.what),
                    theme.dim().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  — {} ({})", truncate_desc(&a.why, why_budget), a.source),
                    theme.dim(),
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), body);
}

/// `f` — source freshness as a [`HealthStrip`] (●/◐/○ per source) plus a dim
/// provenance line for when the snapshot was built.
fn draw_feeds(f: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let hints = &[("f", "back"), ("Esc", "back"), ("q", "quit")];
    let body = framed_view(f, area, "FEEDS", theme, hints);
    let marks = app.source_marks();
    // HealthStrip borrows (&str, Health); build owned strings then borrow them.
    let segs: Vec<(Health, String)> = marks
        .iter()
        .map(|m| (source_health(m.freshness), m.name.clone()))
        .collect();
    let seg_refs: Vec<(Health, &str)> = segs.iter().map(|(h, n)| (*h, n.as_str())).collect();
    let provenance = match app.snapshot_built_at() {
        Some(b) => format!("snapshot built {b}"),
        None => "no snapshot found".to_string(),
    };
    let mut lines = vec![HealthStrip {
        segments: &seg_refs,
    }
    .line(theme)];
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(provenance, theme.dim())));
    f.render_widget(Paragraph::new(lines), body);
}

/// `Enter` — the verdict's particulars: state word, data age, and finding counts.
fn draw_details(f: &mut Frame, area: Rect, v: &Verdict, theme: Theme) {
    let hints = &[("Enter", "back"), ("Esc", "back"), ("q", "quit")];
    let body = framed_view(f, area, "DETAILS", theme, hints);
    let mut lines = vec![
        kv_line("verdict", &verdict_text(v.state), theme),
        kv_line("data age", &v.age, theme),
    ];
    if v.critical + v.high > 0 {
        lines.push(kv_line(
            "findings",
            &format!("{} critical, {} high", v.critical, v.high),
            theme,
        ));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "press a for the full attention list, f for feeds.",
        theme.dim(),
    )));
    f.render_widget(Paragraph::new(lines), body);
}

/// `?` — the keybinding reference, as the suite-ui [`HelpSheet`] overlay (a
/// centered modal over whatever was on screen).
fn draw_help(f: &mut Frame, area: Rect, theme: Theme) {
    let rows = &[
        ("Enter", "details for the current verdict"),
        ("a", "attention — everything that needs action"),
        ("f", "feeds — source freshness & confidence"),
        ("/", "search across visible status"),
        ("r", "open the full RexOps cockpit"),
        ("?", "toggle this help"),
        ("q", "quit"),
        ("Esc", "back to the verdict"),
    ];
    HelpSheet {
        title: "pulse · keys",
        rows,
    }
    .render(f, area, theme);
}

/// `/` — the live filter: a [`SearchBar`] over the matches, with [`EmptyState`]
/// for the empty-query prompt and the no-matches case.
fn draw_search(f: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let hints = &[("Enter", "close"), ("Esc", "close"), ("q", "quit")];
    let body = framed_view(f, area, "SEARCH", theme, hints);
    if body.height == 0 {
        return;
    }
    let q = app.query();
    let hits = if q.is_empty() {
        Vec::new()
    } else {
        app.search_hit_lines(q)
    };
    // The search bar occupies the first body row; the results fill the rest.
    let bar_row = Rect {
        x: body.x,
        y: body.y,
        width: body.width,
        height: 1,
    };
    SearchBar {
        query: q,
        placeholder: "type to filter; Enter or Esc to close",
        match_count: if q.is_empty() { None } else { Some(hits.len()) },
    }
    .render(f, bar_row, theme);

    let results = Rect {
        x: body.x,
        y: body.y + 1,
        width: body.width,
        height: body.height.saturating_sub(1),
    };
    if q.is_empty() {
        return;
    }
    if hits.is_empty() {
        EmptyState {
            message: "No matches.",
            hint: None,
        }
        .render(f, results, theme);
        return;
    }
    let lines: Vec<Line> = hits
        .iter()
        .map(|h| {
            Line::from(Span::styled(
                truncate_desc(h, results.width as usize),
                theme.dim(),
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), results);
}

/// A `key   value` detail row: dim key in a fixed column, plain value.
fn kv_line(key: &str, value: &str, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10}"), theme.dim()),
        Span::styled(value.to_string(), theme.dim().add_modifier(Modifier::BOLD)),
    ])
}

/// Pulse's source freshness mapped onto the suite [`Health`] axis for the strip.
fn source_health(s: crate::verdict::Source) -> Health {
    use crate::verdict::Source;
    match s {
        Source::Current => Health::Healthy,
        Source::Stale => Health::Degraded,
        Source::Missing => Health::Unknown,
    }
}

/// Overlay the transient status line on the bottom row (dim, one column in),
/// matching the string renderer's `overlay_status`.
fn draw_status_overlay(f: &mut Frame, area: Rect, msg: &str, theme: Theme) {
    if area.height == 0 {
        return;
    }
    let row = area.height - 1;
    let text = truncate_desc(msg, area.width.saturating_sub(1) as usize);
    put_line(
        f,
        area,
        row,
        Line::from(Span::styled(format!(" {text}"), theme.dim())),
    );
}

// ── row-placement helpers ───────────────────────────────────────────────────
// ratatui draws into Rects; these pin a single Line at a row offset within the
// frame area, the same fixed-row geometry the string renderer used.

/// Render `line` left-anchored at row `row` (0-based within `area`).
fn put_line(f: &mut Frame, area: Rect, row: u16, line: Line<'static>) {
    if row >= area.height {
        return;
    }
    let rect = Rect {
        x: area.x,
        y: area.y + row,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(line), rect);
}

/// Render `line` horizontally centered at row `row`, using the line's display
/// width for the left pad (so centering is honest about wide glyphs).
fn put_centered(f: &mut Frame, area: Rect, row: u16, line: Line<'static>) {
    if row >= area.height {
        return;
    }
    let line_w = line.width() as u16;
    let pad = area.width.saturating_sub(line_w) / 2;
    let rect = Rect {
        x: area.x + pad,
        y: area.y + row,
        width: area.width.saturating_sub(pad),
        height: 1,
    };
    f.render_widget(Paragraph::new(line), rect);
}

/// Render `text` right-aligned at row `row` with a one-column right margin.
fn put_right(f: &mut Frame, area: Rect, row: u16, text: String, style: ratatui::style::Style) {
    if row >= area.height {
        return;
    }
    let rect = Rect {
        x: area.x,
        y: area.y + row,
        width: area.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style)))
            .right_aligned()
            .block(ratatui::widgets::Block::default().padding(ratatui::widgets::Padding::right(1))),
        rect,
    );
}

// ============================================================================
// Tests — geometry snapshots + width/size safety for the default verdict screen.
// Snapshots capture glyphs/layout only (insta's buffer Display ignores colour),
// so they are rendered with colour OFF; the NO_COLOR/accent guarantees stay
// covered by suite-ui's and the Theme's own style assertions.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::Verdict;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    /// Render `draw_verdict` for a demo state into a `w`×`h` TestBackend buffer.
    fn render(state: &str, w: u16, h: u16) -> Buffer {
        let v = Verdict::demo(state).expect("known demo state");
        let theme = Theme::with_color(false);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_verdict(f, f.area(), &v, theme)).unwrap();
        term.backend().buffer().clone()
    }

    /// Concatenate a buffer row to a string, for content/width assertions.
    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    /// The whole buffer as a newline-joined glyph grid (ratatui 0.29's `Buffer`
    /// has no `Display`), for insta snapshots — layout/glyphs only, no colour.
    fn grid(state: &str, w: u16, h: u16) -> String {
        let buf = render(state, w, h);
        (0..h).map(|y| row(&buf, y)).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn snapshot_healthy_is_calm_and_centered() {
        insta::assert_snapshot!(grid("healthy", 80, 24));
    }

    #[test]
    fn snapshot_attention_has_block_and_footer() {
        insta::assert_snapshot!(grid("attention", 80, 24));
    }

    #[test]
    fn snapshot_incomplete() {
        insta::assert_snapshot!(grid("incomplete", 80, 24));
    }

    #[test]
    fn healthy_screen_shows_only_verdict_and_timestamp() {
        // The design's calmest screen: the lowercase verdict, and a bottom-right
        // age — no wordmark, no footer hints.
        let buf = render("healthy", 80, 24);
        let all: String = (0..buf.area.height).map(|y| row(&buf, y)).collect();
        assert!(all.contains("all clear"), "verdict present");
        assert!(all.contains("2m ago"), "timestamp present");
        assert!(!all.contains("pulse"), "no wordmark on the healthy screen");
        assert!(!all.contains("attention"), "no footer hints on healthy");
    }

    #[test]
    fn busy_screen_has_wordmark_and_footer_hints() {
        let buf = render("attention", 80, 24);
        assert!(row(&buf, 0).contains("pulse"), "dim wordmark on row 0");
        let all: String = (0..buf.area.height).map(|y| row(&buf, y)).collect();
        assert!(all.contains("NEEDS ATTENTION"), "verdict word");
        assert!(all.contains("details"), "footer KeyHints present");
        assert!(all.contains("sources"), "source-confidence line present");
    }

    #[test]
    fn no_row_exceeds_the_viewport_width() {
        // The R1 payoff at the layout level: every drawn row fits the width, at
        // normal and tiny sizes, for every state.
        for state in ["healthy", "attention", "incomplete"] {
            for (w, h) in [(80u16, 24u16), (20, 6), (40, 10)] {
                let buf = render(state, w, h);
                for y in 0..h {
                    let cols = row(&buf, y).chars().count();
                    assert!(
                        cols <= w as usize,
                        "{state} row {y} at {w}x{h} overflowed to {cols} cols"
                    );
                }
            }
        }
    }

    #[test]
    fn tiny_sizes_do_not_panic() {
        for state in ["healthy", "attention", "incomplete"] {
            for (w, h) in [(1u16, 1u16), (10, 3), (24, 8)] {
                let _ = render(state, w, h);
            }
        }
    }

    // ── drill-down views (full view::draw over sample readings) ──────────────

    /// Render a full `draw(app)` for a sample app in `view` (with `query`) into a
    /// `w`×`h` TestBackend buffer.
    fn render_view(view: View, query: &str, w: u16, h: u16) -> Buffer {
        let app = App::sample_with(view, query);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| super::draw(f, &app)).unwrap();
        term.backend().buffer().clone()
    }

    /// The view buffer flattened to a glyph grid, for snapshots.
    fn view_grid(view: View, query: &str, w: u16, h: u16) -> String {
        let buf = render_view(view, query, w, h);
        (0..h).map(|y| row(&buf, y)).collect::<Vec<_>>().join("\n")
    }

    /// The whole view buffer as one string, for content assertions.
    fn view_text(view: View, query: &str, w: u16, h: u16) -> String {
        let buf = render_view(view, query, w, h);
        (0..h).map(|y| row(&buf, y)).collect()
    }

    #[test]
    fn snapshot_attention_view() {
        insta::assert_snapshot!(view_grid(View::Attention, "", 80, 18));
    }

    #[test]
    fn snapshot_feeds_view() {
        insta::assert_snapshot!(view_grid(View::Feeds, "", 80, 18));
    }

    #[test]
    fn snapshot_details_view() {
        insta::assert_snapshot!(view_grid(View::Details, "", 80, 18));
    }

    #[test]
    fn snapshot_help_view() {
        insta::assert_snapshot!(view_grid(View::Help, "", 80, 24));
    }

    #[test]
    fn snapshot_search_view_with_a_match() {
        insta::assert_snapshot!(view_grid(View::Search, "aws", 80, 18));
    }

    #[test]
    fn attention_view_badges_the_finding_and_frames_a_pane() {
        let t = view_text(View::Attention, "", 80, 18);
        assert!(t.contains("pulse · ATTENTION"), "titled pane");
        assert!(
            t.contains("[CRIT]"),
            "SeverityBadge for the critical finding"
        );
        assert!(t.contains("deploy-prod.sh"), "the finding's subject");
        assert!(t.contains("Esc back"), "the footer back hint");
    }

    #[test]
    fn feeds_view_shows_a_health_strip_and_provenance() {
        let t = view_text(View::Feeds, "", 80, 18);
        assert!(t.contains("pulse · FEEDS"));
        assert!(t.contains("● workstate"), "HealthStrip healthy segment");
        assert!(t.contains("snapshot built"), "provenance line");
    }

    #[test]
    fn details_view_lists_the_verdict_particulars() {
        let t = view_text(View::Details, "", 80, 18);
        assert!(t.contains("pulse · DETAILS"));
        assert!(t.contains("verdict"));
        assert!(t.contains("data age"));
    }

    #[test]
    fn help_view_is_an_overlay_listing_the_keys() {
        let t = view_text(View::Help, "", 80, 24);
        assert!(t.contains("pulse · keys"), "HelpSheet title");
        assert!(t.contains("attention"), "a key is described");
        assert!(t.contains("cockpit"), "r key is described");
    }

    #[test]
    fn search_view_shows_the_bar_and_a_hit() {
        let t = view_text(View::Search, "aws", 80, 18);
        assert!(t.contains("pulse · SEARCH"));
        assert!(t.contains("/ aws"), "SearchBar with the query");
        assert!(t.contains("deploy-prod.sh"), "the matching item is listed");
    }

    #[test]
    fn search_view_empty_query_prompts_without_a_count() {
        let t = view_text(View::Search, "", 80, 18);
        assert!(t.contains("type to filter"), "placeholder shown");
        assert!(!t.contains("match"), "no count for an empty query");
    }

    #[test]
    fn search_with_no_match_shows_the_empty_state() {
        // A query nothing matches exercises the EmptyState path in the Search view
        // (the sample readings have findings, so this is the cleanest empty case).
        let t = view_text(View::Search, "zzzznomatch", 80, 18);
        assert!(t.contains("No matches."), "EmptyState for no search hits");
    }

    #[test]
    fn drill_down_rows_never_exceed_the_viewport_width() {
        for (view, q) in [
            (View::Attention, ""),
            (View::Feeds, ""),
            (View::Details, ""),
            (View::Search, "aws"),
        ] {
            for (w, h) in [(80u16, 18u16), (30, 8)] {
                let buf = render_view(view, q, w, h);
                for y in 0..h {
                    let cols = row(&buf, y).chars().count();
                    assert!(cols <= w as usize, "{view:?} row {y} at {w}x{h}: {cols}");
                }
            }
        }
    }

    #[test]
    fn drill_down_tiny_sizes_do_not_panic() {
        for view in [
            View::Attention,
            View::Feeds,
            View::Details,
            View::Help,
            View::Search,
        ] {
            for (w, h) in [(1u16, 1u16), (10, 3), (24, 6)] {
                let _ = render_view(view, "", w, h);
            }
        }
    }
}
