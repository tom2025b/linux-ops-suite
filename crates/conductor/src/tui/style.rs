//! Conductor's local high-contrast palette — a thin readability layer over the
//! shared `suite_ui::Theme`.
//!
//! Bad-vision readability is the goal here, so this deliberately diverges from
//! the suite default in ONE direction: nothing is ever dim/grey. The shared
//! `Theme::dim()` (used for commands, hints, the situation) is the main thing
//! that hurts on weak vision; this palette replaces those roles with bright,
//! bold, full-strength styles. When high contrast is OFF the caller falls back
//! to the plain `Theme` styles, so this is a pure additive toggle.
//!
//! Colour-free safety is preserved: every style still carries a bold/attribute
//! so the hierarchy survives `NO_COLOR` (no role relies on hue alone).

use ratatui::style::{Color, Modifier, Style};

/// The resolved Conductor palette. `high_contrast` picks bright+bold styles;
/// otherwise the renderer uses the shared `Theme` directly. Cheap to copy.
#[derive(Clone, Copy)]
pub struct Palette {
    on: bool,
}

impl Palette {
    /// Build the palette. `on` = high-contrast mode (bright/bold, no grey).
    pub fn new(high_contrast: bool) -> Self {
        Palette { on: high_contrast }
    }

    /// Whether high-contrast styling is active (the renderer branches on this to
    /// fall back to the plain shared theme when off).
    pub fn active(self) -> bool {
        self.on
    }

    /// Body text — what was plain/dim becomes bright white + bold. The single
    /// biggest legibility win (commands, situation lines, hints).
    pub fn text(self) -> Style {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    }

    /// A pane title / header. Bright cyan + bold so every box header pops.
    pub fn title(self) -> Style {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    /// The accent used for borders + the selection rail: bright cyan, bold.
    pub fn accent(self) -> Style {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    /// The focused row's highlight: a strong reverse-video bar (works on any
    /// background and is unmistakable), bold.
    pub fn selection(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    }

    /// A "changes state" / caution tag — bright yellow + bold (a warning hue,
    /// not red, since it's a heads-up, not a failure).
    pub fn caution(self) -> Style {
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    /// A failure / error line — bright red + bold.
    pub fn error(self) -> Style {
        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    /// A done/success accent — bright green + bold.
    pub fn ok(self) -> Style {
        Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_contrast_text_is_never_dim() {
        let p = Palette::new(true);
        // The whole point: text/title/accent carry bold and a bright fg, never
        // the DIM modifier that hurts weak vision.
        for s in [
            p.text(),
            p.title(),
            p.accent(),
            p.caution(),
            p.error(),
            p.ok(),
        ] {
            assert!(
                !s.add_modifier.contains(Modifier::DIM),
                "high-contrast styles must never be dim: {s:?}"
            );
            assert!(
                s.add_modifier.contains(Modifier::BOLD),
                "high-contrast styles must be bold: {s:?}"
            );
        }
    }

    #[test]
    fn text_is_bright_white() {
        assert_eq!(Palette::new(true).text().fg, Some(Color::White));
    }

    #[test]
    fn active_reflects_the_flag() {
        assert!(Palette::new(true).active());
        assert!(!Palette::new(false).active());
    }
}
