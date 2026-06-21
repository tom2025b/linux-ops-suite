//! TUI runtime: suite-ui `Tui` guard + event loop over `AppState`.
pub mod state;
pub mod view;

use crate::error::ForgeError;
use crate::model::Selection;
use crate::registry::Registry;
use crate::tui::state::{AppState, Step};
use std::io::IsTerminal;
use suite_ui::{Tui, TuiOptions};

/// One key action the loop understands, mapped from a crossterm key event.
enum Action {
    Up,
    Down,
    Toggle,
    Next,
    Back,
    FilterChar(char),
    FilterBackspace,
    ToggleGit,
    Quit,
    None,
}

/// Run the interactive flow. Returns the resolved [`Selection`] on completion,
/// or `None` if the user quit (or there is no TTY).
pub fn run(reg: &Registry, project_name: String) -> Result<Option<Selection>, ForgeError> {
    if !std::io::stdout().is_terminal() {
        return Ok(None);
    }
    // Visible filter input -> do not hide the cursor; require a real tty.
    let mut tui = Tui::new(TuiOptions {
        require_tty: true,
        ..Default::default()
    })
    .map_err(|e| ForgeError::Write(crate::error::WriteError::Io(e.to_string())))?;

    let mut state = AppState::new(reg, project_name);

    loop {
        tui.terminal()
            .draw(|f| view::draw(f, &state, reg))
            .map_err(|e| ForgeError::Write(crate::error::WriteError::Io(e.to_string())))?;

        if state.step == Step::Done {
            return Ok(Some(state.selection()));
        }

        match read_action() {
            Action::Quit => return Ok(None),
            other => apply(&mut state, reg, other),
        }
    }
}

/// Apply one action to the state for the current step.
fn apply(state: &mut AppState, reg: &Registry, action: Action) {
    let len = match state.step {
        Step::Base => state.visible_bases(reg).len(),
        Step::Components => state.visible_components(reg).len(),
        _ => 0,
    };
    match action {
        Action::Up => state.move_up(),
        Action::Down => state.move_down(len),
        Action::Next => advance(state, reg),
        Action::Back => retreat(state),
        Action::Toggle => {
            // Space/Enter toggles on the component picker; elsewhere it commits
            // the step (choose base / confirm generate).
            if state.step == Step::Components {
                toggle_current(state, reg);
            } else {
                advance(state, reg);
            }
        }
        Action::ToggleGit => state.git = !state.git,
        Action::FilterChar(c) => {
            if state.step == Step::Components {
                state.filter.push(c);
                state.cursor = 0;
            }
        }
        Action::FilterBackspace => {
            if state.step == Step::Components {
                state.filter.pop();
                state.cursor = 0;
            }
        }
        Action::Quit | Action::None => {}
    }
}

fn toggle_current(state: &mut AppState, reg: &Registry) {
    if state.step != Step::Components {
        return;
    }
    let comps = state.visible_components(reg);
    if let Some(c) = comps.get(state.cursor) {
        let name = c.name.clone();
        state.select_component_by_name(&name);
    }
}

/// Move to the next step, committing the current step's cursor choice.
fn advance(state: &mut AppState, reg: &Registry) {
    match state.step {
        Step::Base => {
            let bases = state.visible_bases(reg);
            if let Some(b) = bases.get(state.cursor) {
                let name = b.name.clone();
                state.choose_base(name);
            }
            state.step = Step::Details;
        }
        Step::Details => {
            state.step = Step::Components;
            state.cursor = 0;
        }
        Step::Components => {
            state.step = Step::Confirm;
        }
        Step::Confirm => state.step = Step::Done,
        Step::Done => {}
    }
}

fn retreat(state: &mut AppState) {
    state.step = match state.step {
        Step::Base | Step::Details => Step::Base,
        Step::Components => Step::Details,
        Step::Confirm => Step::Components,
        Step::Done => Step::Confirm,
    };
    state.cursor = 0;
}

/// Block for one key press and map it to an [`Action`]. Press events only.
fn read_action() -> Action {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    loop {
        let Ok(ev) = event::read() else {
            return Action::Quit;
        };
        let Event::Key(k) = ev else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        return match k.code {
            KeyCode::Up | KeyCode::Char('k') => Action::Up,
            KeyCode::Down | KeyCode::Char('j') => Action::Down,
            KeyCode::Char(' ') | KeyCode::Enter => Action::Toggle,
            KeyCode::Tab => Action::Next,
            KeyCode::Right => Action::Next,
            KeyCode::BackTab => Action::Back,
            KeyCode::Left => Action::Back,
            KeyCode::Char('g') => Action::ToggleGit,
            KeyCode::Char('/') => Action::None, // filter mode is implicit; chars below
            KeyCode::Backspace => Action::FilterBackspace,
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char(c) if !shift => Action::FilterChar(c),
            _ => Action::None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn run_is_callable_and_returns_without_a_tty() {
        let reg = registry::load();
        let result = run(&reg, "myapp".into());
        assert!(result.is_ok());
    }

    #[test]
    fn advance_from_base_chooses_base_and_moves_to_details() {
        let reg = registry::load();
        let mut st = AppState::new(&reg, "x".into());
        // cursor 0 = first base alphabetically
        advance(&mut st, &reg);
        assert!(st.base.is_some());
        assert_eq!(st.step, Step::Details);
    }

    #[test]
    fn full_flow_reaches_done_and_yields_selection() {
        let reg = registry::load();
        let mut st = AppState::new(&reg, "x".into());
        advance(&mut st, &reg); // Base -> Details (base chosen)
        advance(&mut st, &reg); // Details -> Components
        advance(&mut st, &reg); // Components -> Confirm
        advance(&mut st, &reg); // Confirm -> Done
        assert_eq!(st.step, Step::Done);
        let sel = st.selection();
        assert!(!sel.base.is_empty());
    }
}
