//! pulse — a calm, read-only status instrument for the Linux Ops Suite.
//!
//! Pulse answers one question first: *is the suite healthy right now?* It opens
//! to a single verdict centered on a near-empty screen, not a dashboard. The
//! full design lives in `PULSE_DESIGN.md` at the repo root; this file implements
//! its **default screen** — the three verdict states, with the deliberately
//! minimal healthy layout:
//!
//! ```text
//!
//!
//!                              all clear
//!
//!                                                          2m ago
//! ```
//!
//! Design rules this renderer enforces (see PULSE_DESIGN.md "Default Screen"):
//!   - Healthy is the emptiest screen in the suite: just the lowercase verdict
//!     `all clear`, anchored slightly above center, and one dim `2m ago` time in
//!     the lower-right corner. No wordmark, no supporting line, no source
//!     markers, no cause rows, no rule, no hint strip.
//!   - Non-healthy states fill from the center outward: an ALL-CAPS verdict, a
//!     one-line count/summary, an optional confidence line, up to two/three
//!     cause rows, a source-confidence line, and a bottom hint strip. The
//!     wordmark and the `updated` timestamp label return on these states.
//!   - Elements appear and vanish between states but never change position; the
//!     vertical anchor is constant. That stability is the premium feel.
//!
//! Like rex-check, pulse is intentionally dependency-free: it renders its own
//! ANSI and reads terminal size / TTY state via tiny `libc` calls behind a
//! hand-rolled `extern "C"` block, so std is all it needs. Color follows the
//! suite rule — on only when stdout is a TTY and `NO_COLOR` is unset — and the
//! screen stays fully legible with color off, because state is always also
//! carried by the verdict word and by marker shape, never by color alone.
//!
//! This pass renders from a demo verdict so all three layouts can be seen and
//! tested; wiring real producer contracts (Workstate snapshot, Bulwark / Proto
//! / ToolFoundry feeds) into the verdict model is a later step.
//!
//! Usage:
//!   pulse                 render the default (demo: healthy) screen once
//!   pulse --state STATE   force a state: healthy | attention | incomplete
//!   pulse --no-clear      don't clear the screen first (useful for piping)
//!   pulse -h | --help     this help
//!
//! Environment:
//!   NO_COLOR   disable ANSI color (also auto-disabled when stdout isn't a TTY).
//!   COLUMNS / LINES   honored as a fallback terminal size when the ioctl can't
//!                     read one (e.g. when stdout is a pipe).

use std::env;
use std::process::ExitCode;

/// Below this width or height we stop trying to center and fall back to a plain
/// top-left render, so a tiny / odd terminal never clips the verdict. 80x24 is
/// the compact target the suite's prior TUI work calls out.
const MIN_CENTER_WIDTH: u16 = 24;
const MIN_CENTER_HEIGHT: u16 = 8;

/// Default assumed size when no TTY and no COLUMNS/LINES — a classic terminal.
const FALLBACK_WIDTH: u16 = 80;
const FALLBACK_HEIGHT: u16 = 24;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut clear = true;
    // None => build the verdict from live suite data (the default). Some(name)
    // => force a demo state so the three layouts can be shown without feeds.
    let mut demo: Option<String> = None;
    // Some((view, query)) => render one interactive view once and exit. A
    // deterministic preview/snapshot path (no event loop, no PTY).
    let mut dump_view: Option<(String, String)> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--no-clear" => clear = false,
            "--state" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --state needs a value (healthy | attention | incomplete)");
                    return ExitCode::from(2);
                };
                if Verdict::demo(name).is_none() {
                    eprintln!(
                        "pulse: unknown state '{name}' (expected: healthy | attention | incomplete)"
                    );
                    return ExitCode::from(2);
                }
                demo = Some(name.clone());
                i += 1;
            }
            "--data-dir" => {
                let Some(path) = args.get(i + 1) else {
                    eprintln!("pulse: --data-dir needs a path");
                    return ExitCode::from(2);
                };
                // Equivalent to exporting PULSE_DATA_DIR; sources::DataDir reads it.
                std::env::set_var("PULSE_DATA_DIR", path);
                i += 1;
            }
            "--dump-view" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --dump-view needs a view (default|attention|feeds|details|help|search)");
                    return ExitCode::from(2);
                };
                // Optional trailing query for the search view: --dump-view search aws
                let query = args.get(i + 2).cloned().unwrap_or_default();
                let consumed = if query.is_empty() { 1 } else { 2 };
                dump_view = Some((name.clone(), query));
                i += consumed;
            }
            other => {
                eprintln!("pulse: unexpected argument '{other}' (try --help)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let style = Style::resolve();

    // Deterministic single-view render (preview / snapshot), no event loop.
    if let Some((view, query)) = dump_view {
        let readings = verdict::Readings::load(&sources::DataDir::resolve());
        let mut app = app::App::new(readings);
        return match app.dump(&view, &query, &style, TermSize::resolve()) {
            Some(frame) => {
                println!("{frame}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!(
                    "pulse: unknown view '{view}' (default|attention|feeds|details|help|search)"
                );
                ExitCode::from(2)
            }
        };
    }

    // Interactive mode when we own a real screen. Color is deliberately not part
    // of this decision: NO_COLOR must keep the UI interactive, only monochrome.
    // Otherwise render once and exit, which keeps the output greppable and CI-
    // friendly. (A forced --state is a static demo with no live data to drill
    // into, so it stays render-once too.)
    let interactive = should_run_interactive(clear, stdout_is_tty(), demo.is_none());

    if interactive {
        let readings = verdict::Readings::load(&sources::DataDir::resolve());
        return match app::App::new(readings).run(&style) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("pulse: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // Non-interactive: one frame to stdout.
    let state = match demo {
        Some(name) => Verdict::demo(&name).expect("validated above"),
        None => Verdict::build(&sources::DataDir::resolve()),
    };
    let frame = render(&state, &style, TermSize::resolve());
    let mut out = String::new();
    if clear && style.color {
        out.push_str("\u{1b}[2J\u{1b}[H");
    }
    out.push_str(&frame);
    print!("{out}");

    ExitCode::SUCCESS
}

const HELP: &str = "\
pulse — calm, read-only status for the Linux Ops Suite

USAGE:
    pulse [OPTIONS]

OPTIONS:
    --state <STATE>   force a demo state: healthy | attention | incomplete
                      (default: build the verdict from live suite data)
    --data-dir <DIR>  read suite feeds from DIR instead of the default data dir
                      (same as setting $PULSE_DATA_DIR)
    --dump-view <V>   render one view once and exit (no event loop):
                      default | attention | feeds | details | help | search
                      (append a query for search: --dump-view search aws)
    --no-clear        don't clear the screen first (useful when piping)
    -h, --help        print this help

With no options, Pulse reads the suite's file contracts under $XDG_DATA_HOME
(fallback ~/.local/share) and renders the live verdict. See PULSE_DESIGN.md.
";

mod app;
mod cockpit;
mod sources;
mod tui;
mod verdict;

use verdict::{Source, State, Verdict};

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Build the full frame for a verdict as a single string of lines. Pure: takes
/// the resolved style and size, returns text, touches no I/O — so it can be
/// snapshot-tested directly.
pub(crate) fn render(v: &Verdict, style: &Style, size: TermSize) -> String {
    // Too small to center safely: degrade to a plain, unpadded render that
    // can't clip. Still honors color and the verdict word.
    if size.width < MIN_CENTER_WIDTH || size.height < MIN_CENTER_HEIGHT {
        return render_compact(v, style, size.width as usize);
    }

    let w = size.width as usize;
    let h = size.height as usize;
    let mut lines: Vec<String> = vec![String::new(); h];

    // Wordmark: top-left, dim — but only on the busy states. The healthy screen
    // stays free of it.
    if v.state != State::Healthy {
        lines[0] = format!(" {}pulse{}", style.dim, style.rst);
    }

    // Vertical anchor: the verdict sits slightly above true center (≈40% down),
    // so the empty lower half of the healthy screen reads as intentional calm.
    let anchor = (h * 2) / 5;

    // The center block grows downward from the anchor. Each state contributes
    // its own ordered lines; position is constant, content varies.
    let mut block: Vec<Line> = Vec::new();
    match v.state {
        State::Healthy => {
            block.push(Line::center(verdict_text(v.state), style.verdict(v.state)));
        }
        State::NeedsAttention => {
            block.push(Line::center(verdict_text(v.state), style.verdict(v.state)));
            block.push(Line::blank());
            block.push(count_line(v, style));
            if v.confidence_reduced {
                block.push(Line::blank());
                block.push(Line::center(
                    "confidence reduced by stale feeds".to_string(),
                    style.ylw,
                ));
            }
            block.push(Line::blank());
            block.push(Line::blank());
            for c in cause_lines(v, w, style) {
                block.push(c);
            }
        }
        State::Incomplete => {
            block.push(Line::center(verdict_text(v.state), style.verdict(v.state)));
            block.push(Line::blank());
            block.push(Line::center(incomplete_summary(v), style.bold));
            block.push(Line::blank());
            block.push(Line::blank());
            block.push(Line::center(
                "the suite view may be missing data".to_string(),
                style.dim,
            ));
        }
    }

    // Bottom-anchored furniture (busy states only), placed at fixed rows from
    // the bottom up so it can never be crowded out by the center block:
    //   h-1 timestamp · h-2 hints · h-3 rule · h-4 sources.
    // The source line is pinned here (not appended to the flowing block) so it
    // always renders, even when cause rows are tall — that was the bug a live
    // run with long reasons exposed.
    let busy = v.state != State::Healthy && h >= 8;
    let block_floor = if busy {
        h.saturating_sub(5)
    } else {
        h.saturating_sub(2)
    };

    // Lay the center block down from the anchor, stopping before the floor so it
    // never overruns the furniture (a tiny terminal just shows fewer rows).
    for (off, line) in block.iter().enumerate() {
        let row = anchor + off;
        if row >= block_floor {
            break;
        }
        lines[row] = line.render(w);
    }

    if busy {
        lines[h - 4] = source_line(v, style).render(w);
        lines[h - 3] = format!(
            " {}{}{}",
            style.dim,
            "─".repeat(w.saturating_sub(2)),
            style.rst
        );
        lines[h - 2] = hint_strip(style, w);
    }

    // Timestamp: always present, the dimmest mark on screen, in the lower-right
    // corner. On healthy it is the only thing in the lower half.
    let stamp_row = h - 1;
    lines[stamp_row] = right_align(&timestamp(v, style), &timestamp_plain(v), w);

    lines.join("\n")
}

/// Plain top-left render for terminals too small to center. No padding math, so
/// nothing can clip; the verdict still leads.
fn render_compact(v: &Verdict, style: &Style, width: usize) -> String {
    let mut out = String::new();
    push_compact_line(
        &mut out,
        &format!(
            "{}{}{}",
            style.verdict(v.state),
            verdict_text(v.state),
            style.rst
        ),
        width,
        style.rst,
    );
    match v.state {
        State::Healthy => {}
        State::NeedsAttention => {
            push_compact_line(&mut out, &count_summary(v), width, style.rst);
            if v.confidence_reduced {
                push_compact_line(
                    &mut out,
                    &format!(
                        "{}confidence reduced by stale feeds{}",
                        style.ylw, style.rst
                    ),
                    width,
                    style.rst,
                );
            }
        }
        State::Incomplete => {
            push_compact_line(&mut out, &compact_incomplete_summary(v), width, style.rst);
        }
    }
    push_compact_line(
        &mut out,
        &format!("{}{}{}", style.dim, timestamp_plain(v), style.rst),
        width,
        style.rst,
    );
    out
}

fn should_run_interactive(clear: bool, stdout_tty: bool, live_data: bool) -> bool {
    clear && stdout_tty && live_data
}

fn push_compact_line(out: &mut String, line: &str, width: usize, reset: &str) {
    out.push_str(&clip_ansi(line, width.max(1), reset));
    out.push('\n');
}

/// One renderable line. `body` is already fully rendered (color codes embedded
/// if any); `width` is its *visible* character count, kept separately so
/// centering math is honest about padding even when `body` carries escape
/// codes. A blank line has an empty body and zero width.
struct Line {
    body: String,
    width: usize,
    centered: bool,
}

impl Line {
    /// A centered line of plain `text` styled with one `color` (color may be the
    /// empty string under NO_COLOR). Width is the visible char count of `text`.
    fn center(text: String, color: &'static str) -> Self {
        let width = text.chars().count();
        Line {
            body: wrap(color, &text),
            width,
            centered: true,
        }
    }

    /// A left-anchored (non-centered) line of plain `text` styled with `color`.
    /// `width` is the visible char count; padding inside `text` is the caller's.
    fn raw(text: String, color: &'static str) -> Self {
        let width = text.chars().count();
        Line {
            body: wrap(color, &text),
            width,
            centered: false,
        }
    }

    /// A line that is already rendered (e.g. mixes several colors). The caller
    /// supplies the visible width because it can't be derived from the escaped
    /// body.
    fn prerendered(body: String, width: usize, centered: bool) -> Self {
        Line {
            body,
            width,
            centered,
        }
    }

    fn blank() -> Self {
        Line {
            body: String::new(),
            width: 0,
            centered: true,
        }
    }

    /// Render into a field `w` wide. Centered lines get left padding from the
    /// visible width; non-centered lines render as-is (they carry their own
    /// indentation).
    fn render(&self, w: usize) -> String {
        if self.body.is_empty() {
            return String::new();
        }
        let rendered = if self.centered {
            let pad = w.saturating_sub(self.width) / 2;
            format!("{}{}", " ".repeat(pad), self.body)
        } else {
            self.body.clone()
        };
        clip_ansi(&rendered, w.max(1), "\u{1b}[0m")
    }
}

/// Wrap `text` in `color` + reset, or return it bare when `color` is empty
/// (NO_COLOR), so output never carries a stray reset code.
fn wrap(color: &str, text: &str) -> String {
    if color.is_empty() {
        text.to_string()
    } else {
        format!("{color}{text}\u{1b}[0m")
    }
}

/// The verdict word for a state. Healthy is intentionally lowercase ("all
/// clear") — a calm state does not shout; the others are ALL CAPS for urgency.
pub(crate) fn verdict_text(state: State) -> String {
    match state {
        State::Healthy => "all clear".to_string(),
        State::NeedsAttention => "NEEDS ATTENTION".to_string(),
        State::Incomplete => "INCOMPLETE".to_string(),
    }
}

/// "2 critical · 4 high", collapsed onto one calm line. Drops a zero side so a
/// high-only verdict doesn't read "0 critical". This is the *visible* text;
/// `count_line` adds color.
fn count_summary(v: &Verdict) -> String {
    match (v.critical, v.high) {
        (0, 0) => String::new(),
        (c, 0) => format!("{c} critical"),
        (0, h) => format!("{h} high"),
        (c, h) => format!("{c} critical · {h} high"),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn incomplete_summary(v: &Verdict) -> String {
    match (v.unavailable, v.stale) {
        (0, 0) => "suite view unavailable".to_string(),
        (0, stale) => plural(stale, "source stale", "sources stale"),
        (unavailable, 0) => plural(unavailable, "source unavailable", "sources unavailable"),
        (unavailable, stale) => format!(
            "{} · {}",
            plural(unavailable, "unavailable", "unavailable"),
            plural(stale, "stale", "stale")
        ),
    }
}

fn compact_incomplete_summary(v: &Verdict) -> String {
    match (v.unavailable, v.stale) {
        (0, 0) => "view unavailable".to_string(),
        (0, stale) => plural(stale, "stale", "stale"),
        (unavailable, 0) => plural(unavailable, "unavailable", "unavailable"),
        (unavailable, stale) => format!("{unavailable} unavailable · {stale} stale"),
    }
}

/// The centered count line, with the critical portion in red (the design's one
/// licensed use of red on the default screen) and the high portion bold. Width
/// tracks the visible `count_summary`, so centering stays correct under color.
fn count_line(v: &Verdict, style: &Style) -> Line {
    let visible = count_summary(v);
    let width = visible.chars().count();
    let body = match (v.critical, v.high) {
        (0, 0) => String::new(),
        (c, 0) => wrap(style.red, &format!("{c} critical")),
        (0, h) => wrap(style.bold, &format!("{h} high")),
        (c, h) => format!(
            "{}{}{}",
            wrap(style.red, &format!("{c} critical")),
            " · ",
            wrap(style.bold, &format!("{h} high")),
        ),
    };
    Line::prerendered(body, width, true)
}

/// Build the (already-centered-as-a-block) cause rows. Each row is laid out in
/// three soft columns separated by spaces — no borders — and the whole block is
/// indented to sit under the verdict rather than truly centered, matching the
/// design's left-aligned cause column.
fn cause_lines(v: &Verdict, w: usize, style: &Style) -> Vec<Line> {
    // Column widths sized to the demo content; clamped so we never exceed w.
    let what_w = 18usize;
    let why_w = 26usize;
    // Indent the block to roughly a third in, so it reads as a column under the
    // centered verdict instead of hugging the edge.
    let indent = (w / 6).min(14);
    v.causes
        .iter()
        .map(|c| {
            // fit() pads *or truncates* to an exact width, so a long real-world
            // reason can't overrun into the source column (the demo strings were
            // short enough to hide this; live data is not).
            let line = format!(
                "{:indent$}{}{}{}",
                "",
                fit(&c.what, what_w),
                fit(&c.why, why_w),
                c.source,
                indent = indent,
            );
            Line::raw(line, style.dim_text())
        })
        .collect()
}

/// Pad `s` with trailing spaces, or truncate it with a trailing `…`, so the
/// result is exactly `width` visible columns (assuming single-width chars, which
/// the suite's content is). One column of padding is always kept after a
/// truncated value so adjacent columns never touch.
fn fit(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len <= width.saturating_sub(1) {
        // Fits with at least one trailing space.
        format!("{s:width$}")
    } else {
        // Truncate to width-1 chars and add an ellipsis, then one trailing space.
        let keep: String = s.chars().take(width.saturating_sub(2)).collect();
        format!("{keep}… ")
    }
}

/// The source-confidence line: `sources  ● workstate  ◐ toolfoundry  ○ vault`.
/// Hidden on healthy (built but never pushed there). Markers carry state by
/// shape; color is a bonus. Returns a centered, pre-rendered line and tracks
/// its own visible width as it goes, so the colored body and the width stay in
/// lockstep — no separate "strip the codes" pass to drift out of sync.
fn source_line(v: &Verdict, style: &Style) -> Line {
    let mut body = wrap(style.dim, "sources");
    let mut width = "sources".chars().count();

    body.push_str("  ");
    width += 2;

    for (n, m) in v.sources.iter().enumerate() {
        if n > 0 {
            body.push_str("  ");
            width += 2;
        }
        let (glyph, color) = style.source(m.freshness);
        // marker (1 col) + space + name
        body.push_str(&wrap(color, &glyph.to_string()));
        body.push(' ');
        body.push_str(&m.name);
        width += 2 + m.name.chars().count();
    }

    Line::prerendered(body, width, true)
}

/// The bottom hint strip for busy states. `q quit` is intentionally omitted to
/// keep the strip narrow; quit still works.
fn hint_strip(style: &Style, width: usize) -> String {
    let body = if width >= 88 {
        format!(
            " {d}enter{r}  details      {d}a{r}  attention      {d}f{r}  feeds      {d}/{r}  search      {d}r{r}  cockpit      {d}?{r}  help",
            d = style.dim,
            r = style.rst,
        )
    } else if width >= 64 {
        format!(
            " {d}enter{r} details  {d}a{r} attention  {d}f{r} feeds  {d}/{r} search  {d}r{r} cockpit  {d}?{r} help",
            d = style.dim,
            r = style.rst,
        )
    } else if width >= 36 {
        format!(
            " {d}enter{r} details  {d}a/f/?{r} views  {d}/{r} search",
            d = style.dim,
            r = style.rst,
        )
    } else {
        format!(
            " {d}enter{r}  {d}a{r} {d}f{r} {d}/{r} {d}r{r} {d}?{r}",
            d = style.dim,
            r = style.rst,
        )
    };
    clip_ansi(&body, width.max(1), style.rst)
}

/// Timestamp string with color. Healthy shows the bare relative value
/// ("2m ago"); the busy states prefix "updated " so it isn't ambiguous next to
/// other text. Dimmest mark on screen either way.
fn timestamp(v: &Verdict, style: &Style) -> String {
    format!("{}{}{}", style.dim, timestamp_plain(v), style.rst)
}

fn timestamp_plain(v: &Verdict) -> String {
    match v.state {
        State::Healthy => v.age.clone(),
        _ => format!("updated {}", v.age),
    }
}

/// Right-align `colored` (which embeds escape codes) using `plain` for width,
/// leaving a one-column right margin.
fn right_align(colored: &str, plain: &str, w: usize) -> String {
    let pad = w.saturating_sub(plain.chars().count() + 1);
    clip_ansi(
        &format!("{}{}", " ".repeat(pad), colored),
        w.max(1),
        "\u{1b}[0m",
    )
}

fn clip_ansi(input: &str, width: usize, reset: &str) -> String {
    let mut out = String::new();
    let mut visible = 0usize;
    let mut chars = input.chars().peekable();
    let mut clipped = false;

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            out.push(ch);
            for c in chars.by_ref() {
                out.push(c);
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible >= width {
            clipped = true;
            break;
        }
        out.push(ch);
        visible += 1;
    }

    if !clipped && chars.peek().is_some() {
        clipped = true;
    }
    if clipped && !reset.is_empty() && input.contains('\u{1b}') {
        out.push_str(reset);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Style
// ─────────────────────────────────────────────────────────────────────────────

/// ANSI styling, resolved once. Empty strings when color is off so every call
/// site interpolates unconditionally — same rule and shape as rex-check's
/// `Style`. `color` records whether color is live (used to gate the screen
/// clear).
pub(crate) struct Style {
    color: bool,
    pub(crate) bold: &'static str,
    pub(crate) dim: &'static str,
    pub(crate) grn: &'static str,
    pub(crate) ylw: &'static str,
    pub(crate) red: &'static str,
    pub(crate) cyn: &'static str,
    pub(crate) rst: &'static str,
}

impl Style {
    /// Color on only when stdout is a TTY and `NO_COLOR` is unset — the suite
    /// rule.
    fn resolve() -> Self {
        let on = stdout_is_tty() && env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                color: true,
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                red: "\u{1b}[31m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Self::plain()
        }
    }

    /// A color-off style (all codes empty). Used under NO_COLOR / non-TTY.
    fn plain() -> Self {
        Style {
            color: false,
            bold: "",
            dim: "",
            grn: "",
            ylw: "",
            red: "",
            cyn: "",
            rst: "",
        }
    }

    /// Color for an attention severity (Attention view). Critical red, high
    /// amber, the rest dim — color is a bonus on top of the word label.
    pub(crate) fn severity(&self, s: verdict::Severity) -> &'static str {
        use verdict::Severity;
        match s {
            Severity::Critical => self.red,
            Severity::High => self.ylw,
            Severity::Medium => self.dim,
            Severity::Low => self.dim,
        }
    }

    #[cfg(test)]
    pub(crate) fn plain_for_test() -> Self {
        Self::plain()
    }

    /// Color for a verdict word: healthy green, attention amber, incomplete
    /// amber (it's a confidence problem, not a critical one). Bold-less; the
    /// size and centering already make it the focal point.
    fn verdict(&self, state: State) -> &'static str {
        match state {
            State::Healthy => self.grn,
            State::NeedsAttention => self.ylw,
            State::Incomplete => self.ylw,
        }
    }

    /// Dim styling for secondary text (cause rows).
    fn dim_text(&self) -> &'static str {
        self.dim
    }

    /// (glyph, color) for a source marker. Shape carries state; color is the
    /// bonus. ASCII fallback keeps it readable where the glyphs don't render —
    /// here we keep the Unicode glyphs and rely on NO_COLOR for legibility, as
    /// the design allows.
    fn source(&self, f: Source) -> (char, &'static str) {
        match f {
            Source::Current => ('●', self.grn),
            Source::Stale => ('◐', self.ylw),
            Source::Missing => ('○', self.dim),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Terminal size / TTY  (hand-rolled libc, no dependency — see rex-check)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolved terminal size in character cells.
#[derive(Clone, Copy)]
pub(crate) struct TermSize {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl TermSize {
    #[cfg(test)]
    pub(crate) fn for_test(width: u16, height: u16) -> Self {
        TermSize { width, height }
    }

    /// Resolve the terminal size: ask the tty via `TIOCGWINSZ`; if that fails
    /// (e.g. stdout is a pipe), fall back to `$COLUMNS`/`$LINES`, then to a
    /// classic 80x24. Never returns zero in either dimension.
    pub(crate) fn resolve() -> Self {
        if let Some((w, h)) = ioctl_winsize() {
            if w > 0 && h > 0 {
                return TermSize {
                    width: w,
                    height: h,
                };
            }
        }
        let w = env_u16("COLUMNS").unwrap_or(FALLBACK_WIDTH);
        let h = env_u16("LINES").unwrap_or(FALLBACK_HEIGHT);
        TermSize {
            width: w.max(1),
            height: h.max(1),
        }
    }
}

fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok()?.trim().parse().ok()
}

/// Ask the kernel for stdout's window size via `ioctl(TIOCGWINSZ)`. Returns
/// `(cols, rows)` or None when stdout isn't a terminal. One tiny libc call;
/// avoids a dependency just to center text — same spirit as rex-check's
/// hand-rolled `isatty`.
fn ioctl_winsize() -> Option<(u16, u16)> {
    // struct winsize { ws_row, ws_col, ws_xpixel, ws_ypixel } — all c_ushort.
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    // TIOCGWINSZ is 0x5413 on Linux. This binary targets Linux (the suite is
    // Linux-only), so the constant is fixed here rather than pulled from libc.
    const TIOCGWINSZ: u64 = 0x5413;
    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: ioctl writes a Winsize into our stack buffer for the TIOCGWINSZ
    // request; the buffer is correctly sized and aligned, and we only read it
    // back on success.
    let rc = unsafe { ioctl(1, TIOCGWINSZ, &mut ws as *mut Winsize) };
    if rc == 0 {
        Some((ws.ws_col, ws.ws_row))
    } else {
        None
    }
}

/// Whether fd 1 (stdout) is a TTY, via `isatty(3)`. Mirrors rex-check.
fn stdout_is_tty() -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(1) == 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_style() -> Style {
        // Force color off so assertions compare visible text only.
        Style::plain_for_test()
    }

    fn size(w: u16, h: u16) -> TermSize {
        TermSize {
            width: w,
            height: h,
        }
    }

    #[test]
    fn verdict_words_match_the_design() {
        assert_eq!(verdict_text(State::Healthy), "all clear");
        assert_eq!(verdict_text(State::NeedsAttention), "NEEDS ATTENTION");
        assert_eq!(verdict_text(State::Incomplete), "INCOMPLETE");
    }

    #[test]
    fn no_color_does_not_disable_interactive_mode() {
        assert!(should_run_interactive(true, true, true));
        assert!(!should_run_interactive(false, true, true));
        assert!(!should_run_interactive(true, false, true));
        assert!(!should_run_interactive(true, true, false));
    }

    #[test]
    fn healthy_screen_is_almost_empty() {
        let v = Verdict::demo_healthy();
        let frame = render(&v, &plain_style(), size(80, 24));
        let non_empty: Vec<&str> = frame.lines().filter(|l| !l.trim().is_empty()).collect();
        // Only two marks on the whole screen: the verdict and the timestamp.
        assert_eq!(non_empty.len(), 2, "healthy screen had: {non_empty:?}");
        assert!(frame.contains("all clear"));
        assert!(frame.contains("2m ago"));
        // No wordmark, no chrome, no source markers on healthy.
        assert!(!frame.contains("pulse"));
        assert!(!frame.contains("sources"));
        assert!(!frame.contains("enter"));
        assert!(!frame.contains('●'));
    }

    #[test]
    fn healthy_timestamp_has_no_label() {
        let v = Verdict::demo_healthy();
        let frame = render(&v, &plain_style(), size(80, 24));
        assert!(frame.contains("2m ago"));
        assert!(!frame.contains("updated"));
    }

    #[test]
    fn attention_screen_shows_detail_and_chrome() {
        let v = Verdict::demo("attention").unwrap();
        let frame = render(&v, &plain_style(), size(80, 24));
        assert!(frame.contains("NEEDS ATTENTION"));
        assert!(frame.contains("2 critical · 4 high"));
        assert!(frame.contains("confidence reduced by stale feeds"));
        assert!(frame.contains("deploy-prod.sh"));
        assert!(frame.contains("token-like secret"));
        assert!(frame.contains("bulwark"));
        // Busy states regain the wordmark and the "updated" label.
        assert!(frame.contains("pulse"));
        assert!(frame.contains("updated 2m ago"));
        // Source line + hint strip present.
        assert!(frame.contains("sources"));
        assert!(frame.contains("enter"));
    }

    #[test]
    fn incomplete_has_sources_but_no_causes() {
        let v = Verdict::demo("incomplete").unwrap();
        let frame = render(&v, &plain_style(), size(80, 24));
        assert!(frame.contains("INCOMPLETE"));
        assert!(frame.contains("2 sources unavailable"));
        assert!(frame.contains("the suite view may be missing data"));
        assert!(frame.contains("sources"));
        // No cause rows on incomplete.
        assert!(!frame.contains("deploy-prod.sh"));
    }

    #[test]
    fn stale_incomplete_summary_names_stale_sources() {
        let mut v = Verdict::demo("incomplete").unwrap();
        v.unavailable = 0;
        v.stale = 1;
        let frame = render(&v, &plain_style(), size(80, 24));
        assert!(frame.contains("1 source stale"));
        assert!(!frame.contains("0 sources unavailable"));
    }

    #[test]
    fn count_summary_drops_zero_sides() {
        let mut v = Verdict::demo("attention").unwrap();
        v.critical = 0;
        v.high = 3;
        assert_eq!(count_summary(&v), "3 high");
        v.critical = 1;
        v.high = 0;
        assert_eq!(count_summary(&v), "1 critical");
        v.critical = 2;
        v.high = 4;
        assert_eq!(count_summary(&v), "2 critical · 4 high");
    }

    #[test]
    fn layout_position_is_stable_across_states() {
        // The verdict should land on the same row in every state — elements
        // appear/vanish but never move.
        let row_of = |state: &str| -> usize {
            let v = Verdict::demo(state).unwrap();
            let frame = render(&v, &plain_style(), size(80, 24));
            frame
                .lines()
                .position(|l| {
                    l.contains("all clear")
                        || l.contains("NEEDS ATTENTION")
                        || l.contains("INCOMPLETE")
                })
                .unwrap()
        };
        let a = row_of("healthy");
        let b = row_of("attention");
        let c = row_of("incomplete");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn compact_terminal_does_not_panic_and_leads_with_verdict() {
        let v = Verdict::demo_healthy();
        let frame = render(&v, &plain_style(), size(10, 4));
        assert!(frame.starts_with("all clear"));
        assert!(frame.contains("2m ago"));
    }

    #[test]
    fn rendered_lines_do_not_exceed_viewport_width() {
        let style = plain_style();
        for (state, w, h) in [
            ("attention", 80usize, 24u16),
            ("incomplete", 80, 24),
            ("attention", 36, 12),
            ("incomplete", 20, 6),
        ] {
            let v = Verdict::demo(state).unwrap();
            let frame = render(&v, &style, size(w as u16, h));
            let max = frame.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            assert!(
                max <= w,
                "{state} at {w}x{h} overflowed to {max} columns:\n{frame}"
            );
        }
    }
}
