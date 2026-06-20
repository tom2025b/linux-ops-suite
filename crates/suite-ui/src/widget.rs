//! The suite-flavoured line widgets opt into the [`Themed`] `ratatui::Widget`
//! surface by implementing [`ThemedLine`] (both from thomas-tui). The blanket
//! `impl Widget for &Themed<W> where W: ThemedLine` in thomas-tui then gives them
//! `frame.render_widget(&w.themed(theme), area)` for free — implementing the local
//! `ThemedLine` trait on our own types is orphan-rule-safe, whereas a direct
//! `impl Widget for &Themed<OurWidget>` here would not be.

use ratatui::text::Line;
use thomas_tui::{Theme, ThemedLine};

use crate::{AttentionFlag, HealthStrip, StatusBar};

impl ThemedLine for StatusBar<'_> {
    fn themed_line(&self, theme: Theme) -> Line<'static> {
        self.line(theme)
    }
}
impl ThemedLine for AttentionFlag<'_> {
    fn themed_line(&self, theme: Theme) -> Line<'static> {
        self.line(theme)
    }
}
impl ThemedLine for HealthStrip<'_> {
    fn themed_line(&self, theme: Theme) -> Line<'static> {
        self.line(theme)
    }
}

#[cfg(test)]
mod tests {
    use crate::{JobState, StatusBar, Theme};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use thomas_tui::Themable;

    #[test]
    fn themed_status_bar_renders_via_widget_trait() {
        let theme = Theme::with_color(false);
        let bar = StatusBar {
            job: JobState::Running { name: "backup" },
        };
        let mut term = Terminal::new(TestBackend::new(30, 1)).unwrap();
        term.draw(|f| f.render_widget(&bar.themed(theme), f.area()))
            .unwrap();
        let row: String = (0..30)
            .map(|x| {
                term.backend()
                    .buffer()
                    .cell((x, 0))
                    .unwrap()
                    .symbol()
                    .to_string()
            })
            .collect();
        assert!(row.contains("running backup"), "got {row:?}");
    }
}
