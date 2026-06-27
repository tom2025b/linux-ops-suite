// tui/app/saved_search.rs — Phase 3 saved-search modals (name-on-save + recall).
// -----------------------------------------------------------------------------
// Two small modal state machines, following the same shape as edit.rs:
//   * SaveSearchName  — a ONE-field text entry to name the current query, then
//     persist it via the facade (`save_search`).
//   * SavedSearchPicker — a list picker to recall (load into the query) or delete
//     a saved search, showing each entry's query text so the choice is obvious.
// Both are `impl App` blocks reaching the parent's private fields via `super`.
// The `handle_*_key` entry points are `pub(super)` because the parent's
// `handle_key` dispatcher routes to them by mode (exactly like edit.rs).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Mode};

/// State for the two Phase 3 saved-search modals, grouped out of `App`'s top
/// level. The name buffer + captured query belong to the SaveSearchName modal;
/// `selected` is the SavedSearchPicker highlight. They are only ever touched by
/// the `impl App` methods in this file, so the renderer keeps using the
/// unchanged `save_search_name()` / `save_search_query()` / `saved_search_selected()`
/// accessors. `Default` (empty buffers, no selection) matches the old inline init.
#[derive(Debug, Default)]
pub struct SaveSearchState {
    /// SaveSearchName mode: the in-progress name buffer for the query being saved.
    pub name: String,
    /// The query captured when the save-name modal was opened (saved on confirm).
    pub query: String,
    /// SavedSearchPicker mode: index of the highlighted saved search.
    pub selected: Option<usize>,
}

impl App {
    // --- SaveSearchName (C-1): name the current query, then save -------------

    /// Accessor for the renderer: the in-progress name buffer.
    pub fn save_search_name(&self) -> &str {
        &self.save_search.name
    }

    /// Accessor for the renderer: the query that will be saved under the name.
    pub fn save_search_query(&self) -> &str {
        &self.save_search.query
    }

    /// Enter the "name this search" modal, capturing the current main-search query.
    /// An empty query has nothing to save, so we refuse and stay in Search.
    pub fn enter_save_search(&mut self) {
        let q = self.query.trim().to_string();
        if q.is_empty() {
            self.set_status("no query to save");
            return;
        }
        self.save_search.query = q;
        self.save_search.name.clear();
        self.mode = Mode::SaveSearchName;
    }

    /// Handle one key in SaveSearchName mode: type the name, Enter to save, Esc to
    /// cancel. Returns nothing — the caller already knows it consumed the key.
    pub(super) fn handle_save_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) | (KeyCode::Esc, _) => {
                self.cancel_save_search();
            }
            (KeyCode::Enter, _) => {
                let name = self.save_search.name.trim().to_string();
                if name.is_empty() {
                    self.set_status("name cannot be empty (Esc to cancel)");
                    return;
                }
                let query = self.save_search.query.clone();
                match self.scriptvault.save_search(&name, &query) {
                    Ok(()) => self.set_status(format!("saved search as '{name}'")),
                    Err(e) => self.set_status(format!("save error: {e}")),
                }
                self.cancel_save_search();
            }
            (KeyCode::Backspace, false) => {
                self.save_search.name.pop();
            }
            (KeyCode::Char(c), false) => {
                self.save_search.name.push(c);
            }
            _ => {}
        }
    }

    /// Leave the name modal, clearing its buffers, back to Search.
    fn cancel_save_search(&mut self) {
        self.save_search.name.clear();
        self.save_search.query.clear();
        self.mode = Mode::Search;
    }

    // --- SavedSearchPicker (C-2): recall or delete a saved search ------------

    /// Accessor for the renderer: index of the highlighted saved search.
    pub fn saved_search_selected(&self) -> Option<usize> {
        self.save_search.selected
    }

    /// The saved searches as owned (name, query) pairs, in insertion order. Built
    /// from the facade's name list + per-name lookup so the renderer can show both
    /// without a core change. Owned strings keep the renderer borrow-free.
    pub fn saved_searches(&self) -> Vec<(String, String)> {
        self.scriptvault
            .list_saved_searches()
            .into_iter()
            .map(|name| {
                let query = self.scriptvault.get_saved_search(name).unwrap_or("");
                (name.to_string(), query.to_string())
            })
            .collect()
    }

    /// Enter the recall picker. If there are no saved searches, say so and stay.
    pub fn enter_saved_search_picker(&mut self) {
        if self.scriptvault.list_saved_searches().is_empty() {
            self.set_status("no saved searches");
            return;
        }
        self.save_search.selected = Some(0);
        self.mode = Mode::SavedSearchPicker;
    }

    /// Handle one key in SavedSearchPicker mode: j/k move, Enter loads the query,
    /// `d` deletes the highlighted entry (closing the modal if it was the last),
    /// Esc cancels.
    pub(super) fn handle_saved_search_picker_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let names: Vec<String> = self
            .scriptvault
            .list_saved_searches()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let n = names.len();
        if n == 0 {
            self.close_saved_search_picker();
            return;
        }
        match (key.code, ctrl) {
            // A pure pick-list (no text entry), so the universal `c`/`q` close
            // keys apply here like in the action menu. Ctrl-C already quit at the
            // top of `handle_key`; Esc behaves the same as `c`/`q`.
            (KeyCode::Esc, _) | (KeyCode::Char('c'), false) | (KeyCode::Char('q'), false) => {
                self.close_saved_search_picker();
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), false) => {
                if let Some(i) = self.save_search.selected {
                    self.save_search.selected = Some(if i == 0 { n - 1 } else { i - 1 });
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), false) => {
                if let Some(i) = self.save_search.selected {
                    self.save_search.selected = Some(if i + 1 >= n { 0 } else { i + 1 });
                }
            }
            (KeyCode::Enter, _) => {
                if let Some(i) = self.save_search.selected
                    && let Some(q) = self.scriptvault.get_saved_search(&names[i])
                {
                    self.query = q.to_string();
                    self.refilter();
                    self.set_status(format!("loaded '{}'", names[i]));
                }
                self.close_saved_search_picker();
            }
            (KeyCode::Char('d'), false) => {
                if let Some(i) = self.save_search.selected {
                    match self.scriptvault.delete_saved_search(&names[i]) {
                        Ok(_) => self.set_status(format!("deleted '{}'", names[i])),
                        Err(e) => self.set_status(format!("delete error: {e}")),
                    }
                    // Re-clamp selection to the shrunk list; close if now empty.
                    let remaining = self.scriptvault.list_saved_searches().len();
                    if remaining == 0 {
                        self.close_saved_search_picker();
                    } else {
                        self.save_search.selected = Some(i.min(remaining - 1));
                    }
                }
            }
            _ => {}
        }
    }

    /// Leave the recall picker, clearing its selection, back to Search.
    fn close_saved_search_picker(&mut self) {
        self.save_search.selected = None;
        self.mode = Mode::Search;
    }
}
