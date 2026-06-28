// lib.rs — the scriptvault-core public API. `ScriptVault` owns the loaded config,
// index, and persisted state, and runs the scan -> parse -> index -> search
// pipeline. The crate carries no UI code, so the CLI and TUI share it.

pub mod model;

// Core returns `Result<T, ScriptVaultError>` (aliased `crate::Result<T>`); the
// binary maps these to user-friendly messages. `anyhow` lives ONLY in the binary.
pub mod error;

// The pipeline stages.
pub mod actions;
pub mod config;
pub mod index;
pub mod parser;
// The structured query engine: Query/Filter/View/Sort + parser, hybrid frecency
// ranking, and the run() pipeline — what makes every frontend a thin renderer.
pub mod query;
pub mod scan;
pub mod state;

// --- re-exports of the public data types -----------------------------------
pub use config::Config;
pub use error::{Result, ScriptVaultError};
pub use model::{Language, MatchField, MetaSource, ScriptEntry, ScriptMetadata, SearchResult};
pub use query::{Filter, Query, RiskLevel, Sort, View, parse_query, pop_last_filter_token};
pub use state::{Playlist, RecentEntry, State};

use crate::index::Index;

/// Owns the loaded config + index + persisted state and runs the pipeline.
/// Construct with `load()`, read with `search`/`query`/`all`, rebuild with
/// `reload`.
pub struct ScriptVault {
    /// The merged configuration (defaults + user overrides).
    config: Config,
    /// The in-memory index of all discovered scripts.
    index: Index,
    /// Persisted user state (favorites, recents, playlists, notes, searches).
    state: crate::state::State,
    /// Where to persist `state`. `None` = the standard path; tests inject a temp
    /// path so they never touch the user's real state file.
    state_path: Option<std::path::PathBuf>,
}

impl ScriptVault {
    /// Run the full pipeline: load config, scan roots, parse candidates, build
    /// the index, load persisted state. Errors only on fatal problems (a
    /// malformed user config); a missing config/state file is fine, and
    /// unparseable individual files are skipped.
    pub fn load() -> Result<Self> {
        Self::load_with_state(Config::load()?, crate::state::State::load())
    }

    /// Build from an explicit `Config`, loading state from the standard path
    /// (the CLI `--root`, fixture-tree tests).
    pub fn load_with(config: Config) -> Result<Self> {
        Self::load_with_state(config, crate::state::State::load())
    }

    /// Build from an explicit `Config` and `State`, persisting to the standard
    /// path. Internal plumbing shared by the other constructors.
    fn load_with_state(config: Config, state: crate::state::State) -> Result<Self> {
        Ok(Self {
            index: Self::build_index(&config)?,
            config,
            state,
            state_path: None,
        })
    }

    /// Like `load_with_state`, but persists state to an explicit `state_path`
    /// instead of the standard location. This is the fully-hermetic test seam:
    /// tests pass a temp file so a `toggle_favorite`/`record_run` save NEVER
    /// touches the user's real `~/.local/share/scriptvault/state.json`.
    pub fn load_with_state_at(
        config: Config,
        state: crate::state::State,
        state_path: std::path::PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            index: Self::build_index(&config)?,
            config,
            state,
            state_path: Some(state_path),
        })
    }

    /// Search the index. An empty/whitespace query is treated as "show all"
    /// (delegated to the index). Results come back sorted by score, best first.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        self.index.search(query)
    }

    /// All indexed entries, unsorted-by-score, for "browse everything" views.
    /// Borrowed (`&[…]`) — no copying; the caller reads in place.
    pub fn all(&self) -> &[ScriptEntry] {
        self.index.entries()
    }

    /// Re-run scan + parse + index against the *current* config (e.g. after the
    /// user adds a new script). Config itself is not re-read; call `load()` again
    /// for that. Re-reads every candidate file — at our scale (a handful of script
    /// dirs) that's fast and keeps the path simple.
    pub fn reload(&mut self) -> Result<()> {
        self.index = Self::build_index(&self.config)?;
        Ok(())
    }

    /// Read-only access to the active configuration (e.g. to show the configured
    /// roots or editor).
    pub fn config(&self) -> &Config {
        &self.config
    }

    // The persisted-state methods (favorites, recents, playlists, notes, run
    // history, saved searches) are a second `impl ScriptVault` block lower in
    // this file. The methods below are the search/reload engine.

    /// Execute a structured [`Query`] against the index + state — the preferred
    /// entrypoint. Composes views, structured filters (tag/lang/fav/risk/…),
    /// multi-term AND matching, and the hybrid frecency ranking, returning ranked
    /// [`SearchResult`]s. `search`/`browse` are thin conveniences over this.
    pub fn query(&self, query: &Query) -> Vec<SearchResult> {
        query::engine::run(&self.index, &self.state, query)
    }

    /// The "browse everything" view (empty query), ordered for humans by the
    /// hybrid ranking: favorites and frequently/recently-run scripts float to the
    /// top, then display-name order. A thin shim over [`query`] with the default
    /// [`Query`], so browse and search share one engine and one ranking.
    pub fn browse(&self) -> Vec<SearchResult> {
        self.query(&Query::default())
    }

    /// Persist state to the configured location: the injected `state_path` if
    /// set (tests), otherwise the standard path. One place so both mutators
    /// honour the test seam.
    fn save_state(&self) -> Result<()> {
        match &self.state_path {
            Some(path) => self.state.save_to(path),
            None => self.state.save(),
        }
    }

    /// scan -> parse -> Index::build, shared by `load` and `reload`.
    fn build_index(config: &Config) -> Result<Index> {
        let paths = scan::walk(config)?;
        let entries = parser::parse_all(&paths);
        Ok(Index::build(entries))
    }
}

// =============================================================================
// The persisted-state surface: favorites, recents, playlists, notes, run
// history, saved searches. Each mutator changes in-memory `state`, then persists
// via `save_state()` (honouring the test `state_path` seam); a persistence error
// is returned but the in-memory change still stands.
// =============================================================================
impl ScriptVault {
    /// True if the given path is favorited.
    pub fn is_favorite(&self, path: &std::path::Path) -> bool {
        self.state.is_favorite(path)
    }

    /// Toggle a path's favorite status and persist. Returns the new state.
    pub fn toggle_favorite(&mut self, path: &std::path::Path) -> Result<bool> {
        let now_fav = self.state.toggle_favorite(path);
        self.save_state()?;
        Ok(now_fav)
    }

    /// Recently-run scripts, newest first.
    pub fn recents(&self) -> &[crate::state::RecentEntry] {
        &self.state.recents
    }

    /// Compact run-history hint for a script's row (e.g. `▲12× 2h ✓`), or `None`
    /// when never run. Formatting lives in core so every frontend matches.
    pub fn run_hint_for(&self, path: &std::path::Path) -> Option<String> {
        let entry = self.state.recents.iter().find(|r| r.path == path)?;
        entry.run_hint(crate::state::now_secs())
    }

    /// All playlists (names + paths).
    pub fn playlists(&self) -> &[crate::state::Playlist] {
        &self.state.playlists
    }

    /// Get a playlist by name (for filtering).
    pub fn playlist_named(&self, name: &str) -> Option<&crate::state::Playlist> {
        self.state.playlist_named(name)
    }

    /// Create a playlist. Returns true if newly created.
    pub fn create_playlist(&mut self, name: &str) -> Result<bool> {
        let created = self.state.create_playlist(name);
        self.save_state()?;
        Ok(created)
    }

    /// Delete a playlist by name. Returns true if removed.
    pub fn delete_playlist(&mut self, name: &str) -> Result<bool> {
        let removed = self.state.delete_playlist(name);
        self.save_state()?;
        Ok(removed)
    }

    /// Add path to named playlist. Returns true if playlist existed.
    pub fn add_to_playlist(&mut self, name: &str, path: &std::path::Path) -> Result<bool> {
        let added = self.state.add_to_playlist(name, path);
        self.save_state()?;
        Ok(added)
    }

    /// Remove path from named playlist. Returns true if playlist existed.
    pub fn remove_from_playlist(&mut self, name: &str, path: &std::path::Path) -> Result<bool> {
        let removed = self.state.remove_from_playlist(name, path);
        self.save_state()?;
        Ok(removed)
    }

    /// Personal note for a script, if any.
    pub fn note_for(&self, path: &std::path::Path) -> Option<&str> {
        self.state.note_for(path)
    }

    /// Set (or clear with empty) a personal note for a script and persist.
    pub fn set_note(&mut self, path: &std::path::Path, note: &str) -> Result<()> {
        self.state.set_note(path, note);
        self.save_state()
    }

    /// Record a run (no exit/output). Persists.
    pub fn record_run(&mut self, path: &std::path::Path) -> Result<()> {
        self.state.record_run(path);
        self.save_state()
    }

    /// Record a run with an exit code and output snippet. Persists.
    pub fn record_run_with_status(
        &mut self,
        path: &std::path::Path,
        exit: Option<i32>,
        output: Option<String>,
    ) -> Result<()> {
        self.state.record_run_with_status(path, exit, output);
        self.save_state()
    }

    pub fn save_search(&mut self, name: &str, query: &str) -> Result<()> {
        self.state.save_search(name, query);
        self.save_state()
    }

    pub fn list_saved_searches(&self) -> Vec<&str> {
        self.state.list_saved_searches()
    }

    pub fn get_saved_search(&self, name: &str) -> Option<&str> {
        self.state.get_saved_search(name)
    }

    pub fn delete_saved_search(&mut self, name: &str) -> Result<bool> {
        let deleted = self.state.delete_saved_search(name);
        self.save_state()?;
        Ok(deleted)
    }
}

// =============================================================================
// Tests — persisted-state methods (hermetic: explicit empty State, temp path).
#[cfg(test)]
mod state_method_tests {
    use super::*;
    use crate::state::State;

    /// A unique temp state path so saves are fully hermetic (never the real
    /// `~/.local/share/scriptvault/state.json`).
    fn tmp_state_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("scriptvault-facade-{tag}-{nanos}.json"))
    }

    /// A vault over an empty config + empty state, persisting to a temp file.
    fn empty_vault(tag: &str) -> (ScriptVault, std::path::PathBuf) {
        let path = tmp_state_path(tag);
        let v = ScriptVault::load_with_state_at(Config::default(), State::default(), path.clone())
            .unwrap();
        (v, path)
    }

    #[test]
    fn facade_toggle_and_is_favorite() {
        let (mut v, path) = empty_vault("toggle");
        let p = std::path::Path::new("/x/a.sh");
        assert!(!v.is_favorite(p));
        v.toggle_favorite(p).unwrap();
        assert!(v.is_favorite(p));
        // It actually persisted to the temp file (not the real one).
        assert!(
            path.exists(),
            "toggle should have written the temp state file"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn facade_record_run_tracks_recents() {
        let (mut v, path) = empty_vault("recents");
        let p = std::path::Path::new("/x/a.sh");
        v.record_run(p).unwrap();
        assert_eq!(v.recents().len(), 1);
        assert_eq!(v.recents()[0].path, p);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn browse_all_orders_favorites_first() {
        use std::fs;
        // Build a real fixture so entries have paths we can favorite.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-browse-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("aaa.sh"), "#!/bin/sh\n# scriptvault.name: aaa\n").unwrap();
        fs::write(dir.join("zzz.sh"), "#!/bin/sh\n# scriptvault.name: zzz\n").unwrap();
        let cfg = Config {
            roots: vec![dir.clone()],
            ..Default::default()
        };
        // Hermetic: persist state inside the temp fixture dir, not the real path.
        let state_path = dir.join("state.json");
        let mut v = ScriptVault::load_with_state_at(cfg, State::default(), state_path).unwrap();

        // Without favorites, "aaa" sorts before "zzz".
        let before = v.browse();
        assert_eq!(before[0].entry.display_name(), "aaa");

        // Favorite "zzz" → it should now lead the browse view.
        let zzz = before
            .iter()
            .find(|r| r.entry.display_name() == "zzz")
            .unwrap()
            .entry
            .path
            .clone();
        v.toggle_favorite(&zzz).unwrap();
        let after = v.browse();
        assert_eq!(
            after[0].entry.display_name(),
            "zzz",
            "favorite should lead browse view"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn facade_playlists_notes_and_richer_recents() {
        let (mut v, path) = empty_vault("phase2");
        let p = std::path::Path::new("/x/deploy.sh");
        // playlists
        assert!(v.create_playlist("ops").unwrap());
        assert!(v.add_to_playlist("ops", p).unwrap());
        assert_eq!(v.playlists().len(), 1);
        assert!(v.note_for(p).is_none());
        v.set_note(p, "check prod first").unwrap();
        assert_eq!(v.note_for(p), Some("check prod first"));
        // richer record
        v.record_run_with_status(p, Some(0), Some("deployed ok".into()))
            .unwrap();
        let r = &v.recents()[0];
        assert_eq!(r.last_exit, Some(0));
        assert_eq!(r.last_output, Some("deployed ok".into()));
        std::fs::remove_file(&path).ok();
    }

    /// The new `query()` entrypoint composes structured filters end-to-end
    /// through the public facade (not just the engine internals).
    #[test]
    fn facade_query_applies_structured_filters() {
        use std::fs;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-query-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("ci.sh"),
            "#!/bin/sh\n# scriptvault.name: ci-runner\n# scriptvault.tags: ci, prod\n",
        )
        .unwrap();
        fs::write(
            dir.join("db.sh"),
            "#!/bin/sh\n# scriptvault.name: db-backup\n# scriptvault.tags: db\n",
        )
        .unwrap();
        let cfg = Config {
            roots: vec![dir.clone()],
            ..Default::default()
        };
        let v =
            ScriptVault::load_with_state_at(cfg, State::default(), dir.join("state.json")).unwrap();

        // A tag filter built straight from the parser ("t:ci") narrows to ci.sh.
        let q = crate::parse_query("t:ci");
        let out = v.query(&q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "ci-runner");

        fs::remove_dir_all(&dir).ok();
    }

    /// `query()` with the default Query equals `browse()` — they share one engine.
    #[test]
    fn facade_query_default_matches_browse() {
        let (v, path) = empty_vault("query-browse");
        assert_eq!(v.query(&Query::default()).len(), v.browse().len());
        std::fs::remove_file(&path).ok();
    }
}
