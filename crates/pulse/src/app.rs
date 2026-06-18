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

use crate::tui::{self, Key, RawMode};
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
}

impl App {
    pub fn new(readings: Readings) -> Self {
        let verdict = Verdict::from_readings(&readings);
        App { readings, verdict, view: View::Default, query: String::new(), quit: false }
    }

    /// Run the interactive loop until the user quits. Owns raw mode for its
    /// duration; the terminal is restored when `_raw` drops (and by the panic
    /// hook installed here). `input` is stdin in production; tests drive it with
    /// a byte cursor via [`App::handle`] instead of calling `run`.
    pub fn run(mut self, style: &Style) -> io::Result<()> {
        tui::install_panic_guard();
        let _raw = RawMode::enter()?;
        let mut stdin = io::stdin();
        loop {
            let size = TermSize::resolve();
            tui::paint(&self.frame(style, size))?;
            if self.quit {
                break;
            }
            let key = tui::read_key(&mut stdin)?;
            self.handle(key);
            if self.quit {
                // Repaint nothing more; just restore and leave.
                break;
            }
        }
        Ok(())
    }

    /// Render a single named view to a frame, without the event loop. Used by
    /// `--dump-view` to preview/snapshot a view deterministically (no PTY timing
    /// games), and handy in tests. Unknown names return `None`.
    pub fn dump(&mut self, view: &str, query: &str, style: &Style, size: TermSize) -> Option<String> {
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
        // The search box captures most keys while it's open.
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

        match key {
            Key::Char('q') | Key::Eof => self.quit = true,
            // Esc and Enter-from-a-view return to the default screen; from the
            // default screen Enter opens Details.
            Key::Esc => self.view = View::Default,
            Key::Enter => {
                self.view = if self.view == View::Default { View::Details } else { View::Default };
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
        if self.view == target { View::Default } else { target }
    }

    /// Render the current view to a full frame for `size`.
    fn frame(&self, style: &Style, size: TermSize) -> String {
        match self.view {
            View::Default => render(&self.verdict, style, size),
            View::Attention => self.view_attention(style, size),
            View::Feeds => self.view_feeds(style, size),
            View::Details => self.view_details(style, size),
            View::Help => self.view_help(style, size),
            View::Search => self.view_search(style, size),
        }
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
        panel(style, size, "ATTENTION", &body, "a / Esc  back      q  quit")
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
        body.push(format!("{dim}{age}{rst}", dim = style.dim, age = age, rst = style.rst));
        panel(style, size, "FEEDS", &body, "f / Esc  back      q  quit")
    }

    fn view_details(&self, style: &Style, size: TermSize) -> String {
        let v = &self.verdict;
        let mut body = vec![
            format!("  verdict   {}", crate::verdict_text(v.state)),
            format!("  data age  {}", v.age),
        ];
        if v.critical + v.high > 0 {
            body.push(format!("  findings  {} critical, {} high", v.critical, v.high));
        }
        body.push(String::new());
        body.push("  press a for the full attention list, f for feeds.".to_string());
        panel(style, size, "DETAILS", &body, "Enter / Esc  back      q  quit")
    }

    fn view_help(&self, style: &Style, size: TermSize) -> String {
        let body = vec![
            "  Enter   details for the current verdict".to_string(),
            "  a       attention — everything that needs action".to_string(),
            "  f       feeds — source freshness & confidence".to_string(),
            "  /       search across visible status".to_string(),
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
            body.push(format!("  {}type to filter; Enter or Esc to close.{}", style.dim, style.rst));
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
        panel(style, size, "SEARCH", &body, "Enter / Esc  close      type to filter")
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
    let w = size.width.max(20) as usize;
    let mut lines: Vec<String> = vec![String::new(); h];

    lines[0] = format!(" {dim}pulse · {title}{rst}", dim = style.dim, title = title, rst = style.rst);

    // Body starts two rows down, clipped to leave room for the footer.
    let top = 2;
    for (i, line) in body.iter().enumerate() {
        let row = top + i;
        if row >= h - 2 {
            break;
        }
        lines[row] = line.clone();
    }

    lines[h - 2] = format!(" {dim}{}{rst}", "─".repeat(w.saturating_sub(2)), dim = style.dim, rst = style.rst);
    lines[h - 1] = format!(" {dim}{footer}{rst}", dim = style.dim, footer = footer, rst = style.rst);
    lines.join("\n")
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
            bulwark: BulwarkView { attention: Vec::new(), present: true },
            jobs: Vec::new(),
            binaries: ["workstate", "bulwark", "proto", "toolfoundry", "vault"]
                .iter()
                .map(|&name| BinaryCheck { name, present: true })
                .collect(),
            now: Some(0),
        }
    }

    fn app() -> App {
        App::new(sample_readings())
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
        for v in [View::Attention, View::Feeds, View::Details, View::Help, View::Search] {
            let mut a = app();
            a.view = v;
            for (w, h) in [(80u16, 24u16), (20, 6), (200, 60)] {
                let frame = a.frame(&style, TermSize::for_test(w, h));
                assert!(!frame.is_empty());
            }
        }
    }
}
