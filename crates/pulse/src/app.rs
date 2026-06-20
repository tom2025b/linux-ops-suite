//! The interactive Pulse app: state, event loop, and the drill-down views the
//! bottom hint strip advertises (`a` attention, `f` feeds, `Enter` details, `/`
//! search, `?` help).
//!
//! Design constraints honored here (PULSE_DESIGN.md):
//!   - The default screen stays glance mode; the views appear only when asked.
//!   - Every view has an obvious **non-Esc** close path (`q`, or the view's own
//!     toggle key), because the operator may be on a keyboard without a reliable
//!     Escape. Esc still works as a convenience.
//!   - Read-only throughout: navigation changes what is shown, never the suite.
//!
//! The loop reads one [`crate::tui::Key`] at a time and repaints. Data is read
//! once into [`Readings`]; views render from it, so keypresses never re-hit the
//! filesystem.

use std::io;

use suite_ui::{Theme, Tui, TuiOptions};

use crate::tui::{self, Key};
use crate::verdict::{Readings, Source, Verdict};
use crate::{render, Style, TermSize};

/// Which screen is showing. `Default` is the verdict; the rest are the views the
/// hint strip names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Default,
    Attention,
    Feeds,
    Details,
    Help,
    Search,
}

/// Interactive app state: the one-time readings, the derived verdict, the
/// current view, and the search query buffer.
pub struct App {
    readings: Readings,
    verdict: Verdict,
    view: View,
    query: String,
    /// Set true once the user asks to quit; ends the loop.
    quit: bool,
    /// Set true by `r`; the event loop sees it, foreground-launches the RexOps
    /// cockpit, and clears it. A request flag (not the launch itself) keeps
    /// `handle` pure and unit-testable — the loop owns the I/O.
    launch_cockpit: bool,
    /// A transient one-line status shown on the next repaint (e.g. "rexops not
    /// found"). Cleared by the next keypress so it never lingers.
    status: Option<String>,
    /// The resolved suite-ui palette (accent + the `NO_COLOR` gate). The ratatui
    /// draw layer ([`crate::view`]) styles the ported views through it; the legacy
    /// string renderer still uses the bespoke `Style` for views not yet ported.
    theme: Theme,
}

impl App {
    pub fn new(readings: Readings, theme: Theme) -> Self {
        let verdict = Verdict::from_readings(&readings);
        App {
            readings,
            verdict,
            view: View::Default,
            query: String::new(),
            quit: false,
            launch_cockpit: false,
            status: None,
            theme,
        }
    }

    /// The resolved theme this app draws through.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// The current view — the draw layer dispatches on it.
    pub(crate) fn view(&self) -> View {
        self.view
    }

    /// The derived verdict the default screen renders.
    pub(crate) fn verdict(&self) -> &Verdict {
        &self.verdict
    }

    /// The transient status line, if any (e.g. "rexops not found").
    pub(crate) fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Build the legacy string frame for a view the ratatui draw layer hasn't
    /// ported yet. The migration bridge: rendered monochrome (the string carries
    /// no ANSI under a plain style) and blitted as one `Paragraph`. Removed view
    /// by view as T6–T8 land real draws, and entirely in T10.
    pub(crate) fn legacy_frame(&self, size: TermSize) -> String {
        self.frame(&Style::plain_for_bridge(), size)
    }

    /// Run the interactive loop until the user quits. Owns the terminal via the
    /// shared [`suite_ui::Tui`] guard for its duration; the terminal is restored
    /// when `tui` drops (and by ratatui's restoring panic hook, installed by
    /// `Tui::new`). Tests drive the pure state machine via [`App::handle`]
    /// directly instead of calling `run`.
    ///
    /// MIGRATION BRIDGE (T2): the legacy string view is still built by
    /// [`App::frame`], but it is now blitted through ratatui as a single
    /// `Paragraph` instead of raw ANSI. Because a `Paragraph` would render ANSI
    /// escapes literally, the bridge renders the frame with a **plain** (no-code)
    /// style — so this step is intentionally monochrome. Real suite-ui theming
    /// and per-view widgets replace this `frame()`/`Paragraph` path in later
    /// steps (T5–T8); the loop shape here is the final one. The resolved theme
    /// lives on `self` ([`App::theme`]) ready for those draws.
    pub fn run(mut self) -> io::Result<()> {
        // Read-only dashboard: hide the cursor; require a real tty so we fail
        // with a friendly message rather than entering raw mode on a pipe.
        let mut tui = Tui::new(TuiOptions {
            hide_cursor: true,
            require_tty: true,
            ..Default::default()
        })
        .map_err(io::Error::other)?;

        loop {
            // The ratatui draw layer paints the current view (suite-ui chrome for
            // ported views, the legacy string frame for the rest — see crate::view).
            tui.terminal().draw(|f| crate::view::draw(f, &self))?;
            if self.quit {
                break;
            }
            let key = tui::read_event()?;
            self.handle(key);
            if self.quit {
                // Repaint nothing more; just restore and leave.
                break;
            }
            // `r` requested the cockpit: hand the terminal to `rexops tui` and
            // come back. The launch is here (not in `handle`) so `handle` stays
            // pure; `Tui::suspended` guarantees Pulse's terminal is restored
            // afterwards. A missing/failed rexops becomes a transient status
            // line, never an error that ends the session.
            if std::mem::take(&mut self.launch_cockpit) {
                self.status = crate::cockpit::open(&mut tui).status_line();
            }
        }
        Ok(())
    }

    /// Render a single named view to a frame, without the event loop. Used by
    /// `--dump-view` to preview/snapshot a view deterministically (no PTY timing
    /// games), and handy in tests. Unknown names return `None`.
    pub fn dump(
        &mut self,
        view: &str,
        query: &str,
        style: &Style,
        size: TermSize,
    ) -> Option<String> {
        self.view = match view {
            "default" => View::Default,
            "attention" => View::Attention,
            "feeds" => View::Feeds,
            "details" => View::Details,
            "help" => View::Help,
            "search" => View::Search,
            _ => return None,
        };
        self.query = query.to_string();
        Some(self.frame(style, size))
    }

    /// Apply one key to the state. Pure (no I/O), so the whole navigation model
    /// is unit-testable by feeding keys and asserting on `view`/`query`/`quit`.
    pub fn handle(&mut self, key: Key) {
        // The search box captures most keys while it's open (letters, incl. `r`,
        // are literal text there — the cockpit shortcut yields to typing).
        if self.view == View::Search {
            match key {
                Key::Enter | Key::Esc => self.view = View::Default,
                Key::Eof => self.quit = true,
                Key::Backspace => {
                    self.query.pop();
                }
                Key::Char(c) => self.query.push(c),
                Key::Other => {}
            }
            return;
        }

        // Any acted-on key clears a lingering status line (e.g. the "rexops not
        // found" note) so it shows for exactly one interaction.
        self.status = None;

        match key {
            Key::Char('q') | Key::Eof => self.quit = true,
            // `r` opens the full RexOps cockpit. We only *request* it here (pure);
            // the event loop performs the foreground launch. Works from every
            // view except the search box (handled above).
            Key::Char('r') => self.launch_cockpit = true,
            // Esc and Enter-from-a-view return to the default screen; from the
            // default screen Enter opens Details.
            Key::Esc => self.view = View::Default,
            Key::Enter => {
                self.view = if self.view == View::Default {
                    View::Details
                } else {
                    View::Default
                };
            }
            // Each letter toggles its view (press again to return) — a non-Esc
            // close path for every screen.
            Key::Char('a') => self.view = self.toggle(View::Attention),
            Key::Char('f') => self.view = self.toggle(View::Feeds),
            Key::Char('?') => self.view = self.toggle(View::Help),
            Key::Char('/') => {
                self.query.clear();
                self.view = View::Search;
            }
            _ => {}
        }
    }

    /// Toggle `target`: open it, or return to Default if already there.
    fn toggle(&self, target: View) -> View {
        if self.view == target {
            View::Default
        } else {
            target
        }
    }

    /// Render the current view to a full frame for `size`, with any transient
    /// status line overlaid on the bottom row.
    fn frame(&self, style: &Style, size: TermSize) -> String {
        let base = match self.view {
            View::Default => render(&self.verdict, style, size),
            View::Attention => self.view_attention(style, size),
            View::Feeds => self.view_feeds(style, size),
            View::Details => self.view_details(style, size),
            View::Help => self.view_help(style, size),
            View::Search => self.view_search(style, size),
        };
        self.overlay_status(base, style, size)
    }

    /// Replace the bottom row of `frame` with the transient status line when one
    /// is set (e.g. "rexops not found"). Kept dim and on the last row so it reads
    /// as an aside, never disturbing the verdict's position above it. No-op when
    /// there's no status.
    fn overlay_status(&self, frame: String, style: &Style, size: TermSize) -> String {
        let Some(msg) = &self.status else {
            return frame;
        };
        let h = size.height.max(1) as usize;
        let mut lines: Vec<String> = frame.split('\n').map(str::to_string).collect();
        // Ensure the frame is tall enough to address the last row.
        if lines.len() < h {
            lines.resize(h, String::new());
        }
        let last = lines.len().saturating_sub(1);
        lines[last] = clip_ansi(
            &format!(
                " {dim}{msg}{rst}",
                dim = style.dim,
                msg = msg,
                rst = style.rst
            ),
            size.width.max(1) as usize,
            style.rst,
        );
        lines.join("\n")
    }

    // ── Views ────────────────────────────────────────────────────────────────
    // Each view is a simple full-height panel: a dim title, the content, and a
    // footer telling the operator how to get back. Deliberately plain — these
    // are drill-downs, not the calm verdict, so density is acceptable here.

    fn view_attention(&self, style: &Style, size: TermSize) -> String {
        let items = self.readings.all_attention();
        let mut body = Vec::new();
        if items.is_empty() {
            body.push("  nothing needs attention.".to_string());
        } else {
            for a in &items {
                let sev = severity_label(a.severity);
                let sev_c = style.severity(a.severity);
                body.push(format!(
                    "  {sev_c}{sev:>8}{rst}  {what}  {dim}— {why} ({source}){rst}",
                    sev = sev,
                    what = a.what,
                    why = a.why,
                    source = a.source,
                    sev_c = sev_c,
                    dim = style.dim,
                    rst = style.rst,
                ));
            }
        }
        panel(
            style,
            size,
            "ATTENTION",
            &body,
            "a / Esc  back      q  quit",
        )
    }

    fn view_feeds(&self, style: &Style, size: TermSize) -> String {
        let marks = self.readings.source_marks();
        let mut body = Vec::new();
        for m in &marks {
            let (glyph, word, color) = match m.freshness {
                Source::Current => ('●', "current", style.grn),
                Source::Stale => ('◐', "stale", style.ylw),
                Source::Missing => ('○', "missing", style.dim),
            };
            body.push(format!(
                "  {color}{glyph}{rst}  {name:<14}{dim}{word}{rst}",
                color = color,
                glyph = glyph,
                name = m.name,
                word = word,
                dim = style.dim,
                rst = style.rst,
            ));
        }
        let age = if let Some(b) = &self.readings.freshness.built_at {
            format!("  snapshot built {b}")
        } else {
            "  no snapshot found".to_string()
        };
        body.push(String::new());
        body.push(format!(
            "{dim}{age}{rst}",
            dim = style.dim,
            age = age,
            rst = style.rst
        ));
        panel(style, size, "FEEDS", &body, "f / Esc  back      q  quit")
    }

    fn view_details(&self, style: &Style, size: TermSize) -> String {
        let v = &self.verdict;
        let mut body = vec![
            format!("  verdict   {}", crate::verdict_text(v.state)),
            format!("  data age  {}", v.age),
        ];
        if v.critical + v.high > 0 {
            body.push(format!(
                "  findings  {} critical, {} high",
                v.critical, v.high
            ));
        }
        body.push(String::new());
        body.push("  press a for the full attention list, f for feeds.".to_string());
        panel(
            style,
            size,
            "DETAILS",
            &body,
            "Enter / Esc  back      q  quit",
        )
    }

    fn view_help(&self, style: &Style, size: TermSize) -> String {
        let body = vec![
            "  Enter   details for the current verdict".to_string(),
            "  a       attention — everything that needs action".to_string(),
            "  f       feeds — source freshness & confidence".to_string(),
            "  /       search across visible status".to_string(),
            "  r       open the full RexOps cockpit".to_string(),
            "  ?       this help".to_string(),
            "  q       quit".to_string(),
            String::new(),
            "  Esc or the view's own key returns to the verdict.".to_string(),
        ];
        panel(style, size, "HELP", &body, "? / Esc  back      q  quit")
    }

    fn view_search(&self, style: &Style, size: TermSize) -> String {
        let q = &self.query;
        let mut body = vec![
            format!("  search: {}{}{}_", style.cyn, q, style.rst),
            String::new(),
        ];
        if q.is_empty() {
            body.push(format!(
                "  {}type to filter; Enter or Esc to close.{}",
                style.dim, style.rst
            ));
        } else {
            let hits = self.search_hits(q);
            if hits.is_empty() {
                body.push(format!("  {}no matches.{}", style.dim, style.rst));
            } else {
                for h in hits {
                    body.push(format!("  {h}"));
                }
            }
        }
        panel(
            style,
            size,
            "SEARCH",
            &body,
            "Enter / Esc  close      type to filter",
        )
    }

    /// Case-insensitive substring search across the visible status surface:
    /// attention items and source names. Returns formatted match lines.
    fn search_hits(&self, q: &str) -> Vec<String> {
        let needle = q.to_lowercase();
        let mut hits = Vec::new();
        for a in self.readings.all_attention() {
            let hay = format!("{} {} {}", a.what, a.why, a.source).to_lowercase();
            if hay.contains(&needle) {
                hits.push(format!("{}  — {} ({})", a.what, a.why, a.source));
            }
        }
        for m in self.readings.source_marks() {
            if m.name.to_lowercase().contains(&needle) {
                hits.push(format!("source: {}", m.name));
            }
        }
        hits
    }
}

/// Lay out a simple view panel: `pulse · TITLE` top-left, the body lines from
/// the top, and a dim footer on the last row telling the operator how to leave.
fn panel(style: &Style, size: TermSize, title: &str, body: &[String], footer: &str) -> String {
    let h = size.height.max(4) as usize;
    let w = size.width.max(1) as usize;
    let mut lines: Vec<String> = vec![String::new(); h];

    lines[0] = clip_ansi(
        &format!(
            " {dim}pulse · {title}{rst}",
            dim = style.dim,
            title = title,
            rst = style.rst
        ),
        w,
        style.rst,
    );

    // Body starts two rows down, clipped to leave room for the footer.
    let top = 2;
    for (i, line) in body.iter().enumerate() {
        let row = top + i;
        if row >= h - 2 {
            break;
        }
        lines[row] = clip_ansi(line, w, style.rst);
    }

    lines[h - 2] = clip_ansi(
        &format!(
            " {dim}{}{rst}",
            "─".repeat(w.saturating_sub(2)),
            dim = style.dim,
            rst = style.rst
        ),
        w,
        style.rst,
    );
    lines[h - 1] = clip_ansi(
        &format!(
            " {dim}{footer}{rst}",
            dim = style.dim,
            footer = footer,
            rst = style.rst
        ),
        w,
        style.rst,
    );
    lines.join("\n")
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

fn severity_label(s: crate::verdict::Severity) -> &'static str {
    use crate::verdict::Severity;
    match s {
        Severity::Critical => "critical",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Attention, BinaryCheck, BulwarkView, Severity, SnapshotFreshness};
    use crate::verdict::Readings;

    /// A Readings with a couple of attention items and a missing source, enough
    /// to exercise the views and search without touching disk.
    fn sample_readings() -> Readings {
        Readings {
            freshness: SnapshotFreshness {
                built_at: Some("2026-06-14T12:00:00Z".to_string()),
                sections: vec![("scripts", crate::sources::Freshness::Current)],
            },
            rexops: Some(crate::sources::RexopsView {
                generated_at: Some("2026-06-14T12:00:00Z".to_string()),
                sources: vec![
                    ("workstate".to_string(), true),
                    ("scriptvault".to_string(), false),
                ],
                attention: vec![Attention {
                    what: "deploy-prod.sh".to_string(),
                    why: "AWS access key ID detected".to_string(),
                    source: "bulwark".to_string(),
                    severity: Severity::Critical,
                }],
            }),
            bulwark: BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            jobs: Vec::new(),
            binaries: ["workstate", "bulwark", "proto", "toolfoundry", "vault"]
                .iter()
                .map(|&name| BinaryCheck {
                    name,
                    present: true,
                })
                .collect(),
            now: Some(0),
        }
    }

    fn app() -> App {
        // Colour forced off so navigation tests are deterministic regardless of
        // the runner's NO_COLOR; these tests assert on view/query state, not hue.
        App::new(sample_readings(), Theme::with_color(false))
    }

    #[test]
    fn letter_keys_open_their_views() {
        let mut a = app();
        assert_eq!(a.view, View::Default);
        a.handle(Key::Char('a'));
        assert_eq!(a.view, View::Attention);
        a.handle(Key::Char('f'));
        assert_eq!(a.view, View::Feeds);
        a.handle(Key::Char('?'));
        assert_eq!(a.view, View::Help);
    }

    #[test]
    fn enter_opens_details_then_returns() {
        let mut a = app();
        a.handle(Key::Enter);
        assert_eq!(a.view, View::Details);
        a.handle(Key::Enter);
        assert_eq!(a.view, View::Default);
    }

    #[test]
    fn each_view_has_a_non_esc_close_path() {
        // The view's own key toggles it back to Default — no Esc required.
        for (open, key) in [
            (View::Attention, Key::Char('a')),
            (View::Feeds, Key::Char('f')),
            (View::Help, Key::Char('?')),
        ] {
            let mut a = app();
            a.handle(key);
            assert_eq!(a.view, open);
            a.handle(key);
            assert_eq!(a.view, View::Default, "{open:?} should toggle closed");
        }
    }

    #[test]
    fn esc_always_returns_to_default() {
        let mut a = app();
        a.handle(Key::Char('a'));
        a.handle(Key::Esc);
        assert_eq!(a.view, View::Default);
    }

    #[test]
    fn q_quits_from_any_view() {
        let mut a = app();
        a.handle(Key::Char('f'));
        a.handle(Key::Char('q'));
        assert!(a.quit);
    }

    #[test]
    fn plain_style_stays_fully_interactive() {
        let style = Style::plain_for_test();
        assert!(
            !style.color,
            "test must exercise the NO_COLOR-style renderer"
        );

        let mut a = app();
        assert_eq!(a.view, View::Default);
        assert!(a
            .frame(&style, TermSize::for_test(80, 24))
            .contains("NEEDS ATTENTION"));

        a.handle(Key::Enter);
        assert_eq!(a.view, View::Details);
        assert!(a
            .frame(&style, TermSize::for_test(80, 24))
            .contains("pulse · DETAILS"));

        a.handle(Key::Enter);
        assert_eq!(a.view, View::Default);

        a.handle(Key::Char('f'));
        assert_eq!(a.view, View::Feeds);
        assert!(a
            .frame(&style, TermSize::for_test(80, 24))
            .contains("pulse · FEEDS"));

        a.handle(Key::Char('/'));
        a.handle(Key::Char('a'));
        a.handle(Key::Char('w'));
        a.handle(Key::Char('s'));
        assert_eq!(a.view, View::Search);
        assert_eq!(a.query, "aws");
        assert!(!a.quit);

        a.handle(Key::Enter);
        assert_eq!(a.view, View::Default);

        a.handle(Key::Char('q'));
        assert!(a.quit);
    }

    #[test]
    fn r_requests_the_cockpit_from_any_view() {
        // From the default screen and from an open view, `r` sets the launch
        // request (the loop performs the actual foreground launch).
        let mut a = app();
        a.handle(Key::Char('r'));
        assert!(a.launch_cockpit, "r requests the cockpit from default");

        let mut b = app();
        b.handle(Key::Char('a')); // open a view first
        b.handle(Key::Char('r'));
        assert!(b.launch_cockpit, "r requests the cockpit from a view too");
    }

    #[test]
    fn r_is_literal_text_in_the_search_box() {
        // In search, `r` must type into the query, NOT launch the cockpit.
        let mut a = app();
        a.handle(Key::Char('/'));
        a.handle(Key::Char('r'));
        assert_eq!(a.query, "r");
        assert!(!a.launch_cockpit, "r in search must not request a launch");
    }

    #[test]
    fn a_status_line_overlays_the_bottom_row_then_clears_on_next_key() {
        let style = Style::plain_for_test();
        let mut a = app();
        a.status = Some("rexops not found".to_string());
        let frame = a.frame(&style, TermSize::for_test(80, 24));
        let last = frame.split('\n').next_back().unwrap_or("");
        assert!(
            last.contains("rexops not found"),
            "status must overlay the last row:\n{frame}"
        );
        // Any acted-on key clears it.
        a.handle(Key::Char('a'));
        assert!(a.status.is_none(), "status clears on the next key");
    }

    #[test]
    fn search_captures_typing_and_backspace() {
        let mut a = app();
        a.handle(Key::Char('/'));
        assert_eq!(a.view, View::Search);
        for c in "deploy".chars() {
            a.handle(Key::Char(c));
        }
        assert_eq!(a.query, "deploy");
        a.handle(Key::Backspace);
        assert_eq!(a.query, "deplo");
        // 'q' must be a literal in the search box, not a quit.
        a.handle(Key::Char('q'));
        assert_eq!(a.query, "deploq");
        assert!(!a.quit);
        // Enter closes the box back to the verdict.
        a.handle(Key::Enter);
        assert_eq!(a.view, View::Default);
    }

    #[test]
    fn search_finds_an_attention_item() {
        let a = app();
        let hits = a.search_hits("aws");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].contains("deploy-prod.sh"));
        assert!(a.search_hits("nothing-here").is_empty());
    }

    #[test]
    fn views_render_without_panicking_at_small_and_normal_sizes() {
        let style = Style::plain_for_test();
        for v in [
            View::Attention,
            View::Feeds,
            View::Details,
            View::Help,
            View::Search,
        ] {
            let mut a = app();
            a.view = v;
            for (w, h) in [(80u16, 24u16), (20, 6), (200, 60)] {
                let frame = a.frame(&style, TermSize::for_test(w, h));
                assert!(!frame.is_empty());
            }
        }
    }

    #[test]
    fn views_do_not_exceed_viewport_width() {
        let style = Style::plain_for_test();
        for v in [
            View::Attention,
            View::Feeds,
            View::Details,
            View::Help,
            View::Search,
        ] {
            let mut a = app();
            a.view = v;
            let frame = a.frame(&style, TermSize::for_test(20, 6));
            let max = frame.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            assert!(max <= 20, "{v:?} overflowed to {max} columns:\n{frame}");
        }
    }

    #[test]
    fn status_line_is_clipped_to_viewport_width() {
        let style = Style::plain_for_test();
        let mut a = app();
        a.status = Some(
            "rexops not found on PATH — install it to open the cockpit (or run `rexops tui`)."
                .to_string(),
        );
        let frame = a.frame(&style, TermSize::for_test(40, 10));
        let last = frame.split('\n').next_back().unwrap_or("");
        assert!(last.chars().count() <= 40, "status overflowed: {last}");
    }
}
