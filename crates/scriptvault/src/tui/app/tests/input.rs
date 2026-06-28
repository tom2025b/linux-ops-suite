// Key routing, navigation, the Enter action menu, view-mode switching, query
// operators, and mouse-to-row mapping — the "input event → App state" surface.

use super::{ctrl, fixture_app, key, press};
use crate::tui::app::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

#[test]
fn question_mark_toggles_help_mode() {
    let (mut app, dir) = fixture_app();
    assert_eq!(app.mode(), Mode::Search);
    app.handle_key(press('?'));
    assert_eq!(app.mode(), Mode::Help);
    assert_eq!(app.handle_key(key(KeyCode::Esc)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_u_clears_query() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    assert_eq!(app.query(), "deploy");
    app.handle_key(ctrl('u'));
    assert_eq!(app.query(), "");
    assert_eq!(app.results().len(), 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn home_and_end_jump_selection() {
    let (mut app, dir) = fixture_app();
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.selected(), Some(app.results().len() - 1));
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.selected(), Some(0));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn help_mode_ignores_navigation_keys() {
    let (mut app, dir) = fixture_app();
    app.handle_key(press('?'));
    let before = app.selected();
    app.handle_key(key(KeyCode::Down));
    app.handle_key(press('x'));
    assert_eq!(app.selected(), before, "nav inert in help mode");
    assert_eq!(app.query(), "", "query editing inert in help mode");
    assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn starts_with_all_results_selected_top() {
    let (app, dir) = fixture_app();
    assert_eq!(app.results().len(), 2);
    assert_eq!(app.selected(), Some(0));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn typing_filters_and_keeps_valid_selection() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    assert_eq!(app.query(), "deploy");
    assert_eq!(app.results().len(), 1);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.selected_result().unwrap().entry.display_name(),
        "deploy"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_match_clears_selection_and_actions_noop() {
    let (mut app, dir) = fixture_app();
    for c in "zzzzz".chars() {
        app.handle_key(press(c));
    }
    assert!(app.results().is_empty());
    assert_eq!(app.selected(), None);
    assert_eq!(app.handle_key(key(KeyCode::Enter)), Outcome::Continue);
    assert_eq!(app.handle_key(ctrl('r')), Outcome::Continue);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn backspace_restores_results() {
    let (mut app, dir) = fixture_app();
    for c in "deploy".chars() {
        app.handle_key(press(c));
    }
    assert_eq!(app.results().len(), 1);
    for _ in 0..6 {
        app.handle_key(key(KeyCode::Backspace));
    }
    assert_eq!(app.query(), "");
    assert_eq!(app.results().len(), 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn navigation_saturates_no_wrap() {
    let (mut app, dir) = fixture_app();
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.selected(), Some(0));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.selected(), Some(1));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn action_keys_emit_correct_intents_when_selected() {
    let (mut app, dir) = fixture_app();
    // Enter now opens the action menu; pressing 1 there picks Open/Edit.
    assert_eq!(app.handle_key(key(KeyCode::Enter)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::ActionMenu);
    assert_eq!(
        app.handle_key(key(KeyCode::Char('1'))),
        Outcome::Act(ActionKind::OpenEditor)
    );
    assert_eq!(app.mode(), Mode::Search, "menu closes after picking");
    // The Ctrl shortcuts remain direct (no menu).
    assert_eq!(app.handle_key(ctrl('r')), Outcome::Act(ActionKind::Run));
    assert_eq!(
        app.handle_key(ctrl('y')),
        Outcome::Act(ActionKind::CopyPath)
    );
    assert_eq!(
        app.handle_key(ctrl('o')),
        Outcome::Act(ActionKind::PrintPath)
    );
    assert_eq!(
        app.handle_key(ctrl('f')),
        Outcome::Act(ActionKind::ToggleFavorite)
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enter_action_menu_dispatches_each_option() {
    let (mut app, dir) = fixture_app();

    // 2) Run
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.handle_key(key(KeyCode::Char('2'))),
        Outcome::Act(ActionKind::Run)
    );
    assert_eq!(app.mode(), Mode::Search);

    // 3) Delete — stages the confirm modal; only an explicit `y` emits the intent.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.handle_key(key(KeyCode::Char('3'))), Outcome::Continue);
    assert_eq!(app.mode(), Mode::ConfirmDelete, "delete must confirm first");
    assert_eq!(
        app.handle_key(key(KeyCode::Char('y'))),
        Outcome::Act(ActionKind::Delete)
    );
    assert_eq!(app.mode(), Mode::Search);

    // 4) Cancel -> no action, menu closes.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.handle_key(key(KeyCode::Char('4'))), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);

    // Esc also cancels.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::ActionMenu);
    assert_eq!(app.handle_key(key(KeyCode::Esc)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn action_menu_arrows_move_highlight_and_enter_activates() {
    // ↑/↓ (and j/k) move the highlight; Enter activates whatever row is lit —
    // equivalent to pressing that row's digit.
    let (mut app, dir) = fixture_app();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::ActionMenu);
    assert_eq!(app.action_menu_selected(), 0, "opens on the first row");

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.action_menu_selected(), 1, "↓ moves down");
    app.handle_key(press('j'));
    assert_eq!(app.action_menu_selected(), 2, "j also moves down");
    app.handle_key(press('k'));
    assert_eq!(app.action_menu_selected(), 1, "k moves up");
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.action_menu_selected(), 0, "↑ moves up");

    // Highlight clamps at the top (no wrap).
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.action_menu_selected(), 0, "↑ saturates at the top");

    // Move to Run (row 1) and Enter — must emit the Run intent and close.
    app.handle_key(key(KeyCode::Down));
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)),
        Outcome::Act(ActionKind::Run)
    );
    assert_eq!(app.mode(), Mode::Search);

    // Reopen, move to Delete (row 2) and Enter — must stage the confirm modal.
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.action_menu_selected(), 2);
    assert_eq!(app.handle_key(key(KeyCode::Enter)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::ConfirmDelete, "Enter on Delete confirms");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn action_menu_highlight_clamps_at_bottom() {
    // ↓ past the last row (Cancel, row 3) must clamp, not wrap or overflow.
    let (mut app, dir) = fixture_app();
    app.handle_key(key(KeyCode::Enter));
    for _ in 0..6 {
        app.handle_key(key(KeyCode::Down));
    }
    assert_eq!(app.action_menu_selected(), 3, "↓ saturates at the last row");
    // Enter on the Cancel row closes the menu with no action.
    assert_eq!(app.handle_key(key(KeyCode::Enter)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn help_closes_on_c_or_q() {
    // c and q are the universal overlay-close keys: both must dismiss Help.
    for close in ['c', 'q'] {
        let (mut app, dir) = fixture_app();
        app.handle_key(press('?'));
        assert_eq!(app.mode(), Mode::Help);
        assert_eq!(app.handle_key(press(close)), Outcome::Continue);
        assert_eq!(app.mode(), Mode::Search, "'{close}' must close help");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn confirm_delete_closes_on_c_or_q() {
    // c and q cancel a staged delete just like n/Esc — and never confirm it.
    for close in ['c', 'q'] {
        let (mut app, dir) = fixture_app();
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.mode(), Mode::ConfirmDelete);
        assert_eq!(app.handle_key(press(close)), Outcome::Continue);
        assert_eq!(app.mode(), Mode::Search, "'{close}' must cancel the delete");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn ctrl_c_quits_from_every_mode() {
    // Ctrl-C is the universal app-quit: it must return Quit no matter which
    // overlay/menu is open, never merely close it. A fresh app per mode keeps
    // each case independent (Quit doesn't reset App state — the shell tears it
    // down, so reusing one app across cases would leave it in a stale mode).

    // Search (main screen).
    {
        let (mut app, dir) = fixture_app();
        assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit, "from search");
        fs::remove_dir_all(&dir).ok();
    }
    // Help.
    {
        let (mut app, dir) = fixture_app();
        app.handle_key(press('?'));
        assert_eq!(app.mode(), Mode::Help);
        assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit, "from help");
        fs::remove_dir_all(&dir).ok();
    }
    // Action menu.
    {
        let (mut app, dir) = fixture_app();
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.mode(), Mode::ActionMenu);
        assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit, "from action menu");
        fs::remove_dir_all(&dir).ok();
    }
    // Confirm-delete.
    {
        let (mut app, dir) = fixture_app();
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.mode(), Mode::ConfirmDelete);
        assert_eq!(
            app.handle_key(ctrl('c')),
            Outcome::Quit,
            "from confirm-delete"
        );
        fs::remove_dir_all(&dir).ok();
    }
    // Command palette.
    {
        let (mut app, dir) = fixture_app();
        app.handle_key(ctrl('p'));
        assert_eq!(app.mode(), Mode::CommandPalette);
        assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit, "from palette");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn action_menu_cancels_on_plain_c_or_q() {
    // Esc is the prefix byte of every arrow/function-key escape sequence, so a
    // lone Esc can stall in the terminal's input parser and feel dead live.
    // Plain single-byte `c`/`q` are an always-reliable way out of the menu.
    let (mut app, dir) = fixture_app();

    for cancel in ['c', 'q'] {
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.mode(), Mode::ActionMenu);
        assert_eq!(app.handle_key(press(cancel)), Outcome::Continue);
        assert_eq!(app.mode(), Mode::Search, "'{cancel}' must close the menu");
    }

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn playlist_picker_closes_on_c_or_q() {
    // The playlist picker is a pure pick-list (no text entry), so the universal
    // `c`/`q` close keys must dismiss it like the action menu does.
    for close in ['c', 'q'] {
        let (mut app, dir) = fixture_app();
        app.scriptvault.create_playlist("work").unwrap();
        app.enter_playlist_picker();
        assert_eq!(app.mode(), Mode::PlaylistPicker);
        app.handle_key(press(close));
        assert_eq!(app.mode(), Mode::Search, "'{close}' must close the picker");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn delete_confirm_cancels_on_n_or_esc_and_ignores_other_keys() {
    let (mut app, dir) = fixture_app();

    // n cancels and reports it on the status line.
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('3')));
    assert_eq!(app.mode(), Mode::ConfirmDelete);
    assert_eq!(app.handle_key(key(KeyCode::Char('n'))), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);
    assert_eq!(app.status(), "delete cancelled");

    // Stray keys are inert (the modal can't be typed through); Esc cancels.
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('3')));
    assert_eq!(app.handle_key(press('x')), Outcome::Continue);
    assert_eq!(app.mode(), Mode::ConfirmDelete, "stray key must stay modal");
    assert_eq!(app.handle_key(key(KeyCode::Esc)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn action_menu_does_not_open_with_no_selection() {
    // Type a query that matches nothing -> no selection -> Enter shows a hint,
    // never opens the menu (so the menu always has a target).
    let (mut app, dir) = fixture_app();
    for c in "zzzznotathing".chars() {
        app.handle_key(press(c));
    }
    assert!(app.selected_result().is_none());
    assert_eq!(app.handle_key(key(KeyCode::Enter)), Outcome::Continue);
    assert_eq!(app.mode(), Mode::Search, "no menu without a selection");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn digits_in_search_are_unaffected_by_the_menu() {
    // The whole point of the redesign: bare digits type into search, they don't
    // trigger actions. Only inside the open menu do 1-4 act.
    let (mut app, dir) = fixture_app();
    for c in "log2".chars() {
        app.handle_key(press(c));
    }
    assert_eq!(app.query(), "log2", "digits must reach the query");
    assert_eq!(app.mode(), Mode::Search);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn esc_and_ctrl_c_quit() {
    let (mut app, dir) = fixture_app();
    assert_eq!(app.handle_key(key(KeyCode::Esc)), Outcome::Quit);
    assert_eq!(app.handle_key(ctrl('c')), Outcome::Quit);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn typing_r_or_y_without_ctrl_filters_not_acts() {
    let (mut app, dir) = fixture_app();
    let out = app.handle_key(press('r'));
    assert_eq!(out, Outcome::Continue);
    assert_eq!(app.query(), "r");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn j_k_move_like_arrows() {
    let (mut app, dir) = fixture_app();
    let start = app.selected();
    app.handle_key(press('j'));
    assert_eq!(app.selected(), Some(start.unwrap_or(0) + 1));
    app.handle_key(press('k'));
    assert_eq!(app.selected(), start);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn g_g_jump_when_query_empty() {
    let (mut app, dir) = fixture_app();
    app.handle_key(press('G'));
    assert_eq!(app.selected(), Some(app.results().len() - 1));
    app.handle_key(press('g'));
    assert_eq!(app.selected(), Some(0));
    fs::remove_dir_all(&dir).ok();
}

// --- precedence trap (the reason the spine is the single source of truth) ---
//
// There are TWO precedence classes among the letter keys, and the refactor must
// preserve the distinction exactly:
//   * g / G / A / F / R are special ONLY when the query is empty. With a non-empty
//     query they fall through the spine's empty-guard to the `Char(c)` catch-all
//     and TYPE. (Capital letters never collide with a real shortcut otherwise.)
//   * j / k (like the arrow keys) are UNCONDITIONAL navigation — they were never
//     empty-guarded in the original, so they move the selection even mid-query and
//     do NOT type. This asymmetry is intentional (vim muscle memory for movement);
//     the test below pins it so nobody "fixes" j/k into the g/G class by mistake.

#[test]
fn g_and_capital_g_type_into_a_non_empty_query() {
    let (mut app, dir) = fixture_app();
    // Seed a non-empty query first so g/G are no longer jump keys. `dgG` matches
    // nothing in the fixture, so the selection legitimately clears to None — the
    // point of the test is that g/G TYPED (query grew) rather than jumping.
    app.handle_key(press('d'));
    app.handle_key(press('g'));
    app.handle_key(press('G'));
    assert_eq!(
        app.query(),
        "dgG",
        "g/G must type once the query is non-empty"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn j_and_k_are_unconditional_navigation_not_typing() {
    let (mut app, dir) = fixture_app();
    // Full list (empty query) has two rows; j/k move the selection and must NOT
    // append to the query — they are unconditional navigation like the arrows,
    // distinct from g/G which only jump when the query is empty.
    assert_eq!(app.results().len(), 2);
    assert_eq!(app.selected(), Some(0));
    app.handle_key(press('j'));
    assert_eq!(app.selected(), Some(1), "j moves down");
    assert_eq!(app.query(), "", "j must not type");
    app.handle_key(press('k'));
    assert_eq!(app.selected(), Some(0), "k moves up");
    assert_eq!(app.query(), "", "k must not type");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn capital_a_f_r_type_into_a_non_empty_query_and_dont_switch_view() {
    let (mut app, dir) = fixture_app();
    app.handle_key(press('d'));
    assert_eq!(app.view_mode(), ViewMode::All);
    app.handle_key(press('F'));
    app.handle_key(press('R'));
    app.handle_key(press('A'));
    assert_eq!(
        app.query(),
        "dFRA",
        "A/F/R must type once the query is non-empty"
    );
    assert_eq!(
        app.view_mode(),
        ViewMode::All,
        "A/F/R must NOT switch the view while typing"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn shift_pageup_scrolls_output_when_pane_shown() {
    let (mut app, dir) = fixture_app();
    // Seed enough output that scrolling up from the tail has somewhere to go,
    // then reveal the pane (toggle on; the pane starts pinned to the tail = 0).
    for i in 0..30 {
        app.push_output_line(format!("line {i}"));
    }
    app.toggle_output_pane();
    assert!(app.is_showing_output());
    assert_eq!(app.output_scroll(), 0, "pane opens pinned to the tail");

    let shift_pageup = KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT);
    app.handle_key(shift_pageup);
    assert!(
        app.output_scroll() > 0,
        "Shift-PageUp must scroll the output pane up from the tail when shown"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn shift_pageup_is_inert_when_output_hidden() {
    let (mut app, dir) = fixture_app();
    assert!(!app.is_showing_output());
    let sel_before = app.selected();
    let shift_pageup = KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT);
    app.handle_key(shift_pageup);
    // With the pane hidden, Shift-PageUp scrolls nothing and (per the long-standing
    // behavior) does not page the list either — selection is unchanged.
    assert_eq!(app.output_scroll(), 0);
    assert_eq!(app.selected(), sel_before);
    fs::remove_dir_all(&dir).ok();
}
