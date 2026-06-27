//! Terminal event loop and key dispatch for the TUI.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::Backend};

use crate::RiskLevel;

use super::TuiApp;
use crate::tui::ui::render_ui;

/// The main TUI event loop.
pub fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut TuiApp) -> Result<Option<String>> {
    loop {
        terminal.draw(|f| render_ui(f, app))?;

        if crossterm::event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = crossterm::event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Any key press dismisses the status message — it has been visible
            // for at least one frame. Keys that set a new status (r/e)
            // overwrite it during dispatch below.
            app.dismiss_status();

            if key.code == KeyCode::Char('q')
                || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(None);
            }

            if key.code == KeyCode::Char('?') || key.code == KeyCode::F(1) {
                app.toggle_help();
                continue;
            }

            if app.show_help {
                // Any key closes the help overlay.
                app.show_help = false;
                continue;
            }

            if app.filter_mode {
                match key.code {
                    KeyCode::Esc => app.clear_filter(),
                    KeyCode::Enter => app.commit_filter(),
                    KeyCode::Backspace => app.filter_backspace(),
                    KeyCode::Char(c) if !c.is_control() => app.push_filter_char(c),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
                KeyCode::Home | KeyCode::Char('g') => app.select_first(),
                KeyCode::End | KeyCode::Char('G') => app.select_last(),
                KeyCode::PageUp => app.select_page_up(),
                KeyCode::PageDown => app.select_page_down(),
                KeyCode::Char(' ') => app.select_page_down(),
                KeyCode::Enter => {
                    if let Some(&idx) = app.filtered.get(app.selected) {
                        let path = app.entries[idx].entry.discovered.path.display().to_string();
                        return Ok(Some(path));
                    }
                }
                KeyCode::Char('/') => app.begin_filter(),
                KeyCode::Char('d') => app.toggle_details(),
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    app.set_risk_filter(Some(RiskLevel::Low))
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    app.set_risk_filter(Some(RiskLevel::Medium))
                }
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    app.set_risk_filter(Some(RiskLevel::High))
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    app.set_risk_filter(Some(RiskLevel::Critical))
                }
                KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('0') => {
                    app.set_risk_filter(None)
                }
                KeyCode::Char('r') => {
                    if let Err(e) = app.rescan() {
                        app.status_message = Some(format!("rescan error: {e}"));
                    }
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    if let Err(e) = app.export_current_view() {
                        app.status_message = Some(format!("export failed: {e}"));
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => app.cycle_sort(),
                KeyCode::Esc => {
                    if !app.filter.is_empty() {
                        app.clear_filter();
                    } else if app.risk_filter.is_some() {
                        app.set_risk_filter(None);
                    }
                }
                _ => {}
            }
        }
    }
}
