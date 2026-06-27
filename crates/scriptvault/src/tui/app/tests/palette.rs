// Command-palette behaviour: entering/leaving the palette, dispatching an action
// command (parity with the direct Ctrl chord), and the reload command's
// incremental-rescan + debounce semantics.

use super::{ctrl, fixture_app, key, press};
use crate::tui::app::*;
use crossterm::event::KeyCode;
use std::fs;

#[test]
fn ctrl_p_enters_palette_mode_and_esc_exits() {
    let (mut app, dir) = fixture_app();
    assert_eq!(app.mode(), Mode::Search);
    app.handle_key(ctrl('p'));
    assert_eq!(app.mode(), Mode::CommandPalette);
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Search);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_run_command_emits_act_and_exits_palette() {
    // Regression: the palette's PaletteAction::Act arm used to be inert (it only
    // set a "use key or reselect" status). It must now return Outcome::Act so the
    // event loop runs the script via actions::perform — full parity with ^R.
    let (mut app, dir) = fixture_app();
    assert!(
        app.selected_result().is_some(),
        "fixture should select a row"
    );

    app.handle_key(ctrl('p')); // open palette
    // Filter to "run": it is the FIRST entry in palette_commands(), so the
    // default selection (index 0 of the filtered list) lands on it.
    for c in "run".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        outcome,
        Outcome::Act(ActionKind::Run),
        "palette 'run' must emit Outcome::Act(Run)"
    );
    assert_eq!(
        app.mode(),
        Mode::Search,
        "dispatching an action closes the palette"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_act_with_no_selection_is_a_noop_continue() {
    // With nothing selected, an Act command must NOT emit an action; it reports
    // "no result selected" and continues (so the event loop does nothing).
    let (mut app, dir) = fixture_app();
    // Force an empty result set so there is no selection: a query that matches
    // nothing in the fixture.
    for c in "zzzznomatchzzzz".chars() {
        app.handle_key(press(c));
    }
    assert!(
        app.selected_result().is_none(),
        "query should match nothing"
    );

    app.handle_key(ctrl('p'));
    for c in "run".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        outcome,
        Outcome::Continue,
        "Act with no selection must not emit an action intent"
    );
    assert_eq!(app.status(), "no result selected");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_reload_rescans_and_returns_to_search() {
    // handle_key → Outcome flow for the palette "reload" command: it rescans from
    // disk, reports "reloaded", and returns to Search mode (Continue throughout).
    let (mut app, dir) = fixture_app();
    app.handle_key(ctrl('p')); // open palette
    for c in "reload".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, Outcome::Continue, "reload is an in-place command");
    assert_eq!(app.mode(), Mode::Search, "dispatch closes the palette");
    assert_eq!(app.status(), "reloaded");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_edit_metadata_enters_edit_mode() {
    // Regression: dispatch used to enter edit mode and then immediately call
    // exit_palette(), which reset the app back to Search before the modal could
    // render.
    let (mut app, dir) = fixture_app();

    app.handle_key(ctrl('p'));
    for c in "edit metadata".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));

    assert_eq!(outcome, Outcome::Continue);
    assert_eq!(app.mode(), Mode::EditMetadata);
    fs::remove_dir_all(&dir).ok();
}

// --- palette → modal transitions (the exit_palette() guard) ---
//
// The Piece 4 refactor closes the palette with ONE trailing exit_palette() after
// dispatch_special. exit_palette() now resets mode to Search only if still in
// CommandPalette, so a command that opened ANOTHER modal lands in that modal, not
// back in Search. These tests pin that for each modal-opening command (the
// edit-metadata case above is the original regression; these cover the rest).

#[test]
fn palette_save_search_opens_the_name_modal() {
    let (mut app, dir) = fixture_app();
    // save-search needs a non-empty query to capture; seed one first.
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    app.handle_key(ctrl('p'));
    for c in "save search".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, Outcome::Continue);
    assert_eq!(
        app.mode(),
        Mode::SaveSearchName,
        "save-search must open the name modal, not bounce to Search"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_load_saved_search_opens_the_picker() {
    let (mut app, dir) = fixture_app();
    // The picker only opens when at least one saved search exists.
    app.scriptvault.save_search("mine", "deploy").unwrap();

    app.handle_key(ctrl('p'));
    for c in "load saved".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, Outcome::Continue);
    assert_eq!(
        app.mode(),
        Mode::SavedSearchPicker,
        "load-saved-search must open the picker, not bounce to Search"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_add_to_playlist_opens_picker_when_multiple_exist() {
    let (mut app, dir) = fixture_app();
    // With 2+ playlists, add-to-playlist routes to the picker (0/1 act in place).
    app.scriptvault.create_playlist("a").unwrap();
    app.scriptvault.create_playlist("b").unwrap();
    assert!(app.selected_result().is_some());

    app.handle_key(ctrl('p'));
    for c in "add to playlist".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, Outcome::Continue);
    assert_eq!(
        app.mode(),
        Mode::PlaylistPicker,
        "add-to-playlist with multiple playlists must open the picker"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn palette_toggle_output_stays_in_search() {
    // A plain in-place Special (no modal) must close the palette back to Search.
    let (mut app, dir) = fixture_app();
    assert!(!app.is_showing_output());
    app.handle_key(ctrl('p'));
    for c in "toggle output".chars() {
        app.handle_key(press(c));
    }
    let outcome = app.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, Outcome::Continue);
    assert_eq!(
        app.mode(),
        Mode::Search,
        "in-place command returns to Search"
    );
    assert!(app.is_showing_output(), "toggle-output actually toggled");
    fs::remove_dir_all(&dir).ok();
}
