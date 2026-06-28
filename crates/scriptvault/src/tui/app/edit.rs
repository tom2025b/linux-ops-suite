// tui/app/edit.rs — modal forms: the in-TUI metadata editor and playlist picker.
// -----------------------------------------------------------------------------
// Two small modal state machines that overlay the main search list:
//   * EditMetadata — a six-field form (name/desc/tags/usage/category/note) that
//     writes a `<script>.scriptvault.yaml` sidecar and persists the note.
//   * PlaylistPicker — pick which playlist to add the selected script to.
// Both are `impl App` blocks sharing the parent's private fields via `super`.
// The two `handle_*_key` entry points are `pub(super)` because the main
// `handle_key` dispatcher (in mod.rs, the parent) routes to them by mode.

use anyhow::{Context, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Mode};

/// State for the two modal forms in this file — the metadata editor and the
/// playlist picker — grouped out of `App`'s top level. The six text buffers plus
/// `focus` drive the EditMetadata form; `path` is the script being edited;
/// `playlist_picker_selected` is the PlaylistPicker highlight. They cluster here
/// because they are toggled and mutated together and touched ONLY by this file's
/// `impl App` methods. The renderer keeps using the unchanged `edit_name()` …
/// `edit_focus()` / `playlist_picker_selected()` accessors. `Default` (empty
/// buffers, focus 0, no path/selection) matches the old inline init.
#[derive(Debug, Default)]
pub struct EditState {
    /// The script whose metadata is being edited (EditMetadata mode).
    pub path: Option<std::path::PathBuf>,
    pub name: String,
    pub desc: String,
    pub tags: String, // comma separated
    pub usage: String,
    pub category: String,
    pub note: String,
    /// Which field has focus: 0=name,1=desc,2=tags,3=usage,4=category,5=note.
    pub focus: usize,
    /// PlaylistPicker mode: index of the playlist to add the current script to.
    pub playlist_picker_selected: Option<usize>,
}

impl App {
    pub fn playlist_picker_selected(&self) -> Option<usize> {
        self.edit.playlist_picker_selected
    }

    /// Enter playlist picker to choose which playlist to add the selected script to.
    pub fn enter_playlist_picker(&mut self) {
        if self.selected_result().is_none() {
            self.set_status("no result selected");
            return;
        }
        let count = self.scriptvault.playlists().len();
        if count == 0 {
            self.set_status("no playlists; use 'new playlist' first");
            return;
        }
        self.edit.playlist_picker_selected = Some(0);
        self.mode = Mode::PlaylistPicker;
    }

    /// Handle key in PlaylistPicker mode. Modal: every key is consumed here
    /// (unmapped keys are inert), so the caller never needs a consumed/not signal.
    pub(super) fn handle_playlist_picker_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let playlists = self.scriptvault.playlists();
        let n = playlists.len();
        if n == 0 {
            self.mode = Mode::Search;
            return;
        }
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) => {
                self.mode = Mode::Search;
            }
            // A pure pick-list (no text entry), so the universal `c`/`q` close
            // keys apply here like in the action menu. Esc behaves the same.
            (KeyCode::Esc, _) | (KeyCode::Char('c'), false) | (KeyCode::Char('q'), false) => {
                self.mode = Mode::Search;
                self.edit.playlist_picker_selected = None;
            }
            (KeyCode::Enter, _) => {
                if let Some(sel) = self.selected_result().cloned()
                    && let Some(idx) = self.edit.playlist_picker_selected
                    && idx < n
                {
                    let name = playlists[idx].name.clone();
                    let path = sel.entry.path.clone();
                    if let Err(e) = self.scriptvault.add_to_playlist(&name, &path) {
                        self.set_status(format!("add error: {e}"));
                    } else {
                        self.set_status(format!("added to '{}'", name));
                    }
                }
                self.mode = Mode::Search;
                self.edit.playlist_picker_selected = None;
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), false) => {
                if let Some(idx) = self.edit.playlist_picker_selected {
                    self.edit.playlist_picker_selected =
                        Some(if idx == 0 { n - 1 } else { idx - 1 });
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), false) => {
                if let Some(idx) = self.edit.playlist_picker_selected {
                    self.edit.playlist_picker_selected =
                        Some(if idx + 1 >= n { 0 } else { idx + 1 });
                }
            }
            // Modal: ignore anything else (don't leak into search).
            _ => {}
        }
    }

    // --- edit metadata accessors (for renderer, Phase 2) ---
    pub fn edit_name(&self) -> &str {
        &self.edit.name
    }
    pub fn edit_desc(&self) -> &str {
        &self.edit.desc
    }
    pub fn edit_tags(&self) -> &str {
        &self.edit.tags
    }
    pub fn edit_usage(&self) -> &str {
        &self.edit.usage
    }
    pub fn edit_category(&self) -> &str {
        &self.edit.category
    }
    pub fn edit_note(&self) -> &str {
        &self.edit.note
    }
    pub fn edit_focus(&self) -> usize {
        self.edit.focus
    }

    /// Begin editing metadata for the selected script (Phase 2 editor).
    /// Loads current values (from meta + note) into edit buffers.
    pub fn begin_edit_metadata(&mut self) {
        if let Some(sel) = self.selected_result().cloned() {
            let entry = &sel.entry;
            self.edit.path = Some(entry.path.clone());
            self.edit.name = entry
                .meta
                .name
                .clone()
                .unwrap_or_else(|| entry.filename.clone());
            self.edit.desc = entry.meta.desc.clone().unwrap_or_default();
            self.edit.tags = entry.meta.tags.join(", ");
            self.edit.usage = entry.meta.usage.clone().unwrap_or_default();
            self.edit.category = entry.meta.category.clone().unwrap_or_default();
            self.edit.note = self
                .scriptvault
                .note_for(&entry.path)
                .unwrap_or("")
                .to_string();
            self.edit.focus = 0;
            self.mode = Mode::EditMetadata;
        } else {
            self.set_status("no result selected");
        }
    }

    /// Save the current edit buffers to a sidecar YAML and persist note.
    /// Creates/updates <script>.scriptvault.yaml .
    pub fn save_edit_metadata(&mut self) -> anyhow::Result<()> {
        let path = match &self.edit.path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };
        // Build the sidecar mapping from the edit buffers, OMITTING empty fields
        // (see `build_sidecar_mapping`): an empty string would serialize as
        // `desc: ''`, which serde reads back as `Some("")`, clobbering a
        // header-supplied value through the `sidecar.or(header)` merge. Omitting
        // the key leaves it `None` so the header still shows through.
        let mapping = build_sidecar_mapping(
            &self.edit.name,
            &self.edit.desc,
            &self.edit.tags,
            &self.edit.usage,
            &self.edit.category,
        );
        let sidecar = serde_yaml::to_string(&mapping)?;
        let filename = path
            .file_name()
            .ok_or_else(|| anyhow!("cannot derive sidecar name for {}", path.display()))?;
        let sidecar_path =
            path.with_file_name(format!("{}.scriptvault.yaml", filename.to_string_lossy()));
        std::fs::write(&sidecar_path, sidecar)
            .with_context(|| format!("failed to write sidecar {}", sidecar_path.display()))?;
        // Save note separately
        self.scriptvault.set_note(&path, &self.edit.note)?;
        // Clear edit state
        self.edit.path = None;
        self.mode = Mode::Search;
        // Refresh so changes show (reload index to pick up sidecar)
        let _ = self.scriptvault.reload();
        self.refilter();
        let saved_name = sidecar_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| sidecar_path.display().to_string());
        self.set_status(format!("saved metadata to {}", saved_name));
        Ok(())
    }

    pub fn cancel_edit_metadata(&mut self) {
        self.edit.path = None;
        self.mode = Mode::Search;
    }

    /// Handle a key while in EditMetadata mode. Modal: every key is consumed
    /// here (unmapped keys are inert), so the caller never needs a consumed/not
    /// signal — nothing falls through to normal nav/query.
    pub(super) fn handle_edit_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Esc, _) => {
                self.cancel_edit_metadata();
            }
            (KeyCode::Enter, _) => {
                if let Err(e) = self.save_edit_metadata() {
                    self.set_status(format!("save error: {e}"));
                    self.cancel_edit_metadata();
                }
            }
            (KeyCode::Tab, false) => {
                self.edit.focus = (self.edit.focus + 1) % 6;
            }
            (KeyCode::BackTab, false) => {
                self.edit.focus = if self.edit.focus == 0 {
                    5
                } else {
                    self.edit.focus - 1
                };
            }
            (KeyCode::Char(c), false) => {
                let s = match self.edit.focus {
                    0 => &mut self.edit.name,
                    1 => &mut self.edit.desc,
                    2 => &mut self.edit.tags,
                    3 => &mut self.edit.usage,
                    4 => &mut self.edit.category,
                    5 => &mut self.edit.note,
                    _ => return,
                };
                s.push(c);
            }
            (KeyCode::Backspace, false) => {
                let s = match self.edit.focus {
                    0 => &mut self.edit.name,
                    1 => &mut self.edit.desc,
                    2 => &mut self.edit.tags,
                    3 => &mut self.edit.usage,
                    4 => &mut self.edit.category,
                    5 => &mut self.edit.note,
                    _ => return,
                };
                s.pop();
            }
            // Modal: ignore anything else (don't leak into search).
            _ => {}
        }
    }
}

/// Build the sidecar YAML mapping from the edit buffers, OMITTING any empty
/// field. This is the fix for the clobber bug: writing a blank field as
/// `desc: ''` round-trips through serde as `Some("")`, which then overrides a
/// header-supplied value via the parser's `sidecar.or(header)` merge. By leaving
/// the key OUT entirely, the field deserializes to `None`/empty and the header's
/// value still shows through. `tags` is included only when at least one non-empty
/// tag remains (an empty list would likewise wipe header tags). Pure (no `self`,
/// no IO) so it is unit-testable without a TTY.
fn build_sidecar_mapping(
    name: &str,
    desc: &str,
    tags: &str,
    usage: &str,
    category: &str,
) -> serde_yaml::Mapping {
    let mut map = serde_yaml::Mapping::new();

    let mut put = |key: &str, value: &str| {
        let v = value.trim();
        if !v.is_empty() {
            map.insert(
                serde_yaml::Value::String(key.to_string()),
                serde_yaml::Value::String(v.to_string()),
            );
        }
    };
    put("name", name);
    put("desc", desc);
    put("usage", usage);
    put("category", category);

    let parsed_tags: Vec<serde_yaml::Value> = tags
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| serde_yaml::Value::String(s.to_string()))
        .collect();
    if !parsed_tags.is_empty() {
        map.insert(
            serde_yaml::Value::String("tags".into()),
            serde_yaml::Value::Sequence(parsed_tags),
        );
    }

    map
}

#[cfg(test)]
mod tests {
    use super::build_sidecar_mapping;
    use scriptvault_core::ScriptMetadata;

    /// Serialize the mapping the way `save_edit_metadata` does, then parse it back
    /// into the same `ScriptMetadata` the core parser uses — the real round-trip.
    fn roundtrip(
        name: &str,
        desc: &str,
        tags: &str,
        usage: &str,
        category: &str,
    ) -> ScriptMetadata {
        let map = build_sidecar_mapping(name, desc, tags, usage, category);
        let yaml = serde_yaml::to_string(&map).unwrap();
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn empty_fields_are_omitted_so_they_deserialize_to_none() {
        // The regression: only `name` is filled in; the rest are blank. Empty
        // fields must NOT appear as `Some("")` — they must be `None`/empty, so a
        // header-supplied desc/usage/category still shows through the merge.
        let meta = roundtrip("Deploy", "", "", "", "");
        assert_eq!(meta.name.as_deref(), Some("Deploy"));
        assert_eq!(meta.desc, None, "empty desc must be None, not Some(\"\")");
        assert_eq!(meta.usage, None, "empty usage must be None");
        assert_eq!(meta.category, None, "empty category must be None");
        assert!(meta.tags.is_empty(), "empty tags must be an empty list");
    }

    #[test]
    fn populated_fields_round_trip_intact() {
        let meta = roundtrip("Deploy", "ship it", "ci, prod", "deploy.sh [--full]", "ops");
        assert_eq!(meta.name.as_deref(), Some("Deploy"));
        assert_eq!(meta.desc.as_deref(), Some("ship it"));
        assert_eq!(meta.usage.as_deref(), Some("deploy.sh [--full]"));
        assert_eq!(meta.category.as_deref(), Some("ops"));
        // Tags are split/trimmed/emptied here; core lowercases+dedups later.
        assert_eq!(meta.tags, vec!["ci", "prod"]);
    }

    #[test]
    fn whitespace_only_fields_are_treated_as_empty() {
        // A field of just spaces is as good as empty — it must be omitted too.
        let meta = roundtrip("  ", "   ", " , ,, ", "\t", "");
        assert_eq!(meta, ScriptMetadata::default());
    }
}
