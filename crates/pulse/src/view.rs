//! The ratatui draw layer: a pure function of `(App, Theme, frame.area())` that
//! paints the current view using suite-ui chrome. No state, no I/O — the event
//! loop in [`crate::app`] owns those; this file only describes pixels.
//!
//! MIGRATION STATE: [`View::Default`] (the calm verdict screen) is drawn here in
//! real ratatui through the [`Theme`]. The other views still fall back to the
//! legacy string renderer blitted as one `Paragraph` (see [`draw_legacy`]) until
//! T6–T8 port them; the transient status line is overlaid on the bottom row the
//! same way the string renderer did, regardless of which path drew the view.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{truncate_desc, KeyHints, Theme};

use crate::app::{App, View};
use crate::verdict::{State, Verdict};
use crate::{count_summary, incomplete_summary, verdict_text, TermSize, MIN_CENTER_HEIGHT};

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
    match app.view() {
        View::Default => draw_verdict(f, area, app.verdict(), app.theme()),
        // Not yet ported — fall back to the legacy string frame (monochrome).
        _ => draw_legacy(f, area, app),
    }
    // The transient status line overlays the bottom row whatever drew the view,
    // exactly as the string renderer's `overlay_status` did.
    if let Some(msg) = app.status() {
        draw_status_overlay(f, area, msg, app.theme());
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

/// Fall back to the legacy string frame for an unported view, blitted as one
/// monochrome `Paragraph`. Removed view by view in T6–T8.
fn draw_legacy(f: &mut Frame, area: Rect, app: &App) {
    let size = TermSize::from_area(area.width, area.height);
    let text = ratatui::text::Text::raw(app.legacy_frame(size));
    f.render_widget(Paragraph::new(text), area);
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
}
