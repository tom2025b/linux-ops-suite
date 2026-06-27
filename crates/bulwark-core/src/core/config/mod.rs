//! Configuration loading for Bulwark.
//!
//! This module is responsible for:
//! - Defining the shape of the user's configuration (as YAML).
//! - Loading the configuration with a strict priority:
//!     1. Explicit path (if provided by CLI later)
//!     2. User XDG config file (~/.config/bulwark/config.yaml or $XDG_CONFIG_HOME/bulwark/config.yaml)
//!     3. Embedded default (config/default.yaml baked into the binary via include_str!)
//! - Performing basic validation and ~/$HOME expansion.
//! - Returning a fully resolved, ready-to-use `Config`.
//!
//! Why this design?
//! - YAML is the single source of truth (per core philosophy).
//! - Read-only and safe: we never write config files in the MVP.
//! - Deterministic: same inputs always produce same resolved paths.
//! - Library-first: all logic here is unit-testable without touching the filesystem
//!   (we provide `from_str` and `from_path` for tests).
//!
//! Error handling: All errors are `BulwarkError` (centralized, thiserror-based).
//! No unwrap/expect/panic in this module outside of tests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config_dir;
use crate::core::scanner::ScanWarning;
use crate::error::BulwarkError;

/// The top-level configuration struct.
///
/// This is what the rest of the system (scanner, engine, reports) will consume.
/// It is intentionally plain and boring — no smart pointers, no interior mutability,
/// just data. That makes it trivial to clone, serialize for --json output, and test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Configuration schema version. Reserved for future migrations.
    pub version: u32,

    /// Where and how to scan for tools/scripts.
    pub scan: ScanConfig,

    /// What to ignore while scanning.
    #[serde(default)]
    pub ignore: IgnoreConfig,
}

/// Controls directory scanning behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanConfig {
    /// Raw path strings from the config file. These may contain `~` or `$HOME`.
    /// Resolved to absolute `PathBuf`s by `resolved_paths()`.
    #[serde(default = "default_scan_paths")]
    pub paths: Vec<String>,

    /// How deep to recurse into each listed path, in `walkdir` terms: depth 0 is
    /// the root directory itself, depth 1 its direct children, and so on. Because
    /// the scan only ever yields regular files (never the root dir), `max_depth:
    /// 1` means "files directly inside each listed directory, no subdirectories".
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Whether to follow symlinks. Default false for safety and to avoid cycles.
    #[serde(default)]
    pub follow_symlinks: bool,
}

// IMPORTANT: these serde field defaults MUST stay in lockstep with the embedded
// `config/default.yaml`. They are what a *partial* user config (one that omits a
// field) falls back to, so if they diverged from the documented defaults, adding
// a single override key would silently change unrelated scan behavior (fewer
// roots, shallower depth, no ignores). The `serde_defaults_match_embedded_yaml`
// test below pins this invariant so the two sources of truth cannot drift.

fn default_scan_paths() -> Vec<String> {
    vec![
        "~/bin".to_string(),
        "~/.local/bin".to_string(),
        "~/scripts".to_string(),
        "~/tools".to_string(),
        "~/dotfiles/bin".to_string(),
    ]
}

fn default_max_depth() -> usize {
    8
}

fn default_ignore_names() -> Vec<String> {
    [
        ".git",
        ".hg",
        ".svn",
        "target",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        ".cargo",
        "dist",
        "build",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Ignore rules applied during directory walking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IgnoreConfig {
    /// Exact names (files or directories) that should be skipped entirely.
    /// Matching is done on the file name component only (not the full path).
    ///
    /// Defaults to the same built-in ignore list as the embedded
    /// `config/default.yaml`, so a partial user config that omits `ignore`
    /// still skips noise directories instead of suddenly walking `.git`,
    /// `node_modules`, etc.
    #[serde(default = "default_ignore_names")]
    pub names: Vec<String>,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            names: default_ignore_names(),
        }
    }
}

/// Embedded default configuration (baked into the binary at compile time).
/// This is the ultimate fallback and also serves as documentation for new users.
const DEFAULT_CONFIG: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config/default.yaml"));

impl Config {
    /// Load configuration using the standard priority order:
    /// user XDG file (if present and valid) → embedded default.
    ///
    /// This is the primary entry point used by the CLI for normal operation.
    pub fn load() -> Result<Self, BulwarkError> {
        if let Some(user_config) = find_user_config_path()
            && user_config.exists()
        {
            // User has a config — try to load it. If it fails, that's a hard error
            // (we don't want to silently fall back and hide their mistakes).
            return Self::load_from_path(&user_config);
        }

        // No user config, or XDG not available → use the baked-in default.
        Self::from_yaml(DEFAULT_CONFIG)
    }

    /// Load configuration from an explicit YAML file path.
    /// Used by `Config::load` (for the discovered user config) and by tests.
    pub fn load_from_path(path: &Path) -> Result<Self, BulwarkError> {
        let content = std::fs::read_to_string(path).map_err(|e| BulwarkError::Path {
            path: path.to_path_buf(),
            message: format!("failed to read config file: {}", e),
        })?;

        Self::from_yaml(&content)
    }

    /// Parse a YAML string into a validated `Config`.
    /// This is the core deserialization + post-processing step.
    ///
    /// We deliberately do **not** call this `from_str` to avoid confusion with
    /// the `std::str::FromStr` trait (clippy::should_implement_trait).
    pub fn from_yaml(yaml: &str) -> Result<Self, BulwarkError> {
        let config: Config = serde_yaml::from_str(yaml)?;

        // Basic sanity validation
        if config.version == 0 {
            return Err(BulwarkError::config(
                "config version must be >= 1 (got 0 or missing)",
            ));
        }

        if config.scan.paths.is_empty() {
            return Err(BulwarkError::config(
                "scan.paths must contain at least one directory",
            ));
        }

        if config.scan.max_depth == 0 {
            return Err(BulwarkError::config("scan.max_depth must be at least 1"));
        }

        Ok(config)
    }

    /// Resolve all scan paths to absolute paths, expanding `~` and `$HOME`.
    ///
    /// This is called by the scanner layer. We do **not** canonicalize (follow symlinks)
    /// here because `follow_symlinks` is a scanner option — we only do lexical expansion.
    ///
    /// Relative paths are rejected. Resolving them against the process working
    /// directory would make the *same* config scan different trees depending on
    /// where `bulwark` happened to be launched from, breaking the project's
    /// determinism guarantee. Configured scan roots must be absolute (or use
    /// `~`); a relative entry is almost always a mistake, so we fail loudly with
    /// an actionable message instead of silently guessing.
    pub fn resolved_scan_paths(&self) -> Result<Vec<PathBuf>, BulwarkError> {
        let mut resolved = Vec::with_capacity(self.scan.paths.len());

        for raw in &self.scan.paths {
            let expanded = expand_tilde(raw)?;
            if !expanded.is_absolute() {
                return Err(BulwarkError::config(format!(
                    "scan path must be absolute or start with '~' (got {raw:?}); \
                     relative paths are rejected because they would scan different \
                     directories depending on the working directory"
                )));
            }
            resolved.push(expanded);
        }

        Ok(resolved)
    }

    /// Resolve the configured scan roots and return those that do not exist on
    /// disk. The scanner treats a missing root as an optional silent skip, so
    /// callers use this to surface the skip to the user.
    pub fn missing_scan_paths(&self) -> Result<Vec<PathBuf>, BulwarkError> {
        Ok(self
            .resolved_scan_paths()?
            .into_iter()
            .filter(|root| !root.exists())
            .collect())
    }

    /// Build a [`ScanWarning`] for each configured scan root that does not exist.
    ///
    /// This is the single source of the "missing scan path" warning. The CLI and
    /// the TUI both call it (at launch and on rescan) so the message text and the
    /// set of warned paths can never drift between entry points.
    pub fn missing_scan_path_warnings(&self) -> Result<Vec<ScanWarning>, BulwarkError> {
        Ok(self
            .missing_scan_paths()?
            .into_iter()
            .map(|path| ScanWarning {
                path: Some(path),
                message: "scan path does not exist, skipping".to_owned(),
            })
            .collect())
    }
}

/// Find the user-specific config file location according to XDG Base Directory spec.
///
/// On Linux this is usually:
///   $XDG_CONFIG_HOME/bulwark/config.yaml   (or ~/.config/bulwark/config.yaml)
///
/// The directory itself comes from the shared [`config_dir`] helper so the
/// `ProjectDirs` qualifier stays defined in exactly one place.
fn find_user_config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.yaml"))
}

/// Expand a leading `~` or `$HOME` to the current user's home directory.
///
/// This is intentionally simple and predictable for the MVP.
/// We do **not** expand arbitrary $ENV vars yet (that can be added later with shellexpand
/// if the need arises, without changing the public API much).
fn expand_tilde(path: &str) -> Result<PathBuf, BulwarkError> {
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").map_err(|_| {
            BulwarkError::config("cannot expand '~': $HOME environment variable is not set")
        })?;
        if home.is_empty() {
            return Err(BulwarkError::config(
                "cannot expand '~': $HOME environment variable is empty",
            ));
        }
        Ok(PathBuf::from(home).join(stripped))
    } else if path == "~" {
        let home = std::env::var("HOME").map_err(|_| {
            BulwarkError::config("cannot expand '~': $HOME environment variable is not set")
        })?;
        Ok(PathBuf::from(home))
    } else {
        // No tilde — return as-is (caller may still make it absolute).
        Ok(PathBuf::from(path))
    }
}

// -----------------------------------------------------------------------------
// Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// We need tempfile only for tests. Add it as a dev-dependency when we run the first test.
    /// For now the tests that need files will be written to expect the crate to compile.

    #[test]
    fn default_config_parses_and_has_reasonable_values() {
        let cfg = Config::from_yaml(DEFAULT_CONFIG).expect("default config must always parse");
        assert_eq!(cfg.version, 1);
        assert!(!cfg.scan.paths.is_empty());
        assert!(cfg.scan.max_depth >= 1);
    }

    #[test]
    fn from_yaml_rejects_empty_paths() {
        let bad = r#"
            version: 1
            scan:
              paths: []
        "#;
        let err = Config::from_yaml(bad).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("scan.paths must contain at least one directory"));
    }

    #[test]
    fn from_yaml_rejects_version_zero() {
        let bad = "version: 0\nscan:\n  paths: ['~/bin']\n";
        let err = Config::from_yaml(bad).unwrap_err();
        assert!(format!("{}", err).contains("config version must be >= 1"));
    }

    #[test]
    fn expand_tilde_works_with_home_set() {
        // Force a known HOME for determinism in tests.
        // (In real code we read the real $HOME.)
        // SAFETY: This is a unit test. We are intentionally mutating process-global
        // state ($HOME) for test isolation. This is a well-known pattern and is
        // acceptable inside #[cfg(test)] modules.
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }

        let p = expand_tilde("~/bin/tools").unwrap();
        assert_eq!(p, PathBuf::from("/home/testuser/bin/tools"));

        let p2 = expand_tilde("~").unwrap();
        assert_eq!(p2, PathBuf::from("/home/testuser"));
    }

    #[test]
    fn serde_defaults_match_embedded_yaml() {
        // A *partial* user config (omitting fields) falls back to the serde
        // field defaults. Those must equal the values in the embedded
        // default.yaml, otherwise overriding one key silently changes unrelated
        // scan behavior. Parse the embedded default and assert our hard-coded
        // serde defaults reproduce it field-for-field.
        let embedded = Config::from_yaml(DEFAULT_CONFIG).unwrap();
        assert_eq!(default_scan_paths(), embedded.scan.paths);
        assert_eq!(default_max_depth(), embedded.scan.max_depth);
        assert_eq!(default_ignore_names(), embedded.ignore.names);

        // And a config that specifies *only* version + a single path keeps the
        // full default ignore list rather than emptying it.
        let partial = Config::from_yaml("version: 1\nscan:\n  paths: ['~/bin']\n").unwrap();
        assert_eq!(partial.ignore.names, embedded.ignore.names);
        assert_eq!(partial.scan.max_depth, embedded.scan.max_depth);
    }

    #[test]
    fn resolved_scan_paths_rejects_relative_paths() {
        // Relative paths would scan different trees depending on the working
        // directory; resolution must fail loudly instead of guessing.
        let cfg = Config::from_yaml("version: 1\nscan:\n  paths: ['relative/dir']\n").unwrap();
        let err = cfg.resolved_scan_paths().unwrap_err();
        assert!(
            format!("{err}").contains("must be absolute"),
            "relative scan path should be rejected, got: {err}"
        );
    }

    #[test]
    fn resolved_scan_paths_accepts_tilde_and_absolute() {
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }
        let cfg =
            Config::from_yaml("version: 1\nscan:\n  paths: ['~/bin', '/usr/local/bin']\n").unwrap();
        let resolved = cfg.resolved_scan_paths().unwrap();
        assert_eq!(
            resolved,
            vec![
                PathBuf::from("/home/testuser/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        );
    }

    #[test]
    fn missing_scan_path_warnings_reports_only_absent_roots() {
        // One root exists, one does not. The warning set must name exactly the
        // missing root with the single canonical message shared by the CLI and TUI.
        let present = tempdir().unwrap();
        let present_path = present.path().to_string_lossy().into_owned();
        let absent_path = present.path().join("does-not-exist");
        let absent_str = absent_path.to_string_lossy().into_owned();

        let cfg = Config::from_yaml(&format!(
            "version: 1\nscan:\n  paths: ['{present_path}', '{absent_str}']\n"
        ))
        .unwrap();

        let warnings = cfg.missing_scan_path_warnings().unwrap();
        assert_eq!(warnings.len(), 1, "only the absent root should warn");
        assert_eq!(warnings[0].path.as_deref(), Some(absent_path.as_path()));
        assert_eq!(warnings[0].message, "scan path does not exist, skipping");
    }

    #[test]
    fn load_from_path_roundtrips() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("my-config.yaml");

        let content = r#"
            version: 1
            scan:
              paths:
                - "~/mytools"
              max_depth: 3
            ignore:
              names:
                - ".git"
        "#;

        std::fs::write(&file, content).unwrap();

        let cfg = Config::load_from_path(&file).unwrap();
        assert_eq!(cfg.scan.max_depth, 3);
        assert_eq!(cfg.ignore.names, vec![".git".to_string()]);
    }
}
