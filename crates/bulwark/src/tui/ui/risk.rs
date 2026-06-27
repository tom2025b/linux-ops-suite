//! Risk-level styling — Bulwark's one UI extension seam on the shared `Theme`.
//!
//! Risk colour is Bulwark-specific (the suite chrome has no concept of it), so
//! it lives here as an extension trait on `suite_ui::Theme` rather than moving
//! into suite-ui — the same pattern ScriptVault uses for syntax highlighting.
//! Crucially it routes through `Theme::color_enabled()`, so risk colours now get
//! the suite's `NO_COLOR` gate they previously lacked: under `NO_COLOR` the hues
//! drop and only attributes (bold for Critical) survive.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::Cell,
};
use suite_ui::Theme;

use crate::app::RiskLevel;

/// Risk-level styling layered onto the shared [`Theme`]. An extension trait
/// (not an inherent method) because `Theme` is owned by `suite-ui` and risk is
/// Bulwark's alone.
pub(super) trait RiskStyle {
    /// The style for a given risk level, honouring `NO_COLOR`. With colour:
    /// green / yellow / red / light-red+bold. Without: no hue, but Critical
    /// keeps bold so the worst level still stands out on a monochrome terminal.
    fn risk(self, level: RiskLevel) -> Style;
}

impl RiskStyle for Theme {
    fn risk(self, level: RiskLevel) -> Style {
        if !self.color_enabled() {
            // NO_COLOR: attributes only. Critical stays bold; the rest are plain
            // (the text — "Low"/"High"/… — carries the meaning without hue).
            return match level {
                RiskLevel::Critical => Style::default().add_modifier(Modifier::BOLD),
                _ => Style::default(),
            };
        }
        match level {
            RiskLevel::Low => Style::default().fg(Color::Green),
            RiskLevel::Medium => Style::default().fg(Color::Yellow),
            RiskLevel::High => Style::default().fg(Color::Red),
            RiskLevel::Critical => Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        }
    }
}

pub(super) fn colored_risk_cell(text: &str, level: RiskLevel, theme: Theme) -> Cell<'static> {
    Cell::from(Span::styled(text.to_string(), theme.risk(level)))
}

pub(super) fn colored_risk_span(text: &str, level: RiskLevel, theme: Theme) -> Span<'static> {
    Span::styled(text.to_string(), theme.risk(level))
}
