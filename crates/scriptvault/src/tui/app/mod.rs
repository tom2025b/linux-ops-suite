// tui/app.rs — pure App state machine (query, results, selection, Mode, ViewMode).
// -----------------------------------------------------------------------------
// All decision logic lives here. `handle_key` returns Outcome; effects live in
// actions.rs. Tested directly with no terminal (see app/tests.rs).
// Invariants: NO_COLOR never leaks into logic; zero unwrap in non-test code.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use scriptvault_core::{Filter, Query, ScriptVault, SearchResult, View};

use super::theme::Theme;

/// The persistent keybinding hints shown in the footer, as `(key, label)` pairs
/// for suite-ui's `KeyHints`. They never change, so a user can always see what
/// the keys do — transient action results render on a SEPARATE line so they never
/// erase these (the bug a single shared status line otherwise has: after the first
/// action, the help would vanish until restart). Kept short and focused on the
/// MOST important keys so the strip fits comfortably; the full keymap lives behind
/// `?`. `KeyHints` accents each key and dims each label, so the pairs are split
/// here rather than re-parsed from a flat string at render time.
pub const KEY_HINTS: &[(&str, &str)] = &[
    ("type", "search"),
    ("↑↓", "move"),
    ("Enter", "actions"),
    ("^P", "commands"),
    ("?", "help"),
    ("^C", "quit"),
];

/// A compact hint set for NARROW terminals, where [`KEY_HINTS`] would overflow.
/// Keeps the essentials (move, actions, help, quit) and the `?` pointer to the
/// full keymap — so nothing is hidden, just deferred to the help overlay.
pub const KEY_HINTS_SHORT: &[(&str, &str)] = &[
    ("↑↓", "move"),
    ("Enter", "actions"),
    ("?", "help"),
    ("^C", "quit"),
];

/// Which screen the TUI is showing. Help is a modal overlay; in Help mode the
/// navigation/query keys are inert so the overlay can't be typed "through".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Search,
    Help,
    CommandPalette,
    EditMetadata,
    PlaylistPicker,
    /// The small action menu shown when Enter is pressed on a selected script,
    /// offering Open/Edit, Run, Delete, and Cancel. ↑/↓ (or j/k) move the
    /// highlight and Enter activates it; digits 1–4 stay direct picks; `c`/`q`/Esc
    /// close it. Owns all these keys so nothing reaches the search query while open.
    ActionMenu,
    /// A y/n confirmation for a pending delete (suite-ui `ConfirmModal`). Entered
    /// from the action menu's Delete row; only an explicit `y` emits the delete
    /// intent — `n`/`c`/`q`/Esc cancel. Every other key is inert so the modal can't
    /// be typed through: a file can never be removed by a single (mis)keystroke.
    ConfirmDelete,
    /// One-field text entry to name the current query before saving it (Phase 3).
    SaveSearchName,
    /// List picker to recall (or delete) a saved search by name (Phase 3).
    SavedSearchPicker,
}

/// The current "browse" filter for the main list (when query is empty or as
/// overlay on search results for daily narrowing). All is default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    All,
    Favorites,
    Recents,
}

/// What a palette entry does when selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    Act(ActionKind),
    SetView(ViewMode),
    Special(SpecialCmd),
}

/// A non-action, non-view palette command. A fieldless enum (was a `&'static str`
/// matched in `dispatch_special`) so the dispatch is EXHAUSTIVE: adding a variant
/// won't compile until it's handled, and a mistyped command is now impossible —
/// the old string form silently fell through a catch-all. The renderer never
/// shows these names (it shows the `PaletteCmd::label`), so they carry no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialCmd {
    Reload,
    NewPlaylist,
    AddToPlaylist,
    ActivatePlaylist,
    DeletePlaylist,
    ClearPlaylistFilter,
    ShowLastOutput,
    SaveSearch,
    LoadSavedSearch,
    RunCapture,
    RunLive,
    ToggleOutput,
    EditMetadata,
    ClearQuery,
    Help,
    Quit,
}

/// One entry in the command palette (label is both display and match text).
#[derive(Debug, Clone)]
pub struct PaletteCmd {
    pub label: &'static str,
    pub desc: &'static str,
    pub action: PaletteAction,
}

/// Which action the user requested on the selected entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    OpenEditor,
    Run,
    CopyPath,
    PrintPath,
    ToggleFavorite,
    /// Delete the selected script file from disk, then rebuild the index. Reached
    /// only via the Enter action menu (option 3) AND a y/n confirm modal, never a
    /// bare key — so it can't fire by accident and never clashes with typing
    /// into the search box.
    Delete,
}

/// The result of handling a key: an intent for the shell to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quit,
    Act(ActionKind),
}

/// The full interactive state.
pub struct App {
    /// The search engine (owns the index). The TUI only ever calls `search`.
    scriptvault: ScriptVault,
    /// Current query string (what the user has typed).
    query: String,
    /// Current results for `query` (recomputed only when the query changes).
    results: Vec<SearchResult>,
    /// Index of the selected result, or `None` when there are no results.
    selected: Option<usize>,
    /// A transient status message (e.g. "copied path", "editor not found").
    /// Empty when there is nothing to report — the persistent [`KEY_HINTS`] are
    /// always shown on their own footer line regardless of this.
    status: String,
    /// Paths the user asked to print (Ctrl-O); flushed to stdout on quit.
    printed_paths: Vec<String>,
    /// The resolved colour palette (honours `NO_COLOR`), read once at startup.
    theme: Theme,
    /// Current screen (search list vs. help overlay).
    mode: Mode,
    /// Highlighted row in the Enter action menu (0..=3 → Open/Edit, Run, Delete,
    /// Cancel). Reset to 0 each time the menu opens. Driven by ↑/↓/j/k; Enter acts
    /// on the highlighted row. Digits 1–4 stay direct picks regardless of it.
    action_menu_selected: usize,
    /// Current view filter for the results list (affects empty-query browse and
    /// can narrow active search results too). Powers the stronger favs/recents UX.
    view_mode: ViewMode,
    /// Palette state (when Mode::CommandPalette).
    palette_query: String,
    palette_selected: Option<usize>,
    /// Optional active playlist filter (Phase 2 org feature). Intersects with ViewMode + tag/cat.
    active_playlist: Option<String>,
    /// All metadata-editor + playlist-picker modal state (see `edit::EditState`):
    /// the script path, the six text buffers, the focus index, and the picker
    /// highlight. Reached only via the `impl App` methods in `edit.rs`.
    edit: edit::EditState,

    // --- Phase 3 saved searches ---
    /// All saved-search modal state (see `saved_search::SaveSearchState`): the
    /// name buffer + captured query for the save modal, and the picker highlight.
    /// Reached only via the `impl App` methods in `saved_search.rs`.
    save_search: saved_search::SaveSearchState,

    // --- Phase 3 live output pane (toggleable tailing log from run-capture or live run) ---
    /// All output-pane state, grouped into one struct (see `output::OutputState`):
    /// visibility, the bounded line buffer, the scroll offset, and the live-run
    /// paths. Accessed only through the `impl App` methods in `output.rs`, so the
    /// grouping is invisible to the renderer and every public accessor is unchanged.
    output: output::OutputState,

    // --- Phase 3 mouse polish: store the exact list pane rect computed during render ---
    /// The outer Rect of the results list widget from the last draw (includes its block).
    /// Used by handle_mouse for pixel-perfect row/col -> index mapping (replaces old hardcoded
    /// approx that broke on narrow/short terminals, search_h=1, etc.). Updated every frame.
    list_rect: Option<Rect>,
}

impl App {
    /// Create the app with an explicit theme, priming it with the full list
    /// (empty query => all). The single constructor: the binary passes a theme
    /// resolved from `--color` + `NO_COLOR` (`Theme::resolve`), and tests pass an
    /// explicit `Theme::with_color(..)` to render/behave deterministically.
    pub fn with_theme(scriptvault: ScriptVault, theme: Theme) -> Self {
        // At startup the query is empty, so this is the "browse" view: favorites
        // first, then recents, then display-name order.
        let results = scriptvault.browse();
        let selected = if results.is_empty() { None } else { Some(0) };
        Self {
            scriptvault,
            query: String::new(),
            results,
            selected,
            // Starts empty: the footer's persistent KEY_HINTS line covers the
            // "what do the keys do?" question, so the transient line stays clean
            // until an action actually has something to report.
            status: String::new(),
            printed_paths: Vec::new(),
            theme,
            mode: Mode::Search,
            action_menu_selected: 0,
            view_mode: ViewMode::default(),
            palette_query: String::new(),
            palette_selected: Some(0),
            active_playlist: None,
            // Phase 2 metadata-editor + playlist-picker state (empty buffers,
            // focus 0, no path/selection) — all captured by EditState's Default.
            edit: edit::EditState::default(),
            // Phase 3 saved-search modal state (inactive until a modal is entered).
            save_search: saved_search::SaveSearchState::default(),
            // Phase 3 output pane state (starts hidden, empty buffer, pinned to
            // tail, no live run) — all captured by OutputState's Default.
            output: output::OutputState::default(),
            list_rect: None,
        }
    }

    // --- read-only accessors for the renderer ------------------------------
    pub fn query(&self) -> &str {
        &self.query
    }
    /// The current screen (Search vs. Help). The renderer reads this to decide
    /// whether to draw the help overlay.
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }
    /// The highlighted row (0..=3) in the Enter action menu, for the renderer to
    /// draw the selection. Meaningful only in `Mode::ActionMenu`.
    pub fn action_menu_selected(&self) -> usize {
        self.action_menu_selected
    }
    pub fn palette_query(&self) -> &str {
        &self.palette_query
    }
    pub fn palette_selected(&self) -> Option<usize> {
        self.palette_selected
    }
    pub fn active_playlist(&self) -> Option<&str> {
        self.active_playlist.as_deref()
    }
    pub fn set_active_playlist(&mut self, name: Option<String>) {
        self.active_playlist = name;
        self.refilter();
    }
    pub fn playlists(&self) -> &[scriptvault_core::Playlist] {
        self.scriptvault.playlists()
    }
    pub fn note_for(&self, path: &std::path::Path) -> Option<&str> {
        self.scriptvault.note_for(path)
    }
    /// Compact run-history hint for a row (`▲12× 2h ✓`), or `None` if never run.
    pub fn run_hint_for(&self, path: &std::path::Path) -> Option<String> {
        self.scriptvault.run_hint_for(path)
    }

    /// Labels for the active structured filters in the current query, in order
    /// (e.g. `["t:ci", "lang:bash"]`), for the chip row. Playlist filters are
    /// excluded (shown as a separate playlist indicator). Empty when the query
    /// has no operators.
    pub fn active_chips(&self) -> Vec<String> {
        scriptvault_core::parse_query(&self.query)
            .filters
            .iter()
            .filter_map(|f| f.chip_label())
            .collect()
    }

    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn theme(&self) -> Theme {
        self.theme
    }
}

// Self-contained modal sub-machines, each with its own state struct, kept in
// their own files. The App type, its core accessors, the key router, list
// filtering, and the static palette command list all live in THIS file.
mod edit; // metadata editor + playlist picker (modal forms)
mod output; // live/captured output pane + mouse click-to-select
mod palette; // command-palette state machine + dispatch
mod saved_search; // save-name + recall/delete modals for saved searches

// The renderer (ui.rs) styles output lines by their source stream, so the stream
// tag is re-exported here as part of `app`'s public surface within the crate.
pub use output::OutputStream;

// =============================================================================
// Selected-result helpers and action-facing state — the small surface the
// renderer/effects layer uses to inspect or update App without touching the
// owned ScriptVault.
// =============================================================================
impl App {
    /// The currently selected result, if any. `None` when the list is empty —
    /// callers MUST handle that (it's how action keys safely no-op).
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.selected.and_then(|i| self.results.get(i))
    }

    /// Resolve the editor command: configured editor -> $EDITOR -> "nano".
    pub fn editor_command(&self) -> String {
        if let Some(ed) = &self.scriptvault.config().editor {
            return ed.clone();
        }
        std::env::var("EDITOR").unwrap_or_else(|_| String::from("nano"))
    }

    /// Toggle favorite for a path (delegates to core; persists).
    pub fn toggle_favorite(&mut self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(self.scriptvault.toggle_favorite(path)?)
    }

    /// Record a run with its exit code and optional captured output.
    pub fn record_run_with_status(
        &mut self,
        path: &std::path::Path,
        exit: Option<i32>,
        output: Option<String>,
    ) -> anyhow::Result<()> {
        self.scriptvault
            .record_run_with_status(path, exit, output)?;
        Ok(())
    }

    /// True if a path is favorited (for the row marker / preview).
    pub fn is_favorite(&self, path: &std::path::Path) -> bool {
        self.scriptvault.is_favorite(path)
    }

    /// Human recency + count for a path, if it appears in recents
    /// (e.g. "5m ago (12×) ✓"). Pure math on the stored `last_run`.
    pub fn recency_summary(&self, path: &std::path::Path) -> Option<String> {
        self.scriptvault
            .recents()
            .iter()
            .find(|r| r.path == path)
            .map(|r| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(r.last_run);
                let age = now.saturating_sub(r.last_run);
                let when = if age < 60 {
                    format!("{}s ago", age)
                } else if age < 3600 {
                    format!("{}m ago", age / 60)
                } else if age < 86400 {
                    format!("{}h ago", age / 3600)
                } else {
                    format!("{}d ago", age / 86400)
                };
                let exit_str = match r.last_exit {
                    Some(0) => " ✓".to_string(),
                    Some(c) => format!(" ✗{}", c),
                    None => "".to_string(),
                };
                format!("{} ({}×){}", when, r.count, exit_str)
            })
    }

    /// Set the transient status line.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    /// Queue a path to be printed to stdout after the TUI exits (Ctrl-O).
    pub fn record_printed_path(&mut self, path: String) {
        self.printed_paths.push(path);
    }

    /// Take the queued paths, printed to stdout AFTER the TUI exits so the output
    /// lands in the user's shell and is pipeable (skipped on a panic).
    pub fn take_printed_paths(&mut self) -> Vec<String> {
        std::mem::take(&mut self.printed_paths)
    }
}

// =============================================================================
// Query filtering and list selection. The renderer/effects read results through
// the accessors above; these recompute them and keep the cursor valid.
// =============================================================================
impl App {
    /// Recompute the results list (after a run records a recent, so ordering
    /// updates immediately).
    pub fn refresh_results(&mut self) {
        self.refilter();
    }

    /// Rebuild the index from disk and re-filter, after a file was deleted.
    /// `refilter` re-clamps the selection, so the cursor stays valid for free.
    pub(crate) fn reload_after_delete(&mut self) {
        // Non-fatal: the file is already gone, so a stale row clears next reload.
        let _ = self.scriptvault.reload();
        self.refilter();
    }

    /// Translate the current UI state (query string, view toggle, active
    /// playlist) into a structured [`Query`] for the core engine, which does all
    /// the real work (operator parsing, filtering, matching, ranking).
    fn current_query(&self) -> Query {
        let mut q = scriptvault_core::parse_query(&self.query);
        q.view = match self.view_mode {
            ViewMode::All => View::All,
            ViewMode::Favorites => View::Favorites,
            ViewMode::Recents => View::Recents,
        };
        // An active playlist composes as a filter on top of the view (so
        // "Favorites + playlist" means "in both").
        if let Some(name) = &self.active_playlist {
            q.filters.push(Filter::Playlist(name.clone()));
        }
        q
    }

    /// Re-run the search and fix the selection so it always points at a valid row
    /// (or `None` when empty).
    pub(super) fn refilter(&mut self) {
        self.results = self.scriptvault.query(&self.current_query());
        self.selected = if self.results.is_empty() {
            None
        } else {
            let max = self.results.len() - 1;
            Some(self.selected.unwrap_or(0).min(max))
        };
    }

    /// Move the selection by `delta`, saturating at the ends (no wraparound).
    pub(super) fn move_selection(&mut self, delta: isize) {
        let Some(cur) = self.selected else { return };
        let last = self.results.len().saturating_sub(1);
        let next = (cur as isize + delta).clamp(0, last as isize) as usize;
        self.selected = Some(next);
    }

    /// Jump to the first row (if any).
    pub(super) fn select_first(&mut self) {
        self.selected = if self.results.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Jump to the last row (if any).
    pub(super) fn select_last(&mut self) {
        self.selected = if self.results.is_empty() {
            None
        } else {
            Some(self.results.len() - 1)
        };
    }

    /// Change the view filter and re-filter (clamps selection).
    pub(super) fn set_view(&mut self, m: ViewMode) {
        self.view_mode = m;
        self.refilter();
    }
}

// =============================================================================
// Key handling and modal dispatch. The top-level router and the Enter action
// menu. Effects leave through Outcome; terminal/process work stays outside App.
// =============================================================================

/// Rows moved by PageUp/PageDown. A fixed page keeps paging predictable
/// regardless of terminal height.
const PAGE: isize = 10;

/// Last row index in the Enter action menu (0=Open/edit, 1=Run, 2=Delete,
/// 3=Cancel), used to clamp ↑/↓ navigation.
const ACTION_MENU_LAST: usize = 3;

impl App {
    /// Handle one key press, returning the intent for the shell.
    ///
    /// Two-layer routing: Ctrl-C is hoisted (always quits, from any mode), then a
    /// single exhaustive `match self.mode` dispatches to that mode's handler. The
    /// `match` is deliberate — adding a [`Mode`] variant won't compile until it's
    /// routed here, so a new screen can never be silently unreachable. Each mode's
    /// keys live entirely in its handler (the modal sub-machines in their own
    /// files; Search/Help/ConfirmDelete just below); nothing falls through.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl-C ALWAYS quits, from any mode. Hoisted above all dispatch so the
        // rule lives in one place; the CONTROL modifier means it never clashes
        // with a bare `c` closing an overlay or being typed into a field.
        if ctrl && key.code == KeyCode::Char('c') {
            return Outcome::Quit;
        }

        match self.mode {
            Mode::Search => self.handle_search_key(key, ctrl),
            Mode::Help => self.handle_help_key(key),
            Mode::CommandPalette => self.handle_palette_key(key, ctrl),
            Mode::EditMetadata => {
                self.handle_edit_key(key);
                Outcome::Continue
            }
            Mode::PlaylistPicker => {
                self.handle_playlist_picker_key(key);
                Outcome::Continue
            }
            Mode::ActionMenu => self.handle_action_menu_key(key, ctrl),
            Mode::ConfirmDelete => self.handle_confirm_delete_key(key, ctrl),
            Mode::SaveSearchName => {
                self.handle_save_search_key(key);
                Outcome::Continue
            }
            Mode::SavedSearchPicker => {
                self.handle_saved_search_picker_key(key);
                Outcome::Continue
            }
        }
    }

    /// Help overlay: ?/Esc/c/q close it; every other key is inert so it can't be
    /// typed through. `c`/`q` are the universal single-byte close keys (a lone Esc
    /// can stall in the terminal's escape-sequence parser).
    fn handle_help_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('c') | KeyCode::Char('q') => {
                self.mode = Mode::Search;
                Outcome::Continue
            }
            _ => Outcome::Continue,
        }
    }

    /// Delete confirmation: only an explicit `y` emits the destructive intent;
    /// `n`/Esc/c/q cancel, everything else inert — so a file can never be removed
    /// by a stray keystroke.
    fn handle_confirm_delete_key(&mut self, key: KeyEvent, ctrl: bool) -> Outcome {
        match (key.code, ctrl) {
            (KeyCode::Char('y'), false) => {
                self.mode = Mode::Search;
                self.action_if_selected(ActionKind::Delete)
            }
            (KeyCode::Char('n'), false)
            | (KeyCode::Char('c'), false)
            | (KeyCode::Char('q'), false)
            | (KeyCode::Esc, _) => {
                self.mode = Mode::Search;
                self.set_status("delete cancelled");
                Outcome::Continue
            }
            _ => Outcome::Continue,
        }
    }

    /// The main search-screen keymap. This ordered `match` is the SINGLE source of
    /// truth for key precedence — every arm is one line that either resolves a
    /// trivial key inline or delegates to a small focused helper below. Two
    /// precedence rules are encoded HERE (not in the helpers) so they live in one
    /// place: the `Char(c)` catch-all stays physically last so it can't swallow a
    /// guarded key, and the empty-query guard on `g/G/A/F/R` lives on their arms so
    /// a non-empty query falls through to that catch-all and the letter TYPES.
    fn handle_search_key(&mut self, key: KeyEvent, ctrl: bool) -> Outcome {
        let empty = self.query.trim().is_empty();
        match (key.code, ctrl) {
            // --- meta: quit, help, palette ---
            // On the main search screen Esc quits (in overlays it closes above).
            (KeyCode::Esc, _) => Outcome::Quit,
            (KeyCode::Char('?'), false) => {
                self.mode = Mode::Help;
                Outcome::Continue
            }
            (KeyCode::Char('p'), true) => {
                self.enter_palette();
                Outcome::Continue
            }
            // `:` opens the palette only when the query is empty; once typing, `:`
            // is a literal so operators like `t:ci`/`lang:bash` can be entered.
            (KeyCode::Char(':'), false) if self.query.is_empty() => {
                self.enter_palette();
                Outcome::Continue
            }

            // --- actions: Enter menu + direct ctrl shortcuts ---
            (KeyCode::Enter, _) => self.open_action_menu(),
            (KeyCode::Char('r'), true) => self.action_if_selected(ActionKind::Run),
            (KeyCode::Char('y'), true) => self.action_if_selected(ActionKind::CopyPath),
            (KeyCode::Char('o'), true) => self.action_if_selected(ActionKind::PrintPath),
            (KeyCode::Char('f'), true) => self.action_if_selected(ActionKind::ToggleFavorite),
            // ^L toggles the live/captured output pane (^L avoids clobbering ^O).
            (KeyCode::Char('l'), true) => self.toggle_output_and_report(),

            // --- navigation: arrows/paging/jumps are ALWAYS movement ---
            (KeyCode::Up, false)
            | (KeyCode::Char('k'), false)
            | (KeyCode::Down, false)
            | (KeyCode::Char('j'), false)
            | (KeyCode::PageUp, _)
            | (KeyCode::PageDown, _)
            | (KeyCode::Home, false)
            | (KeyCode::End, false) => self.handle_search_navigation(key),
            // g/G jump first/last — but only when the query is empty; a non-empty
            // 'g'/'G' skips this arm and hits the Char(c) catch-all, so it TYPES.
            (KeyCode::Char('g'), false) | (KeyCode::Char('G'), false) if empty => {
                self.handle_search_navigation(key)
            }

            // --- view switching: A/F/R, only when the query is empty (else type) ---
            (KeyCode::Char('A'), false)
            | (KeyCode::Char('F'), false)
            | (KeyCode::Char('R'), false)
                if empty =>
            {
                self.handle_search_view_switch(key.code)
            }

            // --- query editing: catch-all (Char(c)) stays LAST ---
            (KeyCode::Char('u'), true) => {
                self.query.clear();
                self.refilter();
                Outcome::Continue
            }
            (KeyCode::Backspace, false) => self.handle_search_backspace(),
            (KeyCode::Char(c), false) => {
                self.query.push(c);
                self.refilter();
                Outcome::Continue
            }

            _ => Outcome::Continue,
        }
    }

    /// All search-list movement: arrows / j / k, paging, Home/End, and g/G. The
    /// spine routes g/G here only when the query is empty, so the `Char('g'/'G')`
    /// arms below are reached only in that case; arrows and paging are always
    /// movement. Shift+PageUp/Down scroll the OUTPUT pane instead of the list when
    /// it's shown (inert otherwise, so plain PageUp/Down still page the list).
    /// Always returns `Continue` — movement never produces a shell intent.
    fn handle_search_navigation(&mut self, key: KeyEvent) -> Outcome {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp if shift => {
                if self.is_showing_output() {
                    self.scroll_output(PAGE); // PageUp = older output
                }
            }
            KeyCode::PageDown if shift => {
                if self.is_showing_output() {
                    self.scroll_output(-PAGE); // back toward the tail
                }
            }
            KeyCode::PageUp => self.move_selection(-PAGE),
            KeyCode::PageDown => self.move_selection(PAGE),
            KeyCode::Home | KeyCode::Char('g') => self.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.select_last(),
            _ => {} // unreachable via the spine; a safe no-op keeps this total
        }
        Outcome::Continue
    }

    /// Switch the results view (A=all, F=favorites, R=recents). Reached only when
    /// the query is empty — the spine's guard ensures a non-empty query types the
    /// letter instead.
    fn handle_search_view_switch(&mut self, code: KeyCode) -> Outcome {
        match code {
            KeyCode::Char('A') => self.set_view(ViewMode::All),
            KeyCode::Char('F') => self.set_view(ViewMode::Favorites),
            KeyCode::Char('R') => self.set_view(ViewMode::Recents),
            _ => {} // unreachable via the spine
        }
        Outcome::Continue
    }

    /// Backspace on the search box. When the fuzzy text is empty but operator
    /// chips remain, pop the last chip rather than nibbling a trailing space;
    /// otherwise delete one character. Re-filters either way.
    fn handle_search_backspace(&mut self) -> Outcome {
        if scriptvault_core::parse_query(&self.query).text.is_empty()
            && !self.active_chips().is_empty()
        {
            self.query = scriptvault_core::pop_last_filter_token(&self.query);
        } else {
            self.query.pop();
        }
        self.refilter();
        Outcome::Continue
    }

    /// Open the Enter action menu for the selected script, if any (else hint).
    fn open_action_menu(&mut self) -> Outcome {
        if self.selected_result().is_some() {
            self.mode = Mode::ActionMenu;
            self.action_menu_selected = 0;
        } else {
            self.status = String::from("no result selected");
        }
        Outcome::Continue
    }

    /// Toggle the output pane and report the new visibility on the status line.
    fn toggle_output_and_report(&mut self) -> Outcome {
        self.toggle_output_pane();
        self.set_status(if self.is_showing_output() {
            "output pane on (tail view)"
        } else {
            "output pane off"
        });
        Outcome::Continue
    }

    /// Handle a key while the Enter action menu is open. Two equivalent ways to
    /// pick: ↑/↓ (or j/k) move the highlight and Enter activates it, or a digit
    /// 1–4 picks that row directly. `4`/`c`/`q`/Esc cancel; everything else is
    /// inert. We reset `mode` to Search before returning an `Act` so the post-
    /// action redraw doesn't paint the menu over a restored screen.
    fn handle_action_menu_key(&mut self, key: KeyEvent, ctrl: bool) -> Outcome {
        match (key.code, ctrl) {
            (KeyCode::Up, false) | (KeyCode::Char('k'), false) => {
                self.action_menu_selected = self.action_menu_selected.saturating_sub(1);
                Outcome::Continue
            }
            (KeyCode::Down, false) | (KeyCode::Char('j'), false) => {
                self.action_menu_selected = (self.action_menu_selected + 1).min(ACTION_MENU_LAST);
                Outcome::Continue
            }
            (KeyCode::Enter, _) => self.run_action_menu_row(self.action_menu_selected),
            (KeyCode::Char('1'), false) => self.run_action_menu_row(0),
            (KeyCode::Char('2'), false) => self.run_action_menu_row(1),
            (KeyCode::Char('3'), false) => self.run_action_menu_row(2),
            // Cancel: 4 / c / q / Esc close the menu without acting.
            (KeyCode::Char('4'), false)
            | (KeyCode::Char('c'), false)
            | (KeyCode::Char('q'), false)
            | (KeyCode::Esc, _) => self.run_action_menu_row(3),
            _ => Outcome::Continue,
        }
    }

    /// Activate one action-menu row (0=Open/edit, 1=Run, 2=Delete, 3=Cancel),
    /// shared by Enter and the digit keys. The Delete row stages the
    /// ConfirmDelete modal; every other path leaves `mode` at Search.
    fn run_action_menu_row(&mut self, row: usize) -> Outcome {
        match row {
            0 => {
                self.mode = Mode::Search;
                self.action_if_selected(ActionKind::OpenEditor)
            }
            1 => {
                self.mode = Mode::Search;
                self.action_if_selected(ActionKind::Run)
            }
            2 => {
                // Destructive: stage the confirm modal; the intent only fires from
                // ConfirmDelete on an explicit `y`.
                if self.selected_result().is_some() {
                    self.mode = Mode::ConfirmDelete;
                } else {
                    self.mode = Mode::Search;
                    self.status = String::from("no result selected");
                }
                Outcome::Continue
            }
            _ => {
                self.mode = Mode::Search;
                Outcome::Continue
            }
        }
    }

    /// Emit an action intent only if something is selected; otherwise set a hint
    /// and continue. The empty-results no-op guard.
    fn action_if_selected(&mut self, kind: ActionKind) -> Outcome {
        if self.selected_result().is_some() {
            Outcome::Act(kind)
        } else {
            self.status = String::from("no result selected");
            Outcome::Continue
        }
    }
}

/// The static command-palette list (Phase 1: no dynamic plugins). Order is the
/// default presentation order before fuzzy filtering.
pub fn palette_commands() -> Vec<PaletteCmd> {
    use PaletteAction::*;
    vec![
        PaletteCmd {
            label: "run",
            desc: "run the selected script",
            action: Act(ActionKind::Run),
        },
        PaletteCmd {
            label: "edit",
            desc: "open in editor",
            action: Act(ActionKind::OpenEditor),
        },
        PaletteCmd {
            label: "edit metadata",
            desc: "edit name/desc/tags/etc and notes (in-TUI form)",
            action: Special(SpecialCmd::EditMetadata),
        },
        PaletteCmd {
            label: "copy path",
            desc: "copy path to clipboard",
            action: Act(ActionKind::CopyPath),
        },
        PaletteCmd {
            label: "print path",
            desc: "print path on exit",
            action: Act(ActionKind::PrintPath),
        },
        PaletteCmd {
            label: "toggle fav",
            desc: "star/unstar selected",
            action: Act(ActionKind::ToggleFavorite),
        },
        PaletteCmd {
            label: "view: all",
            desc: "show all scripts",
            action: SetView(ViewMode::All),
        },
        PaletteCmd {
            label: "view: favorites",
            desc: "show only ★ favorites",
            action: SetView(ViewMode::Favorites),
        },
        PaletteCmd {
            label: "view: recents",
            desc: "show recently run",
            action: SetView(ViewMode::Recents),
        },
        PaletteCmd {
            label: "new playlist",
            desc: "create a named group",
            action: Special(SpecialCmd::NewPlaylist),
        },
        PaletteCmd {
            label: "add to playlist",
            desc: "add selected script to a playlist",
            action: Special(SpecialCmd::AddToPlaylist),
        },
        PaletteCmd {
            label: "activate playlist",
            desc: "filter list to a playlist (first available)",
            action: Special(SpecialCmd::ActivatePlaylist),
        },
        PaletteCmd {
            label: "delete playlist",
            desc: "remove a playlist (first available for demo)",
            action: Special(SpecialCmd::DeletePlaylist),
        },
        PaletteCmd {
            label: "clear playlist filter",
            desc: "stop filtering by playlist",
            action: Special(SpecialCmd::ClearPlaylistFilter),
        },
        PaletteCmd {
            label: "show last output",
            desc: "show captured output from last run of selected",
            action: Special(SpecialCmd::ShowLastOutput),
        },
        PaletteCmd {
            label: "save search",
            desc: "save current query as named search",
            action: Special(SpecialCmd::SaveSearch),
        },
        PaletteCmd {
            label: "load saved search",
            desc: "load a saved query (picker if multiple)",
            action: Special(SpecialCmd::LoadSavedSearch),
        },
        PaletteCmd {
            label: "run capture",
            desc: "run and capture output (no live, for log)",
            action: Special(SpecialCmd::RunCapture),
        },
        PaletteCmd {
            label: "run live",
            desc: "run (non-int) and stream output live into the pane",
            action: Special(SpecialCmd::RunLive),
        },
        PaletteCmd {
            label: "toggle output",
            desc: "show/hide the output pane (also ^L)",
            action: Special(SpecialCmd::ToggleOutput),
        },
        PaletteCmd {
            label: "reload",
            desc: "rescan scripts from disk",
            action: Special(SpecialCmd::Reload),
        },
        PaletteCmd {
            label: "clear query",
            desc: "clear the search box",
            action: Special(SpecialCmd::ClearQuery),
        },
        PaletteCmd {
            label: "help",
            desc: "show keybindings",
            action: Special(SpecialCmd::Help),
        },
        PaletteCmd {
            label: "quit",
            desc: "exit the TUI",
            action: Special(SpecialCmd::Quit),
        },
    ]
}

#[cfg(test)]
mod tests;
