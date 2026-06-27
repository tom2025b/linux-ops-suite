// tui/ui/overlays/widgets.rs — shared building blocks for the modal overlays.
// -----------------------------------------------------------------------------
// Every overlay in this crate draws the SAME rounded-accent-padded modal frame
// (cleared first so it floats over the list) and most are list pickers that
// repeat the same skeleton: a title line, a dim hint line, a blank spacer, then
// rows with a `› `/`  ` selection prefix, falling back to a dim empty message.
// Both shapes live here once so each overlay is "build rows -> render", not a
// re-typed block, and adding a picker no longer means copying six lines of frame
// boilerplate. These are data-in/draw-out helpers (a Theme, borrowed data, a
// Rect — the same contract suite-ui widgets use); they own no App state.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::tui::theme::Theme;

/// The one modal frame every overlay uses: a rounded block in the accent colour,
/// uniformly padded, with the given title. Factored out so a restyle (border
/// type, padding, accent) lands on every overlay at once instead of six copies.
pub(super) fn modal_frame(title: &str, theme: &Theme) -> Block<'static> {
    modal_frame_with(title, theme, Padding::uniform(1))
}

/// The same rounded-accent frame but with `horizontal(1)` padding only — for the
/// compact, fixed-height action menu, where uniform padding would add top/bottom
/// rows and clip the hint line out of the 8-row box.
pub(super) fn compact_modal_frame(title: &str, theme: &Theme) -> Block<'static> {
    modal_frame_with(title, theme, Padding::horizontal(1))
}

/// Shared frame builder behind [`modal_frame`] / [`compact_modal_frame`]: the
/// border, accent, and titling are identical; only the padding differs.
fn modal_frame_with(title: &str, theme: &Theme, padding: Padding) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.accent_bar())
        .padding(padding)
        .title(format!(" {} ", title.trim()))
}

/// The shared `› ` (selected) / `  ` (not) row prefix and its style. Centralised
/// so every overlay's selection marker is identical — the list pickers get it via
/// [`render_picker`]; the action menu, which builds its own lines, calls this
/// directly.
pub(super) fn selection_prefix(selected: bool, theme: &Theme) -> (&'static str, Style) {
    if selected {
        ("› ", theme.selection())
    } else {
        ("  ", Style::new())
    }
}

/// One row in a list picker: a label styled by selection state, plus optional dim
/// trailing text. The selection prefix (`› ` / `  `) is added by [`render_picker`],
/// NOT here, so the "which row is selected" marker stays in one place. (The action
/// menu doesn't use this — it has an accent digit + red destructive label and a
/// bold subtitle, so it builds its own lines and only shares [`selection_prefix`].)
pub(super) struct PickerRow {
    /// The content spans for this row, AFTER the selection prefix.
    pub spans: Vec<Span<'static>>,
    /// Whether this row is the highlighted one (gets the `› ` prefix + the
    /// selection-styled label).
    pub selected: bool,
}

impl PickerRow {
    /// A label styled by selection state, plus optional dim trailing text (e.g.
    /// " (3 scripts)" or "  (query)"). `selected` drives both the prefix (added by
    /// [`render_picker`]) and the label style here.
    pub fn labeled(
        label: impl Into<String>,
        trailing: Option<String>,
        selected: bool,
        theme: &Theme,
    ) -> Self {
        let label_style = if selected {
            theme.selection()
        } else {
            Style::new()
        };
        let mut spans = vec![Span::styled(label.into(), label_style)];
        if let Some(t) = trailing {
            spans.push(Span::styled(t, theme.dim()));
        }
        Self { spans, selected }
    }
}

/// A list-picker overlay: a title, a dim hint line, a blank spacer, the rows
/// (each with the shared selection prefix), and a dim fallback when empty. This
/// is the skeleton the playlist / saved-search / command palette / action-menu
/// overlays all share; they differ only in their title, hint, and rows.
pub(super) struct PickerSpec<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    pub rows: Vec<PickerRow>,
    /// Shown (dim) when `rows` is empty, e.g. "(no playlists)".
    pub empty_msg: &'a str,
}

/// Render a [`PickerSpec`] into `area`: clear the region, build the lines (title,
/// hint, spacer, rows with `› `/`  ` prefixes, or the empty message), and draw
/// them inside the shared [`modal_frame`]. The single place the picker skeleton
/// lives, so every list overlay reads identically and a layout tweak is one edit.
pub(super) fn render_picker(frame: &mut Frame, area: Rect, theme: &Theme, spec: PickerSpec) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(spec.title.to_string(), theme.prompt())),
        Line::from(Span::styled(spec.hint.to_string(), theme.dim())),
        Line::from(Span::raw("")),
    ];

    if spec.rows.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", spec.empty_msg),
            theme.dim(),
        )));
    } else {
        for row in spec.rows {
            // The selection prefix is owned HERE (not in PickerRow) so the
            // `› `/`  ` convention is identical across every picker.
            let prefix = if row.selected { "› " } else { "  " };
            let prefix_style = if row.selected {
                theme.selection()
            } else {
                Style::new()
            };
            let mut spans = vec![Span::styled(prefix, prefix_style)];
            spans.extend(row.spans);
            lines.push(Line::from(spans));
        }
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(modal_frame(spec.title, theme)),
        area,
    );
}
