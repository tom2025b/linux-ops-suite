//! Color resolver for the interactive TUI. Same discipline as report::Style and
//! pulse: color on iff stdout is a TTY and NO_COLOR is unset (or forced off);
//! every field is an empty string when off, so call sites interpolate
//! unconditionally and the frame reads identically with color stripped. State is
//! always carried by word and glyph — color is only ever a bonus.

use crate::plan::Ring;
use crate::state::Severity;
use crate::util;

/// Resolved ANSI styling. Empty strings when color is off.
pub struct Style {
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub grn: &'static str,
    pub ylw: &'static str,
    pub cyn: &'static str,
    pub rst: &'static str,
}

impl Style {
    /// Resolve styling. `force_off` (e.g. `--no-color`) wins; otherwise color is
    /// on only for a real TTY without `NO_COLOR`.
    pub fn resolve(force_off: bool) -> Self {
        let on = !force_off && util::stdout_is_tty() && std::env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                red: "\u{1b}[31m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Style {
                bold: "",
                dim: "",
                red: "",
                grn: "",
                ylw: "",
                cyn: "",
                rst: "",
            }
        }
    }

    /// Amber for a state-changing ring; dim for read-only/info.
    pub fn ring_color(&self, ring: Ring) -> &'static str {
        match ring {
            Ring::ChangesState => self.ylw,
            Ring::ReadOnly | Ring::Info => self.dim,
        }
    }

    /// Red for critical/high; amber for medium; dim for low.
    pub fn severity_color(&self, sev: Severity) -> &'static str {
        match sev {
            Severity::Critical | Severity::High => self.red,
            Severity::Medium => self.ylw,
            Severity::Low => self.dim,
        }
    }

    /// The cyan focus color used for the current-step `▸` marker.
    pub fn current_marker(&self) -> &'static str {
        self.cyn
    }

    #[cfg(test)]
    fn plain() -> Self {
        Self::resolve(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_off_emits_no_escapes() {
        let s = Style::plain();
        assert_eq!(s.bold, "");
        assert_eq!(s.cyn, "");
        assert_eq!(s.ring_color(Ring::ChangesState), "");
        assert_eq!(s.severity_color(Severity::Critical), "");
        assert_eq!(s.current_marker(), "");
    }

    #[test]
    fn ring_color_maps_state_change_to_amber_when_on() {
        // Build an "on" style directly to test the mapping without a TTY.
        let on = Style {
            bold: "B",
            dim: "D",
            red: "R",
            grn: "G",
            ylw: "Y",
            cyn: "C",
            rst: "0",
        };
        assert_eq!(on.ring_color(Ring::ChangesState), "Y");
        assert_eq!(on.ring_color(Ring::ReadOnly), "D");
        assert_eq!(on.ring_color(Ring::Info), "D");
    }

    #[test]
    fn severity_color_buckets_match_design() {
        let on = Style {
            bold: "B",
            dim: "D",
            red: "R",
            grn: "G",
            ylw: "Y",
            cyn: "C",
            rst: "0",
        };
        assert_eq!(on.severity_color(Severity::Critical), "R");
        assert_eq!(on.severity_color(Severity::High), "R");
        assert_eq!(on.severity_color(Severity::Medium), "Y");
        assert_eq!(on.severity_color(Severity::Low), "D");
    }
}
