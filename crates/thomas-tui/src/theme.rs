//! Theme: the single source of truth for styles, honouring `NO_COLOR`.
//!
//! Resolved once at startup. Every hue/attribute goes through here so
//! `NO_COLOR` produces zero foreground colours (only bold/dim/reverse).

use ratatui::style::{Color, Style, Stylize};

/// The resolved palette for one run of a TUI. Cheap to copy (a flag plus a
/// colour), so callers take it by value.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Whether colour accents are enabled. When `false`, every style drops its
    /// hue and relies on bold/dim only.
    color: bool,
    /// The single accent hue used for emphasis chrome (prompt, titles, the
    /// selection rail, match highlighting, framed borders). Swapping this is
    /// the whole of a colour theme. Ignored when `color` is false.
    accent: Color,
}

/// Which colour theme to use. Only the ACCENT hue changes between themes;
/// neutral chrome (dim text, the selection-row tint) is shared, so a theme is a
/// one-line accent swap.
///
/// Enable the `clap` feature to derive `clap::ValueEnum` (so `--theme
/// cyan|amber` parses straight into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[non_exhaustive]
pub enum ThemeChoice {
    /// The default cool-cyan accent.
    #[default]
    Cyan,
    /// A warm amber/gold accent, for a different feel at the same legibility.
    Amber,
}

impl ThemeChoice {
    /// The accent [`Color`] this theme paints emphasis chrome with.
    fn accent(self) -> Color {
        match self {
            ThemeChoice::Cyan => Color::Cyan,
            // A gruvbox-ish warm gold — distinct from cyan, still high-contrast
            // on both dark and light terminals.
            ThemeChoice::Amber => Color::Rgb(215, 153, 33),
        }
    }
}

/// How colour is decided. `Auto` (the default) honours `NO_COLOR`;
/// `Always`/`Never` are explicit user overrides.
///
/// Enable the `clap` feature to derive `clap::ValueEnum` (so `--color
/// auto|always|never` parses straight into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[non_exhaustive]
pub enum ColorChoice {
    /// Colour on unless `NO_COLOR` is set (the conventional default).
    #[default]
    Auto,
    /// Force colour on, even under `NO_COLOR`.
    Always,
    /// Force colour off (equivalent to setting `NO_COLOR`).
    Never,
}

/// Coarse health status for a monitored thing (an adapter, a producer, a
/// service). Generalized from RexOps so a consumer can map its own status enum
/// to one of these and get a consistent, `NO_COLOR`-safe style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Health {
    Healthy,
    Degraded,
    Unavailable,
    Unknown,
}

/// Coarse severity / risk level for a finding (a flagged script, a risky
/// operation, a review item). The suite's single risk vocabulary, ordered
/// most-severe first so `Critical < High` reads the way the styling escalates.
/// A consumer maps its own grading onto one of these and gets a consistent,
/// `NO_COLOR`-safe style from [`Theme::severity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Severity {
    /// The top level — red + bold. Demands action now.
    Critical,
    /// High risk — yellow + bold.
    High,
    /// Medium risk — plain (no hue), the neutral middle.
    Medium,
    /// Low risk — dim, recedes.
    Low,
}

/// True if `NO_COLOR` is set to a NON-EMPTY value (an empty `NO_COLOR=` is
/// treated as "not set", per the de-facto standard). The single env read for
/// colour.
fn no_color_env() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

impl Theme {
    /// Resolve the theme from the `--color` and `--theme` choices. `ColorChoice`,
    /// layered over the `NO_COLOR` default, decides WHETHER there is any hue
    /// (`Always` forces it on, `Never` off, `Auto` is on unless `NO_COLOR` is
    /// set non-empty). `ThemeChoice` decides WHICH accent hue that is (ignored
    /// when colour is off). One gate for all colour, so the `NO_COLOR` guarantee
    /// and an explicit override meet in exactly one place.
    pub fn resolve(choice: ColorChoice, theme: ThemeChoice) -> Self {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => !no_color_env(),
        };
        Self {
            color,
            accent: theme.accent(),
        }
    }

    /// Build a theme with colour forced on/off, using the default (cyan) accent.
    /// The seam tests and the example gallery use to draw deterministically
    /// regardless of the runner's `NO_COLOR`. Production resolves through
    /// [`resolve`](Self::resolve).
    pub fn with_color(color: bool) -> Self {
        Self {
            color,
            accent: ThemeChoice::Cyan.accent(),
        }
    }

    /// Build a theme with colour and a specific accent forced — for an example
    /// or test that needs amber-with-colour without reading the environment.
    pub fn with(color: bool, theme: ThemeChoice) -> Self {
        Self {
            color,
            accent: theme.accent(),
        }
    }

    /// Whether colour accents are currently enabled.
    pub fn color_enabled(self) -> bool {
        self.color
    }

    /// Apply an accent colour only when colour is enabled; otherwise return the
    /// style untouched. The single primitive every helper below is built on.
    fn accent(self, style: Style, color: Color) -> Style {
        if self.color {
            style.fg(color)
        } else {
            style
        }
    }

    // --- semantic styles the renderer asks for by name ----------------------

    /// The search-bar prompt glyph: accent + bold when colour is on, bold only off.
    pub fn prompt(self) -> Style {
        self.accent(Style::new().bold(), self.accent)
    }

    /// A metadata field's key (e.g. "desc:"). Dimmed always; accent when colour on.
    pub fn meta_key(self) -> Style {
        self.accent(Style::new().dim(), self.accent)
    }

    /// A pane *title* (the border header). Bold always so every pane reads as a
    /// header at a glance; accent hue when colour is on. Bold survives
    /// `NO_COLOR`, so the hierarchy holds without hue.
    pub fn title(self) -> Style {
        self.accent(Style::new().bold(), self.accent)
    }

    /// A "live" activity marker (e.g. a streaming `●`). Green + bold when colour
    /// is on; bold-only under `NO_COLOR` (the glyph itself still distinguishes
    /// live from idle).
    pub fn live_marker(self) -> Style {
        self.accent(Style::new().bold(), Color::Green)
    }

    /// The status line when the message reports a FAILURE. Red + bold when
    /// colour is on; bold-only otherwise (the wording still conveys severity).
    pub fn status_error(self) -> Style {
        self.accent(Style::new().bold(), Color::Red)
    }

    /// The label marking which field a result matched on. Coloured per field
    /// when colour is on; bold + dim otherwise so it still reads as a distinct
    /// annotation without hue.
    pub fn match_label(self, color: Color) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            Style::new().dim()
        }
    }

    /// The highlight style for a selected row. With colour we tint the
    /// background and bold the text; without, we lean on bold + reverse video.
    ///
    /// IMPORTANT: this style deliberately sets NO foreground colour. ratatui
    /// patches it over the whole selected row *including the rail glyph*, and
    /// `Buffer::set_style` only overwrites a cell's fg when the patch's fg is
    /// `Some`. Leaving fg unset is what lets the accent [`Theme::selected_rail`]
    /// survive the whole-row highlight.
    pub fn selection(self) -> Style {
        if self.color {
            Style::new().bold().bg(Color::Rgb(54, 60, 74))
        } else {
            Style::new().bold().reversed()
        }
    }

    /// The left accent rail (`▌`) drawn at the start of the SELECTED row. Accent
    /// hue + bold when colour is on so the selection has a crisp coloured edge;
    /// bold-only under `NO_COLOR`. Relies on [`Theme::selection`] being fg-free.
    pub fn selected_rail(self) -> Style {
        self.accent(Style::new().bold(), self.accent)
    }

    /// Generic dim text (secondary labels, dividers, the status line). Dim is an
    /// attribute, not a colour, so it is identical with or without `NO_COLOR`.
    pub fn dim(self) -> Style {
        Style::new().dim()
    }

    /// Style for a stderr line in an output pane: red when colour is on, plain
    /// otherwise. Under `NO_COLOR` the hue drops, so a caller should ALSO prepend
    /// a small `[err] ` marker to keep stderr distinguishable.
    pub fn stderr(self) -> Style {
        self.accent(Style::new(), Color::Red)
    }

    /// The accent used for emphasis chrome — overlay/modal borders (accent hue
    /// when colour on, bold-only otherwise). The selected *row* uses
    /// `selection()`; this is the shared accent for framed emphasis.
    pub fn accent_bar(self) -> Style {
        self.accent(Style::new().bold(), self.accent)
    }

    /// Style for matched characters inside a result row: bold always, accent hue
    /// when colour is on. Stays legible under `NO_COLOR` (bold survives).
    pub fn match_text(self) -> Style {
        self.accent(Style::new().bold(), self.accent)
    }

    /// Style for a piece of health status. Colour: green+bold (healthy), yellow
    /// (degraded), red (unavailable), dark-gray (unknown). Under `NO_COLOR` the
    /// hue drops but a severity attribute survives (bold for healthy/unavailable,
    /// dim for unknown) so the states stay distinguishable.
    pub fn health(self, health: Health) -> Style {
        match (self.color, health) {
            (true, Health::Healthy) => Style::new().fg(Color::Green).bold(),
            (true, Health::Degraded) => Style::new().fg(Color::Yellow),
            (true, Health::Unavailable) => Style::new().fg(Color::Red),
            (true, Health::Unknown) => Style::new().fg(Color::DarkGray),
            // NO_COLOR: attributes only, never a foreground colour.
            (false, Health::Healthy) => Style::new().bold(),
            (false, Health::Degraded) => Style::new(),
            (false, Health::Unavailable) => Style::new().bold(),
            (false, Health::Unknown) => Style::new().dim(),
            // `Health` is #[non_exhaustive]: a future level renders with a plain
            // (neutral, hue-free) style rather than failing to compile, and
            // never borrows another level's colour. Unreachable today (this crate
            // sees all current variants) but required the moment a variant is
            // added; the allow keeps it from tripping -D warnings until then.
            #[allow(unreachable_patterns)]
            (_, _) => Style::new(),
        }
    }

    /// Style for a severity / risk level. Colour: red+bold (critical),
    /// yellow+bold (high), plain (medium), dim (low). Under `NO_COLOR` the hue
    /// drops but an attribute survives so the levels stay distinguishable: bold
    /// for critical/high, plain for medium, dim for low. Parallels [`health`](Self::health)
    /// — the suite's two colour-coded status axes share one gating shape.
    pub fn severity(self, severity: Severity) -> Style {
        match (self.color, severity) {
            (true, Severity::Critical) => Style::new().fg(Color::Red).bold(),
            (true, Severity::High) => Style::new().fg(Color::Yellow).bold(),
            (true, Severity::Medium) => Style::new(),
            (true, Severity::Low) => Style::new().dim(),
            // NO_COLOR: attributes only, never a foreground colour.
            (false, Severity::Critical) => Style::new().bold(),
            (false, Severity::High) => Style::new().bold(),
            (false, Severity::Medium) => Style::new(),
            (false, Severity::Low) => Style::new().dim(),
            // `Severity` is #[non_exhaustive]: a future level renders plain
            // (neutral, hue-free) rather than failing to compile, and never
            // borrows another level's colour. Unreachable today (this crate sees
            // all current variants) but required once a variant is added; the
            // allow keeps it from tripping -D warnings until then.
            #[allow(unreachable_patterns)]
            (_, _) => Style::new(),
        }
    }

    /// The "refreshing…" / working indicator: yellow when colour is on, dim
    /// otherwise (so the transient state still reads without hue).
    pub fn working(self) -> Style {
        if self.color {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().dim()
        }
    }

    /// Attention style for a pending destructive action (e.g. a confirm modal's
    /// message): bright yellow + bold when colour is on so it's impossible to
    /// miss; bold-only under `NO_COLOR`.
    pub fn confirm(self) -> Style {
        self.accent(Style::new().bold(), Color::Yellow)
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_disabled_strips_foreground() {
        let dark = Theme::with_color(false);
        assert_eq!(dark.prompt().fg, None);
        assert_eq!(dark.meta_key().fg, None);
        assert_eq!(dark.match_label(Color::Green).fg, None);
        // Selection must not rely on a background colour when colour is off.
        assert_eq!(dark.selection().bg, None);
    }

    #[test]
    fn color_enabled_applies_foreground() {
        let lit = Theme::with_color(true);
        assert_eq!(lit.prompt().fg, Some(Color::Cyan));
        assert_eq!(lit.match_label(Color::Green).fg, Some(Color::Green));
        assert!(lit.selection().bg.is_some());
    }

    #[test]
    fn accent_bar_present_only_with_color() {
        assert!(Theme::with_color(true).accent_bar().fg.is_some());
        assert_eq!(Theme::with_color(false).accent_bar().fg, None);
    }

    #[test]
    fn resolve_respects_explicit_choice_over_env() {
        // Always → colour on; Never → colour off, ignoring NO_COLOR by design.
        assert!(
            Theme::resolve(ColorChoice::Always, ThemeChoice::Cyan)
                .prompt()
                .fg
                .is_some(),
            "--color=always must force colour on"
        );
        assert_eq!(
            Theme::resolve(ColorChoice::Never, ThemeChoice::Cyan)
                .prompt()
                .fg,
            None,
            "--color=never must force colour off"
        );
    }

    #[test]
    fn theme_choice_swaps_the_accent_hue_only_when_colour_on() {
        let amber = Theme::resolve(ColorChoice::Always, ThemeChoice::Amber);
        let cyan = Theme::resolve(ColorChoice::Always, ThemeChoice::Cyan);
        assert_eq!(cyan.prompt().fg, Some(Color::Cyan));
        assert_eq!(amber.prompt().fg, Some(Color::Rgb(215, 153, 33)));
        // The accent threads through every emphasis helper, not just the prompt.
        assert_eq!(amber.title().fg, Some(Color::Rgb(215, 153, 33)));
        assert_eq!(amber.accent_bar().fg, Some(Color::Rgb(215, 153, 33)));
        assert_eq!(amber.match_text().fg, Some(Color::Rgb(215, 153, 33)));
        assert_eq!(amber.selected_rail().fg, Some(Color::Rgb(215, 153, 33)));
        // NO_COLOR wins over the theme: no accent survives regardless of choice.
        assert_eq!(
            Theme::resolve(ColorChoice::Never, ThemeChoice::Amber)
                .prompt()
                .fg,
            None
        );
    }

    #[test]
    fn dim_is_colorless_in_both_modes() {
        for theme in [Theme::with_color(true), Theme::with_color(false)] {
            assert_eq!(theme.dim().fg, None);
        }
    }

    #[test]
    fn health_is_coloured_on_and_colourless_off() {
        let lit = Theme::with_color(true);
        assert_eq!(lit.health(Health::Healthy).fg, Some(Color::Green));
        assert_eq!(lit.health(Health::Degraded).fg, Some(Color::Yellow));
        assert_eq!(lit.health(Health::Unavailable).fg, Some(Color::Red));
        assert_eq!(lit.health(Health::Unknown).fg, Some(Color::DarkGray));

        let dark = Theme::with_color(false);
        for h in [
            Health::Healthy,
            Health::Degraded,
            Health::Unavailable,
            Health::Unknown,
        ] {
            assert_eq!(
                dark.health(h).fg,
                None,
                "{h:?} must have no fg under NO_COLOR"
            );
        }
        // Severity still distinguishable without hue: healthy bold, unknown dim.
        assert!(dark
            .health(Health::Healthy)
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));
        assert!(dark
            .health(Health::Unknown)
            .add_modifier
            .contains(ratatui::style::Modifier::DIM));
    }

    #[test]
    fn severity_is_coloured_on_and_colourless_off() {
        use ratatui::style::Modifier;
        let lit = Theme::with_color(true);
        assert_eq!(lit.severity(Severity::Critical).fg, Some(Color::Red));
        assert_eq!(lit.severity(Severity::High).fg, Some(Color::Yellow));
        assert_eq!(lit.severity(Severity::Medium).fg, None, "medium is neutral");
        assert_eq!(lit.severity(Severity::Low).fg, None, "low is dim, no hue");

        let dark = Theme::with_color(false);
        for s in [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
        ] {
            assert_eq!(
                dark.severity(s).fg,
                None,
                "{s:?} must have no fg under NO_COLOR"
            );
        }
        // Severity still distinguishable without hue: critical/high bold, low dim.
        assert!(dark
            .severity(Severity::Critical)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(dark
            .severity(Severity::High)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(dark
            .severity(Severity::Low)
            .add_modifier
            .contains(Modifier::DIM));
        // Medium is the neutral middle — neither bold nor dim, in either mode.
        for theme in [lit, dark] {
            let med = theme.severity(Severity::Medium).add_modifier;
            assert!(!med.contains(Modifier::BOLD));
            assert!(!med.contains(Modifier::DIM));
        }
    }

    #[test]
    fn working_and_confirm_follow_the_colour_gate() {
        assert_eq!(Theme::with_color(true).working().fg, Some(Color::Yellow));
        assert_eq!(Theme::with_color(false).working().fg, None);
        assert_eq!(Theme::with_color(true).confirm().fg, Some(Color::Yellow));
        assert_eq!(Theme::with_color(false).confirm().fg, None);
    }
}
