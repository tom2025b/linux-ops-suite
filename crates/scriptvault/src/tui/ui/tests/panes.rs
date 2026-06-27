// Whole-frame layout: the search/results/preview panes with metadata, the calm
// empty-results state, robustness across small/narrow/short terminals, and the
// NO_COLOR vs coloured frame distinction.

use super::super::*;
use super::{app_with_theme, fixture_app, render_to_rows, render_to_string};
use crate::tui::app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;

#[test]
fn active_filters_render_as_chips() {
    // Typing an operator should surface a removable chip under the search box.
    let (mut app, dir) = fixture_app();
    for c in "t:ci".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains("[t:ci ✕]"),
        "active filter chip missing under search box\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn backspace_on_empty_text_pops_the_last_chip() {
    // With only operators in the query (no fuzzy text), Backspace removes the
    // last chip rather than a character.
    let (mut app, dir) = fixture_app();
    for c in "t:ci lang:bash".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    assert_eq!(app.active_chips(), vec!["t:ci", "lang:bash"]);
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(
        app.active_chips(),
        vec!["t:ci"],
        "Backspace on empty text should pop the last chip"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_line_shows_status_strip() {
    // The search line carries a right-aligned status strip: View · Sort · count.
    // The fixture indexes two scripts in the default All view.
    let (app, dir) = fixture_app();
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains("All · Auto · 2"),
        "status strip (View · Sort · count) missing from search line\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn renders_panes_and_metadata() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }

    let screen = render_to_rows(&app, 160, 30);

    assert!(
        screen.contains("ScriptVault"),
        "search bar title missing\n{screen}"
    );
    assert!(
        screen.contains("results"),
        "results pane title missing\n{screen}"
    );
    assert!(
        screen.contains("preview"),
        "preview pane title missing\n{screen}"
    );
    assert!(
        screen.contains("Deploy App"),
        "selected name missing\n{screen}"
    );
    assert!(screen.contains("ship it"), "desc missing\n{screen}");
    assert!(
        screen.contains("echo go"),
        "file-head content missing\n{screen}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_results_renders_calmly_no_panic() {
    let (mut app, dir) = fixture_app();
    for c in "zzzzzz".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    assert!(app.results().is_empty());
    let screen = render_to_string(&app, 100, 30);
    assert!(
        screen.contains("no result selected"),
        "empty-results preview hint missing"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn renders_in_a_small_terminal_without_panic() {
    let (app, dir) = fixture_app();
    let _ = render_to_string(&app, 20, 6);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn narrow_terminal_still_renders_list_and_status() {
    let (app, dir) = fixture_app();
    let screen = render_to_rows(&app, 60, 20); // 60 cols = narrow per layout_areas
    assert!(
        screen.contains("results"),
        "list title missing on narrow\n{screen}"
    );
    assert!(screen.contains("↑↓ move"), "hint missing\n{screen}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn tiny_height_collapses_search_bar() {
    let (app, dir) = fixture_app();
    // Very short: layout should still produce a valid frame without panic and
    // show results (search becomes 1 line).
    let screen = render_to_rows(&app, 80, 8);
    assert!(
        screen.contains("results"),
        "list must still render on short term\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

fn any_cell_is_coloured(app: &App, w: u16, h: u16) -> bool {
    use ratatui::style::Color;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    buf.content()
        .iter()
        .any(|c| !matches!(c.style().fg, None | Some(Color::Reset)))
}

#[test]
fn no_color_theme_draws_no_foreground_colour() {
    let dir = std::env::temp_dir().join(format!(
        "scriptvault-ui-nocolor-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("deploy.sh"),
        "#!/bin/bash\n# scriptvault.name: Deploy\n# scriptvault.tags: ci\necho go\n",
    )
    .unwrap();

    let app = app_with_theme(&dir, Theme::with_color(false));
    assert!(
        !any_cell_is_coloured(&app, 120, 24),
        "NO_COLOR frame must contain no foreground colour"
    );

    let lit = app_with_theme(&dir, Theme::with_color(true));
    assert!(
        any_cell_is_coloured(&lit, 120, 24),
        "coloured frame should contain at least one foreground colour"
    );

    fs::remove_dir_all(&dir).ok();
}
