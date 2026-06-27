use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, BorderType, Padding, Paragraph, Wrap};

use crate::tui::app::{App, OutputStream};

use super::layout::pane;

/// Render the tailing output pane for live/captured runs.
pub(super) fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let block = if app.live_active() {
        let title = Title::from(Line::from(vec![
            Span::styled(" output ", theme.title()),
            Span::styled("●", theme.live_marker()),
            Span::styled(" live ", theme.title()),
        ]));
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.dim())
            .padding(Padding::horizontal(1))
            .title(title)
    } else if app.output_lines().is_empty() {
        pane("output (empty — run live or capture)", theme)
    } else {
        pane("output (last run)", theme)
    };

    let lines = app.output_lines();
    if lines.is_empty() {
        let hint =
            Paragraph::new("use palette 'run live' or 'run capture'\nor ^L to hide this pane")
                .block(block)
                .style(theme.dim());
        frame.render_widget(hint, area);
        return;
    }

    let inner_h = area.height.saturating_sub(2) as usize;
    let tail_start = lines.len().saturating_sub(inner_h);
    let start = tail_start.saturating_sub(app.output_scroll().min(tail_start));
    let end = (start + inner_h).min(lines.len());
    let visible: Vec<Line> = lines[start..end]
        .iter()
        .map(|l| match l.stream {
            OutputStream::Stdout => Line::from(Span::styled(l.text.clone(), theme.dim())),
            OutputStream::Stderr => {
                Line::from(Span::styled(format!("[err] {}", l.text), theme.stderr()))
            }
        })
        .collect();

    let para = Paragraph::new(Text::from(visible))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}
