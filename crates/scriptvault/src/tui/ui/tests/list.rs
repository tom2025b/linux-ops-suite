// Results-list rendering: match highlighting, name-vs-other-field highlight
// gating, the selection rail glyph, the favorite star, and the matched-field
// badge (shown only when a query is active).

use super::super::*;
use super::{fixture_app, render_to_rows, render_to_string};
use std::fs;
use std::path::PathBuf;

#[test]
fn highlight_spans_split_on_matched_chars() {
    let spans = highlight_spans("deploy", &[0, 1, 2], Theme::with_color(true));
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].content, "dep");
    assert_eq!(spans[1].content, "loy");
}

#[test]
fn highlight_spans_empty_indices_is_single_plain_span() {
    let spans = highlight_spans("deploy", &[], Theme::with_color(true));
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "deploy");
}

#[test]
fn name_indices_empty_when_match_is_on_tags_not_name() {
    use scriptvault_core::{Language, MetaSource, ScriptEntry, ScriptMetadata};
    let r = SearchResult {
        entry: ScriptEntry {
            path: PathBuf::from("/x/thing.sh"),
            filename: "thing.sh".to_string(),
            lang: Language::Bash,
            meta: ScriptMetadata {
                name: Some("Thing".to_string()),
                tags: vec!["backup".to_string()],
                ..Default::default()
            },
            source: MetaSource::Header,
        },
        score: 50,
        matched_field: MatchField::Tags,
        matched_indices: vec![0, 1, 2],
    };
    assert!(
        name_indices(&r).is_empty(),
        "a tags match must not highlight the name"
    );
}

#[test]
fn renders_selection_rail() {
    let (app, dir) = fixture_app();
    let screen = render_to_string(&app, 100, 30);
    // The selected row is marked by a solid cyan rail glyph built into its
    // content (no ratatui highlight_symbol anymore).
    assert!(
        screen.contains('▌'),
        "selection rail glyph not drawn\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn favorited_row_shows_star() {
    let (mut app, dir) = fixture_app();
    let path = app.results()[0].entry.path.clone();
    app.toggle_favorite(&path).unwrap();
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains('★'),
        "favorite star missing from list\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_row_shows_frecency_hint() {
    // After a script is run, its results row gains a dim run-history hint
    // (`▲N× … ✓`). A never-run row shows no such hint.
    let (mut app, dir) = fixture_app();
    let path = app.results()[0].entry.path.clone();
    app.record_run_with_status(&path, Some(0), None).unwrap();
    app.refresh_results();
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains('▲'),
        "run-history hint glyph missing after a recorded run\n{screen}"
    );
    assert!(
        screen.contains("▲1×"),
        "hint should show a run count of 1\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn never_run_row_has_no_frecency_hint() {
    let (app, dir) = fixture_app();
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        !screen.contains('▲'),
        "a fresh fixture with no runs must show no run-history hint\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn results_row_shows_matched_field_badge() {
    let (mut app, dir) = fixture_app();
    for c in "app".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains("Deploy App"),
        "expected the name-matching entry\n{screen}"
    );
    assert!(
        screen.contains("·name"),
        "matched-field badge missing from results row\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_results_for_query_show_no_match_hint() {
    // Typing a query that matches nothing must replace the empty box with a
    // context-aware hint naming the query and offering Ctrl-U to clear it.
    let (mut app, dir) = fixture_app();
    for c in "zzzznomatchzzzz".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    assert!(app.results().is_empty(), "query should match nothing");
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains("no matches for"),
        "empty results should show the no-match hint\n{screen}"
    );
    assert!(
        screen.contains("Ctrl-U"),
        "no-match hint should suggest clearing the query\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_favorites_view_shows_star_hint() {
    // The favorites view with no favorites yet must guide the user to Ctrl-F,
    // not render a blank pane.
    let (mut app, dir) = fixture_app();
    // Switch to the favorites view the way a user does: `F` with an empty query.
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('F'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.results().is_empty(), "no favorites in a fresh fixture");
    let screen = render_to_rows(&app, 120, 24);
    assert!(
        screen.contains("no favorites yet"),
        "empty favorites view should show its hint\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_query_view_shows_no_match_badge() {
    let (app, dir) = fixture_app();
    assert_eq!(app.query(), "", "fixture should start with an empty query");
    let screen = render_to_rows(&app, 120, 24);
    for badge in ["·name", "·tags", "·desc", "·file"] {
        assert!(
            !screen.contains(badge),
            "empty-query view must not show a `{badge}` badge\n{screen}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}
