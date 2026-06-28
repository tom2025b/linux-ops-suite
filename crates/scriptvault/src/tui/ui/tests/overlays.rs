// Overlay rendering: the Enter action menu, its title truncation, and the help
// and command-palette overlays.

use super::super::*;
use super::{fixture_app, render_to_rows};
use std::fs;

#[test]
fn enter_action_menu_renders_its_options() {
    use crossterm::event::{KeyCode, KeyEvent};
    let (mut app, dir) = fixture_app();
    // Open the action menu on the selected script.
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let screen = render_to_rows(&app, 80, 24);
    assert!(
        screen.contains("Open / edit") && screen.contains("Run") && screen.contains("Delete file"),
        "action menu options must be visible\n{screen}"
    );
    assert!(screen.contains("Cancel"), "cancel option must be visible");
    // The dim hint line (consistent with every other overlay) must render — and
    // its presence proves the compact box did not clip its last row. It now
    // advertises arrow navigation alongside the direct digit picks and cancels.
    assert!(
        screen.contains("↑/↓: move")
            && screen.contains("1–4: pick")
            && screen.contains("Esc / c / q: cancel"),
        "action menu hint line must be visible\n{screen}"
    );
    // The freshly opened menu highlights the first row (Open / edit) with the
    // shared `› ` selection marker, so arrow navigation has a visible anchor.
    assert!(
        screen.contains("› 1  Open / edit"),
        "first row must be highlighted on open\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn action_menu_title_truncates_long_names() {
    let title = menu_title("this-is-a-very-long-script-name-that-would-crowd-the-menu.sh");
    assert!(title.starts_with("For this-is-a-very-long-script"));
    assert!(title.ends_with('…'));
    assert!(title.chars().count() <= 34);
}

#[test]
fn help_overlay_renders_bindings_when_in_help_mode() {
    let (mut app, dir) = fixture_app();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('?'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let screen = render_to_rows(&app, 100, 30);
    assert!(
        screen.contains("Keybindings"),
        "help title missing\n{screen}"
    );
    assert!(
        screen.contains("run the selected script"),
        "help body missing\n{screen}"
    );
    assert!(
        screen.contains("filter by tag"),
        "operators should be in help\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_overlay_renders_on_ctrl_p() {
    let (mut app, dir) = fixture_app();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let screen = render_to_rows(&app, 100, 30);
    assert!(
        screen.contains("Command Palette"),
        "palette title\n{screen}"
    );
    assert!(
        screen.contains("view: favorites") || screen.contains("reload"),
        "palette body\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}
