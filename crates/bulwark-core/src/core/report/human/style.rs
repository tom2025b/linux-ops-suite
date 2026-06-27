//! Terminal style decisions for the human table.
//!
//! ANSI styling is isolated here so width calculation and row layout never need
//! to reason about escape sequences.

use std::io::IsTerminal;

/// Whether the human table should emit ANSI color codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// Decide based on stdout and the `NO_COLOR` convention.
    #[default]
    Auto,
    /// Always colorize, even when output is redirected.
    Always,
    /// Never colorize.
    Never,
}

impl ColorChoice {
    /// Resolve the user-facing color choice to a concrete on/off decision.
    pub fn use_color(self) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
            }
        }
    }
}

/// Apply simple ANSI colors to an already-padded risk cell.
pub(super) fn color_risk(cell: &str, use_color: bool) -> String {
    if !use_color {
        return cell.to_string();
    }

    match cell.trim() {
        "Low" => format!("\x1b[32m{cell}\x1b[0m"),
        "Medium" => format!("\x1b[33m{cell}\x1b[0m"),
        "High" => format!("\x1b[31m{cell}\x1b[0m"),
        "Critical" => format!("\x1b[1;31m{cell}\x1b[0m"),
        _ => cell.to_string(),
    }
}
