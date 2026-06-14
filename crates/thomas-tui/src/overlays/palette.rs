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
use crate::text::truncate_desc;
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
    /// The selected index into `items`. `Some(i)` highlights row `i`; `None`
    /// highlights no row (an empty list, or a query with the selection cleared).
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

        // The content width inside the chrome: the framed area minus the border
        // (1 cell each side) and the uniform padding (1 cell each side). Each row
        // is `prefix(2) + label(LABEL_W) + gap(2) + desc`, so the description gets
        // whatever is left — truncated to it (with the shared `…`) rather than
        // letting ratatui chop it wordlessly at the border.
        let inner_w = area.width.saturating_sub(4) as usize; // 2 border + 2 padding
        let desc_w = inner_w.saturating_sub(2 + Self::LABEL_W + 2);

        let mut lines: Vec<Line> = Vec::with_capacity(self.items.len().min(Self::MAX_ROWS) + 2);

        // Input row: a `>` prompt, the live query, and a block cursor.
        lines.push(Line::from(vec![
            Span::styled(" > ", theme.prompt()),
            Span::raw(self.query.to_string()),
            Span::styled("█", theme.dim()),
        ]));
        lines.push(Line::from(Span::styled("— commands —", theme.dim())));

        for (i, item) in self.items.iter().enumerate().take(Self::MAX_ROWS) {
            // Only the row at `selected` is highlighted; `None` highlights none.
            let is_sel = self.selected == Some(i);
            let prefix = if is_sel { "› " } else { "  " };
            let style = if is_sel {
                theme.selection()
            } else {
                Style::new()
            };
            // Truncate the label to its column so an over-long one can't shove the
            // description out of alignment, and the description to the space left.
            let label = truncate_desc(item.label, Self::LABEL_W);
            let desc = truncate_desc(item.desc, desc_w);
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{label:<w$}", w = Self::LABEL_W), style),
                Span::raw(format!("  {desc}")),
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

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use ratatui::Terminal;

    /// Draw the palette into a large test backend (so its percentage-sized area is
    /// wide), returning the terminal for both text and per-cell style assertions.
    fn draw(
        width: u16,
        height: u16,
        query: &str,
        items: &[PaletteItem],
        selected: Option<usize>,
    ) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        let theme = Theme::with_color(false);
        term.draw(|f| {
            PaletteFrame {
                query,
                items,
                selected,
            }
            .render(f, f.area(), theme);
        })
        .unwrap();
        term
    }

    /// The whole buffer flattened to one newline-free string.
    fn flat(term: &Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    /// True if ANY cell in the buffer carries the REVERSED modifier — the
    /// NO_COLOR selection style (`bold().reversed()`). Used to assert whether a
    /// row is highlighted without depending on its exact position.
    fn any_reversed(term: &Terminal<TestBackend>) -> bool {
        term.backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.style().add_modifier.contains(Modifier::REVERSED))
    }

    #[test]
    fn shows_the_query_and_each_item_label() {
        let items = [
            PaletteItem {
                label: "reload",
                desc: "rescan",
            },
            PaletteItem {
                label: "help",
                desc: "keybindings",
            },
        ];
        let term = draw(80, 16, "re", &items, Some(0));
        let out = flat(&term);
        assert!(out.contains("re"), "the query is echoed in the input row");
        assert!(out.contains("reload"));
        assert!(out.contains("help"));
    }

    #[test]
    fn a_long_description_is_truncated_with_an_ellipsis_not_chopped() {
        // The bug this guards: descriptions were appended raw and chopped at the
        // border with no marker. They must now end in the shared `…`.
        let items = [PaletteItem {
            label: "reload",
            desc: "rescan all the scripts incrementally and then reindex everything thoroughly",
        }];
        let term = draw(60, 16, "", &items, Some(0));
        let out = flat(&term);
        assert!(
            out.contains('…'),
            "an overflowing description must be marked with an ellipsis:\n{out}"
        );
        // And the raw, untruncated tail must NOT appear in full.
        assert!(
            !out.contains("reindex everything thoroughly"),
            "the description must actually be cut, not just decorated:\n{out}"
        );
    }

    #[test]
    fn a_long_label_is_truncated_to_its_column() {
        // An over-long label must be cut (to LABEL_W) so it can't shove the
        // description column out of alignment.
        let items = [PaletteItem {
            label: "an-absurdly-long-command-name-that-overflows",
            desc: "do a thing",
        }];
        let term = draw(80, 16, "", &items, Some(0));
        let out = flat(&term);
        assert!(out.contains('…'), "the long label is ellipsised:\n{out}");
        assert!(
            !out.contains("an-absurdly-long-command-name-that-overflows"),
            "the full over-long label must not appear:\n{out}"
        );
    }

    #[test]
    fn selected_some_highlights_a_row_and_none_highlights_nothing() {
        let items = [
            PaletteItem {
                label: "a",
                desc: "first",
            },
            PaletteItem {
                label: "b",
                desc: "second",
            },
        ];
        // With a selection, exactly one row is drawn in the reversed style.
        let with_sel = draw(80, 16, "", &items, Some(1));
        assert!(
            any_reversed(&with_sel),
            "Some(i) must highlight the selected row"
        );
        // With None, NO row is highlighted (the old unwrap_or(0) wrongly lit row 0).
        let no_sel = draw(80, 16, "", &items, None);
        assert!(!any_reversed(&no_sel), "None must highlight no row at all");
    }

    #[test]
    fn empty_items_show_the_no_match_placeholder() {
        let term = draw(80, 16, "zzz", &[], None);
        assert!(flat(&term).contains("(no match)"), "empty list says so");
    }

    #[test]
    fn items_past_max_rows_are_truncated_and_no_match_is_not_shown() {
        // More items than MAX_ROWS: only the first MAX_ROWS render, the rest are
        // dropped, and the "(no match)" placeholder must NOT appear (the list is
        // non-empty, just truncated).
        let labels: Vec<String> = (0..15).map(|i| format!("cmd{i:02}")).collect();
        let items: Vec<PaletteItem> = labels
            .iter()
            .map(|l| PaletteItem {
                label: l,
                desc: "x",
            })
            .collect();
        // Tall backend so MAX_ROWS (not the centered area's height) is what limits
        // the list: the area is centered_rect(.., 55, ..), and after border +
        // padding + the input/header rows it must still leave room for 12 items, so
        // give it generous height.
        let term = draw(80, 40, "", &items, None);
        let out = flat(&term);
        // The first row and the last row that fits (index MAX_ROWS-1 = 11) show.
        assert!(out.contains("cmd00"), "first item shows");
        assert!(
            out.contains("cmd11"),
            "the 12th item (last within MAX_ROWS) shows"
        );
        // The 13th item onward is truncated away.
        assert!(
            !out.contains("cmd12"),
            "items past MAX_ROWS must be dropped:\n{out}"
        );
        assert!(!out.contains("cmd14"), "and so is the very last one");
        assert!(
            !out.contains("(no match)"),
            "a truncated (but non-empty) list must not claim no match:\n{out}"
        );
    }

    #[test]
    fn an_out_of_range_selection_highlights_nothing_and_does_not_panic() {
        // `selected` is a caller-supplied index; a stale/oversized one (e.g. the
        // list shrank after a keystroke) must simply highlight no row rather than
        // panic or light the wrong one.
        let items = [
            PaletteItem {
                label: "a",
                desc: "first",
            },
            PaletteItem {
                label: "b",
                desc: "second",
            },
        ];
        let term = draw(80, 16, "", &items, Some(99));
        assert!(
            !any_reversed(&term),
            "an index past the end highlights no row"
        );
        // Both items still render normally.
        let out = flat(&term);
        assert!(out.contains('a') && out.contains('b'));
    }
}
