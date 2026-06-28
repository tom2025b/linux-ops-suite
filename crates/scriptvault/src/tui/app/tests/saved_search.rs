// Saved-search modals (Phase 3): the name-on-save flow and the recall/delete
// picker. These drive the real handle_key dispatch (no TTY).

use super::{fixture_app, key, press};
use crate::tui::app::*;
use crossterm::event::KeyCode;
use std::fs;

// --- Phase 3 C-1: name-on-save modal -----------------------------------------
#[test]
fn save_search_modal_saves_under_typed_name() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    assert_eq!(app.query(), "deploy");

    // Enter the naming modal (what the palette "save search" item now triggers).
    app.enter_save_search();
    assert_eq!(app.mode(), Mode::SaveSearchName);

    // Type a real name and confirm.
    for c in "ci".chars() {
        app.handle_key(press(c));
    }
    app.handle_key(key(KeyCode::Enter));

    // Back to search, saved under the typed name with the captured query.
    assert_eq!(app.mode(), Mode::Search);
    assert_eq!(app.scriptvault.list_saved_searches(), vec!["ci"]);
    assert_eq!(app.scriptvault.get_saved_search("ci"), Some("deploy"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn save_search_modal_refuses_empty_name_and_cancels_on_esc() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    app.enter_save_search();

    // Enter with an empty name does NOT save and stays in the modal.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.mode(),
        Mode::SaveSearchName,
        "empty name keeps the modal open"
    );
    assert!(app.scriptvault.list_saved_searches().is_empty());

    // Esc cancels without saving.
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Search);
    assert!(app.scriptvault.list_saved_searches().is_empty());

    // Empty *main query* refuses to even open the modal.
    let (mut app2, dir2) = fixture_app();
    app2.enter_save_search();
    assert_eq!(app2.mode(), Mode::Search, "no query -> no modal");
    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&dir2).ok();
}

// --- Phase 3 C-2: recall / delete picker -------------------------------------
#[test]
fn saved_search_picker_loads_and_deletes() {
    let (mut app, dir) = fixture_app();
    // Seed two saved searches through the facade.
    app.scriptvault.save_search("ci", "deploy").unwrap();
    app.scriptvault.save_search("all", "backup").unwrap();

    app.enter_saved_search_picker();
    assert_eq!(app.mode(), Mode::SavedSearchPicker);
    assert_eq!(app.saved_search_selected(), Some(0));
    // Picker shows name + query pairs for the renderer.
    assert_eq!(
        app.saved_searches(),
        vec![
            ("ci".to_string(), "deploy".to_string()),
            ("all".to_string(), "backup".to_string()),
        ]
    );

    // Move to the second entry and load it -> query is recalled, modal closes.
    app.handle_key(press('j'));
    assert_eq!(app.saved_search_selected(), Some(1));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::Search);
    assert_eq!(app.query(), "backup");

    // Re-enter and delete the highlighted entry with `d`.
    app.enter_saved_search_picker();
    app.handle_key(press('d')); // deletes "ci" (index 0)
    assert_eq!(app.scriptvault.list_saved_searches(), vec!["all"]);

    // Deleting the last one closes the picker.
    app.handle_key(press('d'));
    assert!(app.scriptvault.list_saved_searches().is_empty());
    assert_eq!(app.mode(), Mode::Search);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn saved_search_picker_closes_on_c_or_q() {
    // The picker is a pure pick-list (no text entry), so the universal `c`/`q`
    // close keys must dismiss it — and must NOT delete or load anything.
    for close in ['c', 'q'] {
        let (mut app, dir) = fixture_app();
        app.scriptvault.save_search("ci", "deploy").unwrap();
        app.enter_saved_search_picker();
        assert_eq!(app.mode(), Mode::SavedSearchPicker);
        app.handle_key(press(close));
        assert_eq!(app.mode(), Mode::Search, "'{close}' must close the picker");
        // The saved search is untouched.
        assert_eq!(app.scriptvault.list_saved_searches(), vec!["ci"]);
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn save_search_name_treats_c_and_q_as_typed_text() {
    // CRITICAL: the name modal is a TEXT field, so `c`/`q` are literal characters,
    // never close keys. A user must be able to name a search "cleanup" or "queue".
    let (mut app, dir) = fixture_app();
    for ch in "deploy".chars() {
        app.handle_key(press(ch));
    }
    app.enter_save_search();
    assert_eq!(app.mode(), Mode::SaveSearchName);

    for ch in "cq-name".chars() {
        app.handle_key(press(ch));
    }
    // Still in the modal — c/q did NOT close it — and they landed in the buffer.
    assert_eq!(
        app.mode(),
        Mode::SaveSearchName,
        "c/q must not close a text modal"
    );
    assert_eq!(app.save_search_name(), "cq-name");

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.scriptvault.get_saved_search("cq-name"), Some("deploy"));

    fs::remove_dir_all(&dir).ok();
}
