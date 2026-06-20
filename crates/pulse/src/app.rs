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
use crate::verdict::{Readings, Verdict};
use crate::TermSize;

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
    /// draw layer ([`crate::view`]) styles every view through it.
    theme: Theme,
}

impl App {
    pub fn new(readings: Readings, theme: Theme) -> Self {
        let verdict = Verdict::from_readings(&readings);
        Self::with_verdict(readings, verdict, theme)
    }

    /// Build an app around a pre-computed `verdict` (e.g. a `--state` demo) with
    /// empty readings — for the one-shot non-interactive render, which only shows
    /// the default verdict screen and has no drill-down lists to populate.
    pub fn from_verdict(verdict: Verdict, theme: Theme) -> Self {
        Self::with_verdict(Readings::empty(), verdict, theme)
    }

    fn with_verdict(readings: Readings, verdict: Verdict, theme: Theme) -> Self {
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

    /// The current search query buffer (what the user has typed in the box).
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    /// The attention items for the Attention/Search views (data the draw layer
    /// renders; the verdict logic stays in [`crate::verdict`]).
    pub(crate) fn attention_items(&self) -> Vec<crate::sources::Attention> {
        self.readings.all_attention()
    }

    /// The per-source freshness marks for the Feeds view.
    pub(crate) fn source_marks(&self) -> Vec<crate::verdict::SourceMark> {
        self.readings.source_marks()
    }

    /// When the underlying suite snapshot was built, if known — the Feeds
    /// view's provenance line.
    pub(crate) fn snapshot_built_at(&self) -> Option<&str> {
        self.readings.freshness.built_at.as_deref()
    }

    /// The search hit lines for the current query (delegates to the existing
    /// pure search). Exposed so the Search draw renders them.
    pub(crate) fn search_hit_lines(&self, q: &str) -> Vec<String> {
        self.search_hits(q)
    }

    /// Test-only: an app on representative sample readings (one critical
    /// attention item, mixed-freshness sources), set to `view` with `query`, so
    /// `crate::view`'s draw snapshots can exercise each screen without disk I/O.
    /// Colour is forced off so snapshots (glyphs/layout only) are deterministic.
    #[cfg(test)]
    pub(crate) fn sample_with(view: View, query: &str) -> Self {
        let mut app = App::new(crate::verdict::sample_readings(), Theme::with_color(false));
        app.view = view;
        app.query = query.to_string();
        app
    }

    /// Test-only: set the transient status line, so `crate::view`'s draw can be
    /// exercised with an overlay present.
    #[cfg(test)]
    pub(crate) fn with_status(mut self, msg: &str) -> Self {
        self.status = Some(msg.to_string());
        self
    }

    /// Run the interactive loop until the user quits. Owns the terminal via the
    /// shared [`suite_ui::Tui`] guard for its duration; the terminal is restored
    /// when `tui` drops (and by ratatui's restoring panic hook, installed by
    /// `Tui::new`). Tests drive the pure state machine via [`App::handle`]
    /// directly instead of calling `run`.
    ///
    /// Each frame is painted by the ratatui draw layer ([`crate::view::draw`])
    /// through the resolved [`theme`](App::theme); input comes from crossterm via
    /// [`crate::tui::read_event`]. The `r` cockpit hand-off uses [`Tui::suspended`]
    /// so Pulse's terminal is restored even if the child errors.
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
            // The ratatui draw layer (crate::view) paints the current view in
            // suite-ui chrome through the theme.
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

    /// Render a single named view headlessly, without the event loop — the
    /// deterministic preview/snapshot path behind `--dump-view` (no PTY timing
    /// games). Goes through the same [`crate::view::draw`] the live loop uses (via
    /// a ratatui `TestBackend`), so a dump is the real UI. Unknown names return
    /// `None`.
    pub fn dump(&mut self, view: &str, query: &str, size: TermSize) -> Option<String> {
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
        Some(crate::view::render_to_string(self, size.width, size.height))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::sample_readings;

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

    /// Render the app's current view through the real ratatui draw, headlessly,
    /// and return the glyph grid — so navigation tests can assert on what each
    /// view actually paints (the same path the live UI uses).
    fn drawn(a: &App) -> String {
        crate::view::render_to_string(a, 80, 24)
    }

    #[test]
    fn navigation_drives_every_view_and_renders_it() {
        // Colour-off so the assertions compare glyph content only; this walks the
        // full navigation model and confirms each view renders through view::draw.
        let mut a = app();
        assert_eq!(a.view, View::Default);
        assert!(drawn(&a).contains("NEEDS ATTENTION"));

        a.handle(Key::Enter);
        assert_eq!(a.view, View::Details);
        assert!(drawn(&a).contains("pulse · DETAILS"));

        a.handle(Key::Enter);
        assert_eq!(a.view, View::Default);

        a.handle(Key::Char('f'));
        assert_eq!(a.view, View::Feeds);
        assert!(drawn(&a).contains("pulse · FEEDS"));

        a.handle(Key::Char('/'));
        a.handle(Key::Char('a'));
        a.handle(Key::Char('w'));
        a.handle(Key::Char('s'));
        assert_eq!(a.view, View::Search);
        assert_eq!(a.query, "aws");
        assert!(drawn(&a).contains("/ aws"), "search bar shows the query");
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
        let mut a = app();
        a.status = Some("rexops not found".to_string());
        let frame = drawn(&a);
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

    // Render geometry (width-safety, tiny-size no-panic, status clipping) is now
    // covered against the real ratatui draw in `crate::view`'s tests, not the
    // retired string renderer.
}
