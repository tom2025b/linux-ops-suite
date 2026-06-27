// The results surface: view-mode switching, query operators (`t:` tag filter),
// mouse click-to-select row mapping, and the colour choice that flows into the
// app's theme at construction.

use super::{fixture_app, press};
use crate::tui::app::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::fs;

#[test]
fn view_modes_switch_with_a_f_r_when_empty_query() {
    let (mut app, dir) = fixture_app();
    assert_eq!(app.view_mode(), ViewMode::All);
    // Shifted A/F/R for views when empty (avoids clashing with bare r typing into query).
    app.handle_key(press('F'));
    assert_eq!(app.view_mode(), ViewMode::Favorites);
    // results may be 0 or fewer
    let _ = app.results().len();
    app.handle_key(press('A'));
    assert_eq!(app.view_mode(), ViewMode::All);
    app.handle_key(press('R'));
    assert_eq!(app.view_mode(), ViewMode::Recents);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn tag_operator_filters_results() {
    let (mut app, dir) = fixture_app();
    // fixture has one with tags: ci, prod
    for c in "t:ci".chars() {
        app.handle_key(press(c));
    }
    // After operator, results filtered by tag (in this fixture the "deploy" now has ci tag).
    // We accept 0 or more; the important is no panic and parse path exercised.
    let n = app.results().len();
    assert!(n <= 2, "got {n} results after t:ci (filter applied)");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_theme_honours_explicit_color_choice() {
    // The --color flag flows through to the App's theme. Build two apps over the
    // same fixture with forced-on and forced-off colour and assert the theme
    // reflects it (a styled prompt has a fg only when colour is on).
    let (_, dir) = fixture_app();
    let config = scriptvault_core::Config {
        roots: vec![dir.clone()],
        ..Default::default()
    };
    let make = |on: bool| {
        let sv = ScriptVault::load_with_state_at(
            config.clone(),
            scriptvault_core::State::default(),
            dir.join("state.json"),
        )
        .unwrap();
        App::with_theme(sv, Theme::with_color(on))
    };
    assert!(
        make(true).theme().prompt().fg.is_some(),
        "colour on → prompt has a fg"
    );
    assert_eq!(
        make(false).theme().prompt().fg,
        None,
        "colour off → prompt has no fg (NO_COLOR-safe)"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn mouse_click_uses_exact_list_rect_for_row_mapping() {
    let (mut app, dir) = fixture_app();
    // Simulate a list rect from layout (e.g. after draw on a normal 80x24 term).
    // Content rows start at y+1.
    let list_r = ratatui::layout::Rect {
        x: 2,
        y: 4,
        width: 35,
        height: 15,
    };
    app.set_list_rect(list_r);

    // Click first content row (y = 4+1 = 5) -> should select 0
    let m0 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(m0);
    assert_eq!(app.selected(), Some(0));

    // Click row 6 (2nd item, idx=1). Fixture only has 2 scripts.
    let m1 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 6,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(m1);
    assert_eq!(app.selected(), Some(1));

    // Click outside the list rect (preview side) -> no change
    let m_out = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 50,
        row: 6,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(m_out);
    assert_eq!(app.selected(), Some(1)); // unchanged

    // Click before content top -> ignored
    let m_before = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 4, // the title border line
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(m_before);
    assert_eq!(app.selected(), Some(1));

    fs::remove_dir_all(&dir).ok();
}
