// tui/theme.rs — ScriptVault's view onto the suite's shared theme.
// -----------------------------------------------------------------------------
// The palette, the NO_COLOR gate, and the cyan/amber accent all live in the
// shared `suite-ui` crate now — its `Theme` is the single source of truth for
// the whole suite. This module re-exports those types so the rest of the TUI
// keeps importing `super::theme::{Theme, ThemeChoice, ColorChoice}` unchanged,
// and adds the ONE thing that is genuinely ScriptVault-specific: syntax-
// highlight styling, which colour the shared chrome has no reason to know about.

use ratatui::style::{Color, Style, Stylize};

pub use suite_ui::{ColorChoice, Theme, ThemeChoice};

/// Coarse syntax categories the preview highlighter emits. Frontend-defined (NOT
/// in core, NOT in the shared chrome) so colour stays a ScriptVault UI concern.
/// The highlighter classifies each token into one of these, then asks for the
/// style via [`SyntaxStyle::syntax`] — which routes syntax colour through the
/// same `NO_COLOR` gate the shared `Theme` enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxKind {
    Keyword,
    StringLit,
    Comment,
    Number,
    Normal,
}

/// Syntax-highlight styling layered onto the shared [`Theme`]. An extension
/// trait (rather than an inherent method) because `Theme` is owned by `suite-ui`
/// and highlighting is ours alone. It reads the theme's public colour gate, so
/// the `NO_COLOR` guarantee still meets in exactly one place.
pub trait SyntaxStyle {
    /// Map a syntax category to a Style, honouring `NO_COLOR`. With colour on,
    /// each kind gets a hue; with colour OFF, only attributes survive (comments
    /// dim, keywords bold) so the preview stays legible AND colourless.
    fn syntax(self, kind: SyntaxKind) -> Style;
}

impl SyntaxStyle for Theme {
    fn syntax(self, kind: SyntaxKind) -> Style {
        match (self.color_enabled(), kind) {
            (true, SyntaxKind::Keyword) => Style::new().fg(Color::Magenta),
            (true, SyntaxKind::StringLit) => Style::new().fg(Color::Green),
            (true, SyntaxKind::Comment) => Style::new().fg(Color::DarkGray),
            (true, SyntaxKind::Number) => Style::new().fg(Color::Yellow),
            (true, SyntaxKind::Normal) => Style::new(),
            // NO_COLOR: attributes only, never a foreground colour.
            (false, SyntaxKind::Keyword) => Style::new().bold(),
            (false, SyntaxKind::Comment) => Style::new().dim(),
            (false, _) => Style::new(),
        }
    }
}

// ============================================================================
// Tests — only the ScriptVault-specific syntax styling. The shared Theme's
// behaviour (NO_COLOR gate, accent swap, resolve) is tested in suite-ui.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_has_no_fg_color_under_no_color() {
        let mono = Theme::with_color(false);
        for k in [
            SyntaxKind::Keyword,
            SyntaxKind::StringLit,
            SyntaxKind::Comment,
            SyntaxKind::Number,
            SyntaxKind::Normal,
        ] {
            assert_eq!(
                mono.syntax(k).fg,
                None,
                "{k:?} must have no fg under NO_COLOR"
            );
        }
        // With colour, at least keyword is coloured (so the assertion is meaningful).
        assert!(
            Theme::with_color(true)
                .syntax(SyntaxKind::Keyword)
                .fg
                .is_some()
        );
    }
}

// Syntax styling is the only piece of palette ScriptVault keeps; everything else
// is the shared suite-ui Theme. See ARCHITECTURE.md.
