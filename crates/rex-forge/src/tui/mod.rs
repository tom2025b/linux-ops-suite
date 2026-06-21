//! TUI runtime: suite-ui `Tui` guard + event loop over `AppState`.
pub mod state;

use crate::error::ForgeError;
use crate::registry::Registry;

/// Run the interactive flow. Returns the resolved `Selection` on completion,
/// or `None` if the user quit (or there is no TTY).
///
/// The real suite-ui event loop is wired in the next task; the state machine in
/// [`state`] is already unit-tested. With no TTY this returns `Ok(None)` so
/// tests and piped invocations stay safe.
pub fn run(
    _reg: &Registry,
    _project_name: String,
) -> Result<Option<crate::model::Selection>, ForgeError> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Ok(None);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn run_is_callable_and_returns_without_a_tty() {
        // No TTY in the test env -> run() must not panic and returns Ok(None).
        let reg = registry::load();
        let result = run(&reg, "myapp".into());
        assert!(result.is_ok());
    }
}
