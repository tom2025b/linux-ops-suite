// tui/app/palette.rs — the command palette state machine (Mode::CommandPalette).
// -----------------------------------------------------------------------------
// Opening the palette (^P or ':') gives a fuzzy-filterable list of named
// commands (see `palette_commands()` in mod.rs). This file owns: entering and
// leaving the palette, moving/clamping the selection as the user types, and
// `dispatch_palette` — the big match that turns a chosen command into an effect
// (reload, playlist ops, save/load searches, run-capture/live, toggle output…).
//
// `handle_palette_key` and `enter_palette` are `pub(super)` because the parent's
// `handle_key` routes into them; the filtering/selection helpers stay private to
// this file. All of it is `impl App`, reaching App's private fields via `super`.

use crossterm::event::{KeyCode, KeyEvent};
use scriptvault_core::actions as core_actions;

use super::{App, Mode, Outcome, PaletteAction, PaletteCmd, SpecialCmd, palette_commands};

impl App {
    // --- palette key handling (entry point; called by handle_key in the parent) ---

    /// Handle one key while Mode::CommandPalette. Returns the shell Outcome.
    /// Owns its own query + selection (separate from the main search box): Esc
    /// closes, Enter dispatches the highlighted command, letters edit the filter.
    pub(super) fn handle_palette_key(&mut self, key: KeyEvent, ctrl: bool) -> Outcome {
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) => Outcome::Quit,
            (KeyCode::Esc, _) => {
                self.exit_palette();
                Outcome::Continue
            }
            (KeyCode::Enter, _) => {
                // Dispatch the selected command (if any). Its return value flows
                // straight back to the event loop, so an action command can yield
                // Outcome::Act(kind) and actually run via actions::perform.
                if let Some(cmd) = self.selected_palette_cmd() {
                    self.dispatch_palette(cmd)
                } else {
                    self.exit_palette();
                    Outcome::Continue
                }
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), false) => {
                self.move_palette(-1);
                Outcome::Continue
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), false) => {
                self.move_palette(1);
                Outcome::Continue
            }
            (KeyCode::Backspace, false) => {
                self.palette_query.pop();
                self.clamp_palette_selection();
                Outcome::Continue
            }
            (KeyCode::Char(c), false) => {
                self.palette_query.push(c);
                self.clamp_palette_selection();
                Outcome::Continue
            }
            _ => Outcome::Continue,
        }
    }

    // --- palette helpers (live only while mode == CommandPalette) -----------
    pub(super) fn enter_palette(&mut self) {
        self.mode = Mode::CommandPalette;
        self.palette_query.clear();
        self.palette_selected = Some(0);
    }

    /// Close the palette, returning to the search screen — but ONLY if we are
    /// still showing the palette. A command that opened another modal (help, the
    /// save-search / edit-metadata / picker flows) has already set its own mode;
    /// guarding on `CommandPalette` means the single trailing `exit_palette()` in
    /// `dispatch_palette` can't clobber that mode back to Search. Query/selection
    /// are kept for a possible re-open.
    fn exit_palette(&mut self) {
        if self.mode == Mode::CommandPalette {
            self.mode = Mode::Search;
        }
    }

    fn move_palette(&mut self, delta: isize) {
        let cmds = palette_commands();
        let filtered: Vec<_> = self.filtered_palette_cmds(&cmds);
        if filtered.is_empty() {
            self.palette_selected = None;
            return;
        }
        let cur = self.palette_selected.unwrap_or(0);
        let last = filtered.len().saturating_sub(1) as isize;
        let next = (cur as isize + delta).clamp(0, last) as usize;
        // Map back to index in full list? For simplicity we store index in *filtered*.
        self.palette_selected = Some(next);
    }

    fn clamp_palette_selection(&mut self) {
        let cmds = palette_commands();
        let n = self.filtered_palette_cmds(&cmds).len();
        if n == 0 {
            self.palette_selected = None;
        } else if let Some(s) = self.palette_selected {
            self.palette_selected = Some(s.min(n - 1));
        } else {
            self.palette_selected = Some(0);
        }
    }

    /// Currently selected PaletteCmd (after filtering by palette_query).
    fn selected_palette_cmd(&self) -> Option<PaletteCmd> {
        let cmds = palette_commands();
        let filtered = self.filtered_palette_cmds(&cmds);
        self.palette_selected.and_then(|i| filtered.get(i).cloned())
    }

    fn filtered_palette_cmds(&self, all: &[PaletteCmd]) -> Vec<PaletteCmd> {
        let q = self.palette_query.to_lowercase();
        if q.is_empty() {
            return all.to_vec();
        }
        all.iter()
            .filter(|c| c.label.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Execute a palette command and return the resulting [`Outcome`].
    ///
    /// An `Act` command (run / edit / copy / print / fav) closes the palette and
    /// returns `Outcome::Act(kind)`, which the event loop hands to
    /// `actions::perform` — so these behave exactly like their key shortcuts.
    /// `SetView` and every `Special` command do their work in-place and return
    /// `Outcome::Continue`.
    fn dispatch_palette(&mut self, cmd: PaletteCmd) -> Outcome {
        match cmd.action {
            PaletteAction::Act(kind) => {
                // Reuse the real action path instead of duplicating it: close the
                // palette and emit the intent. The event loop already routes
                // Outcome::Act to actions::perform (which may suspend the
                // terminal for run/editor), so the palette gets full parity with
                // the ^R / Enter / ^Y / ^O / ^F shortcuts.
                if self.selected_result().is_some() {
                    self.exit_palette();
                    Outcome::Act(kind)
                } else {
                    self.set_status("no result selected");
                    Outcome::Continue
                }
            }
            PaletteAction::SetView(v) => {
                self.set_view(v);
                self.exit_palette();
                Outcome::Continue
            }
            PaletteAction::Special(cmd) => {
                // Each Special does its work (and sets a status / opens a modal),
                // then we close the palette ONCE here. `exit_palette` only resets
                // mode when still in CommandPalette, so a command that opened
                // another modal (help, save-search, edit-metadata, the pickers) is
                // left in that mode rather than bounced back to Search.
                self.dispatch_special(cmd);
                self.exit_palette();
                // Every Special resolves in-place; none is an action intent.
                Outcome::Continue
            }
        }
    }

    /// Run one [`SpecialCmd`]. A flat, exhaustive table — the `match` has NO
    /// catch-all, so adding a variant won't compile until it's handled here (the
    /// old `&'static str` dispatch silently swallowed typos through a `_` arm).
    /// Per-arm `exit_palette()` is gone: the single caller closes the palette
    /// after this returns (see [`dispatch_palette`]). Arms that open another modal
    /// just set that mode; the caller's guarded `exit_palette` won't undo it.
    fn dispatch_special(&mut self, cmd: SpecialCmd) {
        match cmd {
            SpecialCmd::Reload => match self.scriptvault.reload() {
                Ok(()) => {
                    self.refilter();
                    self.set_status("reloaded");
                }
                Err(e) => self.set_status(format!("reload error: {e}")),
            },
            SpecialCmd::NewPlaylist => {
                // Simple auto-named for Phase 2; user can manage via future editor.
                let name = format!("Playlist {}", self.scriptvault.playlists().len() + 1);
                if let Err(e) = self.scriptvault.create_playlist(&name) {
                    self.set_status(format!("playlist error: {e}"));
                } else {
                    self.set_status(format!("created playlist '{}'", name));
                    self.set_active_playlist(Some(name));
                }
            }
            SpecialCmd::AddToPlaylist => {
                let Some(sel) = self.selected_result().cloned() else {
                    self.set_status("no result selected");
                    return;
                };
                let pls = self.scriptvault.playlists().to_vec(); // clone to avoid borrow
                if pls.is_empty() {
                    let n = "default".to_string();
                    if let Err(e) = self.scriptvault.create_playlist(&n) {
                        self.set_status(format!("create error: {e}"));
                    } else {
                        let _ = self.scriptvault.add_to_playlist(&n, &sel.entry.path);
                        self.set_status("created 'default' and added");
                    }
                } else if pls.len() == 1 {
                    let name = pls[0].name.clone();
                    if let Err(e) = self.scriptvault.add_to_playlist(&name, &sel.entry.path) {
                        self.set_status(format!("add error: {e}"));
                    } else {
                        self.set_status(format!("added to '{}'", name));
                    }
                } else {
                    // Multiple playlists: open the picker. exit_palette() first so
                    // the picker isn't drawn under the (still-open) palette.
                    self.exit_palette();
                    self.enter_playlist_picker();
                }
            }
            SpecialCmd::ActivatePlaylist => {
                let name = self.scriptvault.playlists().first().map(|p| p.name.clone());
                if let Some(n) = name {
                    self.set_active_playlist(Some(n.clone()));
                    self.set_status(format!("filtering to '{}'", n));
                } else {
                    self.set_status("no playlists yet (use 'new playlist')");
                }
            }
            SpecialCmd::DeletePlaylist => {
                let name = self.scriptvault.playlists().first().map(|p| p.name.clone());
                if let Some(n) = name {
                    if let Err(e) = self.scriptvault.delete_playlist(&n) {
                        self.set_status(format!("delete error: {e}"));
                    } else {
                        if self.active_playlist() == Some(n.as_str()) {
                            self.set_active_playlist(None);
                        }
                        self.set_status(format!("deleted playlist '{}'", n));
                    }
                } else {
                    self.set_status("no playlists to delete");
                }
            }
            SpecialCmd::ClearPlaylistFilter => {
                self.set_active_playlist(None);
                self.set_status("cleared playlist filter");
            }
            SpecialCmd::ShowLastOutput => {
                if let Some(sel) = self.selected_result() {
                    if let Some(out) = self.last_output_for(&sel.entry.path) {
                        self.set_status(format!("last out: {}", out));
                    } else {
                        self.set_status("no captured output for this script");
                    }
                } else {
                    self.set_status("no result selected");
                }
            }
            SpecialCmd::SaveSearch => {
                // Open the name-this-search modal (captures the current query).
                self.exit_palette();
                self.enter_save_search();
            }
            SpecialCmd::LoadSavedSearch => {
                // Open the recall/delete picker (lists saved searches by name).
                self.exit_palette();
                self.enter_saved_search_picker();
            }
            SpecialCmd::RunCapture => self.run_capture_selected(),
            SpecialCmd::RunLive => {
                if let Some(sel) = self.selected_result().cloned() {
                    self.clear_live_output();
                    self.output.show = true;
                    self.output.pending_live_run = Some(sel.entry.path.clone());
                    self.set_status("starting live run (streaming to output pane)...");
                } else {
                    self.set_status("no result selected");
                }
            }
            SpecialCmd::ToggleOutput => {
                self.toggle_output_pane();
                self.set_status(if self.is_showing_output() {
                    "output pane on"
                } else {
                    "output pane off"
                });
            }
            SpecialCmd::EditMetadata => {
                self.exit_palette();
                self.begin_edit_metadata();
            }
            SpecialCmd::ClearQuery => {
                self.query.clear();
                self.refilter();
            }
            SpecialCmd::Help => {
                self.mode = Mode::Help; // switch to help (caller's exit won't undo it)
            }
            SpecialCmd::Quit => {
                // A Special can't return Outcome::Quit, so this only hints; the
                // user quits via the normal Esc binding.
                self.set_status("use Esc to quit");
            }
        }
    }

    /// Run the selected script with output captured (no live streaming), feeding
    /// the output pane and recording the run. Uses the SAME core runner as
    /// foreground/live execution so exit codes, timeouts, shebang fallback, and
    /// process cleanup can't drift across launch modes. Extracted out of the
    /// palette dispatch so that table stays a one-line-per-command list.
    fn run_capture_selected(&mut self) {
        let Some(sel) = self.selected_result().cloned() else {
            self.set_status("no result selected");
            return;
        };
        match core_actions::capture(&sel.entry) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = combined_capture_text(&stdout, &stderr);
                let flat = flatten_for_status(&combined);
                let mut snippet = truncate_for_status(&flat, 200);
                if snippet.is_empty() {
                    snippet = "(no output)".to_string();
                }

                self.output.show = true;
                self.clear_live_output();
                for line in stdout.lines() {
                    self.push_output_line(line.to_string());
                }
                for line in stderr.lines() {
                    self.push_stderr_line(line.to_string());
                }

                let stored = (!combined.trim().is_empty()).then_some(snippet.clone());
                let _ = self.record_run_with_status(&sel.entry.path, output.exit_code, stored);
                self.refresh_results();

                if output.timed_out {
                    self.set_status(format!("capture timed out: {snippet}"));
                } else if output.success() {
                    self.set_status(format!("captured: {snippet}"));
                } else {
                    self.set_status(format!(
                        "capture failed (exit {}): {snippet}",
                        output
                            .exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    ));
                }
            }
            Err(e) => self.set_status(format!("capture error: {e}")),
        }
    }
}

/// Join captured stdout/stderr for status/history snippets. The output pane keeps
/// stream tags separately; this flat string is only for status and recents.
fn combined_capture_text(stdout: &str, stderr: &str) -> String {
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => {
            let mut joined = stdout.to_string();
            if !joined.ends_with('\n') {
                joined.push('\n');
            }
            joined.push_str(stderr);
            joined
        }
    }
}

/// Character-safe status truncation. Rust strings are UTF-8, so slicing by byte
/// index can panic in the middle of a multibyte character.
fn truncate_for_status(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

/// Status bars and preview snippets are single-line UI surfaces. Collapse all
/// whitespace so stdout/stderr boundaries stay readable instead of running words
/// together or embedding control newlines in a footer.
fn flatten_for_status(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
