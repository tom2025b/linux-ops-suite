//! PaletteFrame: the command-palette *chrome* — input row, list, footer.
//!
//! This draws the box only. Filtering the items as the user types, moving the
//! selection, and dispatching the chosen command are the consuming app's job —
//! they involve the app's commands and effects and are deliberately NOT shared.
//! The caller filters its own command list, tracks the selected index, and
//! hands the already-filtered slice here to render.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::centered_rect;
use crate::theme::Theme;

/// One selectable row in the palette: a short label and a longer description.
pub struct PaletteItem<'a> {
    /// The command name (left column).
    pub label: &'a str,
    /// A one-line description (right column).
    pub desc: &'a str,
}

/// The command-palette overlay chrome. The caller supplies the current query
/// text, the already-filtered items, and which one is selected.
///
/// ```no_run
/// # use thomas_tui::{PaletteFrame, PaletteItem, Theme};
/// # use ratatui::Frame;
/// # fn draw(frame: &mut Frame, theme: Theme) {
/// let items = [
///     PaletteItem { label: "reload", desc: "rescan scripts" },
///     PaletteItem { label: "help", desc: "show keybindings" },
/// ];
/// PaletteFrame { query: "re", items: &items, selected: Some(0) }
///     .render(frame, frame.area(), theme);
/// # }
/// ```
pub struct PaletteFrame<'a> {
    /// The current filter text the user has typed.
    pub query: &'a str,
    /// The items to show, already filtered by the caller, in display order.
    pub items: &'a [PaletteItem<'a>],
    /// The selected index into `items`, if any.
    pub selected: Option<usize>,
}

impl PaletteFrame<'_> {
    /// How many rows of items to show before truncating.
    const MAX_ROWS: usize = 12;
    /// Width of the label column (descriptions start after this).
    const LABEL_W: usize = 14;

    /// Draw the palette over `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        let area = centered_rect(55, 55, area);
        let selected = self.selected.unwrap_or(0);

        let mut lines: Vec<Line> = Vec::with_capacity(self.items.len().min(Self::MAX_ROWS) + 2);

        // Input row: a `>` prompt, the live query, and a block cursor.
        lines.push(Line::from(vec![
            Span::styled(" > ", theme.prompt()),
            Span::raw(self.query.to_string()),
            Span::styled("█", theme.dim()),
        ]));
        lines.push(Line::from(Span::styled("— commands —", theme.dim())));

        for (i, item) in self.items.iter().enumerate().take(Self::MAX_ROWS) {
            let is_sel = i == selected;
            let prefix = if is_sel { "› " } else { "  " };
            let style = if is_sel {
                theme.selection()
            } else {
                Style::new()
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{:<w$}", item.label, w = Self::LABEL_W), style),
                Span::raw(format!("  {}", item.desc)),
            ]));
        }
        if self.items.is_empty() {
            lines.push(Line::from(Span::styled("  (no match)", theme.dim())));
        }

        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.accent_bar())
            .padding(Padding::uniform(1))
            .title(" Command Palette (^P / :) ");

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
    }
}
