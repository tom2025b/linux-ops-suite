// state — persisted user state: favorites, recents, playlists, notes, searches.
// The only mutable, persisted piece of core, behind the `ScriptVault` facade.
// Persistence is JSON at `dirs::data_dir()/scriptvault/state.json`.
//
// Failure philosophy (mirrors the parser): a missing state file is empty state,
// not an error; a malformed file degrades to empty + a warning — a corrupt file
// must never block the app. Mutators do NOT persist; the facade saves after a
// mutation.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The persisted user state. Defaults to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Favorited script paths.
    pub favorites: Vec<PathBuf>,
    /// Recently-run scripts. Kept newest-first by `record_run`.
    pub recents: Vec<RecentEntry>,
    /// User-defined playlists (named groups of script paths). Names are unique.
    pub playlists: Vec<Playlist>,
    /// Per-script personal notes (path -> free text). Not script metadata.
    pub notes: std::collections::HashMap<PathBuf, String>,
    /// Saved searches for quick recall: (name, query) pairs.
    #[serde(default)]
    pub saved_searches: Vec<(String, String)>,
}

// --- persistence ------------------------------------------------------------

impl State {
    /// Load from the standard path. Missing → empty; malformed → empty + warning.
    pub fn load() -> Self {
        match state_file_path() {
            Some(path) => Self::load_from(&path),
            None => State::default(),
        }
    }

    /// Load from a specific path (the test seam). Missing → empty; malformed →
    /// empty + warning.
    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return State::default();
        }
        match std::fs::read_to_string(path) {
            Ok(raw) if raw.trim().is_empty() => State::default(),
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(state) => state,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "ignoring malformed state file");
                    State::default()
                }
            },
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not read state file");
                State::default()
            }
        }
    }

    /// Persist to the standard path. Best-effort; returns an error for the caller
    /// to surface, never panics.
    pub fn save(&self) -> Result<()> {
        match state_file_path() {
            Some(path) => self.save_to(&path),
            None => Ok(()), // no data dir on this OS — skip persistence
        }
    }

    /// Persist to a specific path (test seam). Creates the parent dir.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::error::ScriptVaultError::state_io(
                    format!("failed to create state dir {}", parent.display()),
                    e,
                )
            })?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(crate::error::ScriptVaultError::StateSerialize)?;

        let temp_path = temp_state_path(path);
        let mut file = std::fs::File::create(&temp_path).map_err(|e| {
            crate::error::ScriptVaultError::state_io(
                format!("failed to create temp state file {}", temp_path.display()),
                e,
            )
        })?;
        file.write_all(json.as_bytes()).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            crate::error::ScriptVaultError::state_io(
                format!("failed to write temp state file {}", temp_path.display()),
                e,
            )
        })?;
        file.sync_all().map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            crate::error::ScriptVaultError::state_io(
                format!("failed to sync temp state file {}", temp_path.display()),
                e,
            )
        })?;
        drop(file);

        std::fs::rename(&temp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            crate::error::ScriptVaultError::state_io(
                format!("failed to replace state file {}", path.display()),
                e,
            )
        })?;
        Ok(())
    }
}

/// Same-directory temp file so the final rename is atomic on a single filesystem.
fn temp_state_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state.json".to_string());
    let temp_name = format!(".{name}.tmp-{}", std::process::id());
    path.with_file_name(temp_name)
}

/// The standard state path. Uses `data_dir()` (cross-platform), not `state_dir()`
/// which is `None` on macOS/Windows.
fn state_file_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("scriptvault").join("state.json"))
}

/// Current time as Unix epoch seconds (0 before the epoch, which can't happen but
/// keeps us panic-free).
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// --- favorites --------------------------------------------------------------

impl State {
    /// True if `path` is currently favorited.
    pub fn is_favorite(&self, path: &Path) -> bool {
        self.favorites.iter().any(|p| p == path)
    }

    /// Toggle favorite status. Returns the new state (true = now favorited).
    pub fn toggle_favorite(&mut self, path: &Path) -> bool {
        if let Some(pos) = self.favorites.iter().position(|p| p == path) {
            self.favorites.remove(pos);
            false
        } else {
            self.favorites.push(path.to_path_buf());
            true
        }
    }
}

// --- recents ----------------------------------------------------------------

/// One recently-run script: where, how often, and when last run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// How many times it has been run via ScriptVault.
    pub count: u64,
    /// Last run time as Unix epoch SECONDS (avoids a date-crate dep; recency
    /// sorting only needs ordering).
    pub last_run: u64,
    /// Exit code from the last run; `None` if not reported.
    #[serde(default)]
    pub last_exit: Option<i32>,
    /// Truncated last output (stdout+stderr) if captured. Bounded for low
    /// resource use.
    #[serde(default)]
    pub last_output: Option<String>,
}

/// Cap on a stored output snippet, in BYTES — keeps `state.json` small.
const MAX_OUTPUT_BYTES: usize = 4096;

/// Cap on how many recent entries we retain, newest-first. Every distinct script
/// ever run would otherwise stay forever (each holding up to `MAX_OUTPUT_BYTES`
/// of captured output), so `state.json` would grow without bound and be fully
/// re-serialized on every mutation. A generous window keeps frecency/recents
/// useful while bounding the file.
const MAX_RECENTS: usize = 200;

impl State {
    /// Record a run: bump count, set last_run = now, move to the front.
    pub fn record_run(&mut self, path: &Path) {
        self.record_run_with_status(path, None, None);
    }

    /// Record a run with optional exit code and bounded output for history.
    pub fn record_run_with_status(
        &mut self,
        path: &Path,
        exit: Option<i32>,
        output: Option<String>,
    ) {
        let now = now_secs();
        // Remove any existing entry, carrying its count so the tally accumulates.
        let count = match self.recents.iter().position(|r| r.path == path) {
            Some(pos) => self.recents.remove(pos).count + 1,
            None => 1,
        };
        self.recents.insert(
            0,
            RecentEntry {
                path: path.to_path_buf(),
                count,
                last_run: now,
                last_exit: exit,
                last_output: output.map(|o| bound_output(&o)),
            },
        );
        // Bound the list: the newest `MAX_RECENTS` survive (we just inserted at
        // the front, so truncation drops the oldest). The re-run branch above
        // removed the old copy first, so an existing script only moves to the
        // front — it never pushes the list past the cap on its own.
        self.recents.truncate(MAX_RECENTS);
    }
}

impl RecentEntry {
    /// A compact run-history hint for a results row, e.g. `▲12× 2h ✓` (count, age,
    /// status glyph). `None` with no run history. `now`-injected for deterministic
    /// tests.
    pub fn run_hint(&self, now: u64) -> Option<String> {
        if self.count == 0 {
            return None;
        }
        let age = humanize_age(now.saturating_sub(self.last_run));
        let status = match self.last_exit {
            Some(0) => " ✓",
            Some(_) => " ✗",
            None => "",
        };
        Some(format!("▲{}× {}{}", self.count, age, status))
    }
}

/// Render an age in seconds as a coarse single-unit string (`now`/`9s`/`5m`/`2h`/
/// `3d`/`4w`) — a glanceable hint, not a precise duration.
fn humanize_age(secs: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    match secs {
        0..=4 => "now".to_string(),
        5..=59 => format!("{secs}s"),
        s if s < HOUR => format!("{}m", s / MIN),
        s if s < DAY => format!("{}h", s / HOUR),
        s if s < WEEK => format!("{}d", s / DAY),
        s => format!("{}w", s / WEEK),
    }
}

/// Truncate an output snippet to `MAX_OUTPUT_BYTES` on a char boundary, with a
/// visible marker. Avoids ballooning `state.json` with a chatty script's output.
fn bound_output(o: &str) -> String {
    if o.len() <= MAX_OUTPUT_BYTES {
        return o.to_string();
    }
    let mut s: String = o.chars().take(4000).collect();
    s.push_str("… [truncated]");
    s
}

// --- playlists --------------------------------------------------------------

/// A named user playlist/group of scripts. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

impl State {
    /// Create a playlist. Returns false if the name already exists.
    pub fn create_playlist(&mut self, name: &str) -> bool {
        if self.playlists.iter().any(|p| p.name == name) {
            return false;
        }
        self.playlists.push(Playlist {
            name: name.to_string(),
            paths: Vec::new(),
        });
        true
    }

    /// Delete a playlist by name. Returns true if it was removed.
    pub fn delete_playlist(&mut self, name: &str) -> bool {
        if let Some(pos) = self.playlists.iter().position(|p| p.name == name) {
            self.playlists.remove(pos);
            true
        } else {
            false
        }
    }

    /// Add a path to a playlist (idempotent). Returns true if the playlist existed.
    pub fn add_to_playlist(&mut self, name: &str, path: &Path) -> bool {
        if let Some(p) = self.playlists.iter_mut().find(|pl| pl.name == name) {
            if !p.paths.iter().any(|pp| pp == path) {
                p.paths.push(path.to_path_buf());
            }
            true
        } else {
            false
        }
    }

    /// Remove a path from a playlist. Returns true if the playlist existed (the
    /// path being absent is fine).
    pub fn remove_from_playlist(&mut self, name: &str, path: &Path) -> bool {
        if let Some(p) = self.playlists.iter_mut().find(|pl| pl.name == name) {
            if let Some(idx) = p.paths.iter().position(|pp| pp == path) {
                p.paths.remove(idx);
            }
            true
        } else {
            false
        }
    }

    /// Get a playlist by name.
    pub fn playlist_named(&self, name: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.name == name)
    }
}

// --- notes ------------------------------------------------------------------

impl State {
    /// Set (or overwrite) a personal note for a path. Empty note clears it, so
    /// the map never accumulates blanks.
    pub fn set_note(&mut self, path: &Path, note: &str) {
        if note.trim().is_empty() {
            self.notes.remove(path);
        } else {
            self.notes.insert(path.to_path_buf(), note.to_string());
        }
    }

    /// Get the note for a path, if any.
    pub fn note_for(&self, path: &Path) -> Option<&str> {
        self.notes.get(path).map(|s| s.as_str())
    }
}

// --- saved searches ---------------------------------------------------------

impl State {
    /// Save a query under a name. Overwrites if the name already exists.
    pub fn save_search(&mut self, name: &str, query: &str) {
        if let Some(pos) = self.saved_searches.iter().position(|(n, _)| n == name) {
            self.saved_searches[pos] = (name.to_string(), query.to_string());
        } else {
            self.saved_searches
                .push((name.to_string(), query.to_string()));
        }
    }

    /// List saved-search names in insertion order.
    pub fn list_saved_searches(&self) -> Vec<&str> {
        self.saved_searches
            .iter()
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// Get the query for a saved-search name.
    pub fn get_saved_search(&self, name: &str) -> Option<&str> {
        self.saved_searches
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, q)| q.as_str())
    }

    /// Delete a saved search by name. Returns true if one was removed.
    pub fn delete_saved_search(&mut self, name: &str) -> bool {
        if let Some(pos) = self.saved_searches.iter().position(|(n, _)| n == name) {
            self.saved_searches.remove(pos);
            true
        } else {
            false
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp path per test so parallel runs never collide.
    fn tmp_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("scriptvault-state-{tag}-{nanos}.json"))
    }

    // --- persistence ---

    #[test]
    fn load_missing_file_is_empty_not_error() {
        let path = tmp_path("missing");
        let state = State::load_from(&path);
        assert_eq!(state, State::default());
    }

    #[test]
    fn load_malformed_file_degrades_to_empty() {
        let path = tmp_path("malformed");
        std::fs::write(&path, "{ this is not valid json ").unwrap();
        let state = State::load_from(&path);
        assert_eq!(state, State::default());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = tmp_path("roundtrip");
        let mut s = State::default();
        s.favorites.push(PathBuf::from("/x/deploy.sh"));
        s.save_to(&path).unwrap();
        let loaded = State::load_from(&path);
        assert_eq!(loaded.favorites, vec![PathBuf::from("/x/deploy.sh")]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_load_roundtrips_all_features() {
        let path = tmp_path("allfeatures");
        let mut s = State::default();
        s.create_playlist("ops");
        s.add_to_playlist("ops", Path::new("/x/deploy.sh"));
        s.set_note(Path::new("/x/deploy.sh"), "prod only");
        let r = RecentEntry {
            path: PathBuf::from("/x/deploy.sh"),
            count: 3,
            last_run: 12345,
            last_exit: Some(0),
            last_output: Some("ok".into()),
        };
        s.recents.push(r.clone());
        s.save_to(&path).unwrap();
        let loaded = State::load_from(&path);
        assert_eq!(loaded.playlists.len(), 1);
        assert_eq!(loaded.playlists[0].name, "ops");
        assert_eq!(
            loaded.note_for(Path::new("/x/deploy.sh")),
            Some("prod only")
        );
        assert_eq!(loaded.recents[0].last_exit, Some(0));
        std::fs::remove_file(&path).ok();
    }

    // --- favorites ---

    #[test]
    fn toggle_favorite_adds_then_removes() {
        let mut s = State::default();
        let p = Path::new("/x/a.sh");
        assert!(s.toggle_favorite(p)); // now favorite -> true
        assert!(s.is_favorite(p));
        assert!(!s.toggle_favorite(p)); // toggled off -> false
        assert!(!s.is_favorite(p));
    }

    // --- recents ---

    #[test]
    fn record_run_increments_and_moves_to_front() {
        let mut s = State::default();
        let a = Path::new("/x/a.sh");
        let b = Path::new("/x/b.sh");
        s.record_run(a);
        s.record_run(b);
        s.record_run(a); // a again -> count 2, and a is now newest
        assert_eq!(s.recents.len(), 2);
        assert_eq!(s.recents[0].path, a); // most recent first
        assert_eq!(s.recents[0].count, 2);
        assert_eq!(s.recents[1].path, b);
    }

    #[test]
    fn recents_are_capped_at_max_keeping_the_newest() {
        // Running many DISTINCT scripts must not grow recents without bound.
        let mut s = State::default();
        for i in 0..(MAX_RECENTS + 50) {
            s.record_run(&PathBuf::from(format!("/x/s{i}.sh")));
        }
        assert_eq!(s.recents.len(), MAX_RECENTS, "recents must be capped");
        // The most recently run script is at the front; the oldest are dropped.
        assert_eq!(
            s.recents[0].path,
            PathBuf::from(format!("/x/s{}.sh", MAX_RECENTS + 49)),
            "newest run must be retained at the front"
        );
        assert!(
            !s.recents.iter().any(|r| r.path == Path::new("/x/s0.sh")),
            "the oldest entries must be evicted"
        );
    }

    #[test]
    fn re_running_an_existing_script_does_not_grow_past_cap() {
        // Fill exactly to the cap, then re-run one already-present script. It must
        // move to the front WITHOUT pushing the list over the cap (the re-run
        // branch removes the old copy before inserting).
        let mut s = State::default();
        for i in 0..MAX_RECENTS {
            s.record_run(&PathBuf::from(format!("/x/s{i}.sh")));
        }
        assert_eq!(s.recents.len(), MAX_RECENTS);
        let again = PathBuf::from("/x/s0.sh");
        s.record_run(&again);
        assert_eq!(
            s.recents.len(),
            MAX_RECENTS,
            "re-run must not exceed the cap"
        );
        assert_eq!(
            s.recents[0].path, again,
            "re-run moves the script to the front"
        );
        assert_eq!(s.recents[0].count, 2, "re-run accumulates the count");
    }

    #[test]
    fn record_run_with_status_populates_new_fields() {
        let mut s = State::default();
        let a = Path::new("/x/a.sh");
        s.record_run_with_status(a, Some(0), Some("hello\nworld\n".into()));
        assert_eq!(s.recents.len(), 1);
        let r = &s.recents[0];
        assert_eq!(r.last_exit, Some(0));
        assert_eq!(r.last_output, Some("hello\nworld\n".into()));
        let long = "x".repeat(5000);
        s.record_run_with_status(a, Some(1), Some(long));
        let r2 = &s.recents[0];
        assert_eq!(r2.last_exit, Some(1));
        assert!(r2.last_output.as_ref().unwrap().ends_with("[truncated]"));
    }

    fn entry(count: u64, last_run: u64, last_exit: Option<i32>) -> RecentEntry {
        RecentEntry {
            path: PathBuf::from("/x/a.sh"),
            count,
            last_run,
            last_exit,
            last_output: None,
        }
    }

    #[test]
    fn run_hint_is_none_without_history() {
        assert_eq!(entry(0, 0, None).run_hint(1000), None);
    }

    #[test]
    fn run_hint_formats_count_age_and_status() {
        let now = 1_000_000u64;
        let cases = [
            (12, 2 * 3600, Some(0), "▲12× 2h ✓"),
            (3, 24 * 3600, Some(1), "▲3× 1d ✗"),
            (1, 0, None, "▲1× now"),
            (5, 90, Some(0), "▲5× 1m ✓"),
            (7, 30, None, "▲7× 30s"),
            (2, 8 * 24 * 3600, Some(0), "▲2× 1w ✓"),
        ];
        for (count, age, exit, want) in cases {
            let e = entry(count, now - age, exit);
            assert_eq!(
                e.run_hint(now).as_deref(),
                Some(want),
                "age={age} exit={exit:?}"
            );
        }
    }

    #[test]
    fn run_hint_clock_skew_is_safe() {
        // last_run in the "future" must not underflow; treated as just-now.
        let e = entry(1, 2000, Some(0));
        assert_eq!(e.run_hint(1000).as_deref(), Some("▲1× now ✓"));
    }

    // --- playlists ---

    #[test]
    fn playlist_create_add_remove_roundtrips() {
        let mut s = State::default();
        let p = Path::new("/x/deploy.sh");
        assert!(s.create_playlist("deploy"));
        assert!(!s.create_playlist("deploy")); // duplicate name
        assert!(s.add_to_playlist("deploy", p));
        assert!(s.add_to_playlist("deploy", p)); // idempotent
        let pl = s.playlist_named("deploy").unwrap();
        assert_eq!(pl.paths, vec![p.to_path_buf()]);
        assert!(s.remove_from_playlist("deploy", p));
        assert!(s.delete_playlist("deploy"));
        assert!(s.playlist_named("deploy").is_none());
    }

    // --- notes ---

    #[test]
    fn notes_set_get_clear() {
        let mut s = State::default();
        let p = Path::new("/x/backup.sh");
        assert!(s.note_for(p).is_none());
        s.set_note(p, "remember to check disk space");
        assert_eq!(s.note_for(p), Some("remember to check disk space"));
        s.set_note(p, ""); // clear
        assert!(s.note_for(p).is_none());
    }

    // --- saved searches ---

    #[test]
    fn saved_search_save_overwrite_get_delete() {
        let mut s = State::default();
        assert!(s.get_saved_search("ci").is_none());
        s.save_search("ci", "tag:ci");
        assert_eq!(s.get_saved_search("ci"), Some("tag:ci"));
        s.save_search("ci", "tag:ci deploy"); // overwrite
        assert_eq!(s.get_saved_search("ci"), Some("tag:ci deploy"));
        assert_eq!(s.list_saved_searches(), vec!["ci"]);
        assert!(s.delete_saved_search("ci"));
        assert!(!s.delete_saved_search("ci")); // already gone
        assert!(s.list_saved_searches().is_empty());
    }
}
