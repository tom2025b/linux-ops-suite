//! `Tui`: a RAII terminal scope guard. Setup in `new`, guaranteed teardown in
//! `Drop` (runs on normal return, `?` propagation, and panic unwind alike).

use std::io::{self, IsTerminal, Write, stdout};

use crossterm::execute;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::DefaultTerminal;

/// What can go wrong setting up the terminal.
#[derive(Debug)]
pub enum TuiError {
    /// `require_tty` was set but stdout is not a terminal. `Display` carries an
    /// actionable message pointing the user at the non-interactive CLI.
    NotATerminal,
    /// A terminal setup call failed (entering raw mode, the alt screen, etc.).
    Io(io::Error),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuiError::NotATerminal => write!(
                f,
                "this command requires an interactive terminal\n\
                 (stdout is not a tty / not connected to a real terminal).\n\n\
                 For non-interactive use, run the CLI subcommands instead."
            ),
            TuiError::Io(e) => write!(f, "terminal setup failed: {e}"),
        }
    }
}

impl std::error::Error for TuiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TuiError::Io(e) => Some(e),
            TuiError::NotATerminal => None,
        }
    }
}

impl From<io::Error> for TuiError {
    fn from(e: io::Error) -> Self {
        TuiError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_terminal_display_is_actionable() {
        let msg = TuiError::NotATerminal.to_string();
        assert!(msg.contains("interactive terminal"), "names the requirement");
        assert!(msg.contains("CLI"), "points at the non-interactive fallback");
    }
}
