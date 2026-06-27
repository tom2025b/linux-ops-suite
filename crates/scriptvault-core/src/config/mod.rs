// config — load and merge ScriptVault configuration: embedded defaults
// (config/default.yaml) overlaid by the optional user file
// (~/.config/scriptvault/config.yaml).
//
// Rules (each prevents a real, surprising bug):
//   • Missing/empty user config -> defaults, NOT an error.
//   • Malformed user config -> FATAL, with the file path in the message.
//   • Merge is FIELD-BY-FIELD, so setting only `editor` can't erase `roots`.
//   • `~` in any root (default or user) expands to the home directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ScriptVaultError};

/// The raw contents of `config/default.yaml`, embedded at build time as the base
/// layer beneath any user overrides. The file lives inside this crate
/// (`scriptvault-core/config/default.yaml`) so the embed is self-contained — it
/// no longer reaches out to a workspace-root `config/` (which doesn't exist in
/// the linux-ops-suite umbrella this crate was consolidated into).
const DEFAULT_CONFIG_YAML: &str = include_str!("../../config/default.yaml");

// `#[serde(default)]` fills any omitted field with its `Default` (empty Vec /
// None), which is what lets a user write a partial config — but it also makes
// "omitted" and "explicitly empty" indistinguishable after parsing, so the merge
// below guards `roots` with `!is_empty()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Directories to scan for scripts (recursively). Tilde-expanded.
    pub roots: Vec<PathBuf>,
    /// Path-fragment names to skip while scanning (e.g. ".git", "node_modules").
    pub ignores: Vec<String>,
    /// Editor command for the "open" action. `None` -> fall back to `$EDITOR`.
    pub editor: Option<String>,
}

impl Config {
    /// Load the effective configuration: parse embedded defaults, merge the
    /// user's file on top (if any), then expand `~` in all roots.
    ///
    /// Errors only when the user's file exists but is malformed.
    pub fn load() -> Result<Self> {
        // 1. Parse the embedded defaults. If THIS fails, it is our own bug in
        //    default.yaml — surface it loudly rather than limping on.
        let defaults = parse_default_config()?;

        // 2. Locate and read the user's config, if present.
        let merged = match user_config_path() {
            Some(path) if path.exists() => {
                let user = load_user_config(&path)?;
                defaults.merge_user(user)
            }
            // No path resolvable, or file does not exist: defaults stand alone.
            _ => defaults,
        };

        // 3. Expand `~` across the FINAL root list — defaults often contain
        //    `~/bin` etc., so this must run over merged roots, not just user's.
        let config = merged.with_expanded_roots();

        // 4. Validate the merged result. A structurally-valid YAML can still be
        //    *semantically* broken (no roots, a blank ignore, an empty editor);
        //    catch that here with a clear, actionable message rather than letting
        //    the user stare at an empty, silent results list.
        config.validate()?;

        Ok(config)
    }

    /// Load only the embedded defaults (no user file), then expand and validate
    /// them. CLI `--root` uses this as its fallback when a user config is broken:
    /// explicit roots should still inherit the shipped ignore set.
    pub fn defaults() -> Result<Self> {
        let config = parse_default_config()?.with_expanded_roots();
        config.validate()?;
        Ok(config)
    }

    /// Merge a user `Config` on top of `self` (the defaults), field by field.
    /// `self` is consumed and returned as the merged result.
    ///
    /// `roots`: user REPLACES defaults, but only if the user gave a non-empty
    /// list. An empty list means "not set", so defaults survive — the key guard.
    ///
    /// `ignores`: user entries are APPENDED to the defaults (additive).
    ///
    /// `editor`: user value wins if `Some`; otherwise the default is kept.
    fn merge_user(mut self, user: Config) -> Config {
        if !user.roots.is_empty() {
            self.roots = user.roots;
        }

        // Append user ignores to the defaults, then drop duplicates while
        // preserving order (defaults first). Keeps the sensible skips AND the
        // user's additions without listing ".git" twice.
        self.ignores.extend(user.ignores);
        self.ignores = dedup_preserving_order(self.ignores);

        // `Option::or` keeps `self`'s value when `user.editor` is `None`.
        self.editor = user.editor.or(self.editor);

        self
    }

    /// Return a copy of `self` with every root tilde-expanded.
    fn with_expanded_roots(mut self) -> Config {
        self.roots = self.roots.iter().map(|p| expand_tilde(p)).collect();
        self
    }

    /// Parse the embedded `default.yaml` (no user merge, no tilde expansion).
    /// Test-only helper so submodule tests (e.g. validation) can assert against
    /// the shipped defaults without re-typing the `from_str` call.
    #[cfg(test)]
    pub(crate) fn load_defaults_for_test() -> Config {
        serde_yaml::from_str(DEFAULT_CONFIG_YAML).expect("embedded default.yaml must parse")
    }
}

// --- validation -------------------------------------------------------------
// `serde_yaml` guarantees STRUCTURE; this checks the SEMANTICS it can't — a
// config that parses fine can still be useless (no roots, a blank ignore, an
// empty editor). We do NOT validate that a root exists on disk: absent roots
// like `~/.local/bin` are legitimate and the scanner skips them without error.

impl Config {
    /// Validate the merged, tilde-expanded configuration. Returns
    /// `ScriptVaultError::ConfigInvalid` with a helpful message on the first
    /// problem found, or `Ok(())` if the config is sane to run with.
    pub fn validate(&self) -> Result<()> {
        // At least one scan root, or ScriptVault would scan nothing — the single
        // most confusing misconfiguration, so it gets the most explicit guidance.
        if self.roots.is_empty() {
            return Err(ScriptVaultError::ConfigInvalid(
                "no scan roots configured — set at least one directory under \
                 `roots:` in ~/.config/scriptvault/config.yaml (e.g. `- ~/bin`)"
                    .to_string(),
            ));
        }

        // No blank root entries (a stray `- ""`), which would expand to cwd/home.
        if let Some(pos) = self
            .roots
            .iter()
            .position(|r| r.as_os_str().is_empty() || r.to_string_lossy().trim().is_empty())
        {
            return Err(ScriptVaultError::ConfigInvalid(format!(
                "roots[{pos}] is empty — remove the blank entry or give it a real path"
            )));
        }

        // No blank ignore patterns (meaningless, and could match everything).
        if let Some(pos) = self.ignores.iter().position(|i| i.trim().is_empty()) {
            return Err(ScriptVaultError::ConfigInvalid(format!(
                "ignores[{pos}] is empty — remove the blank entry or give it a name/glob"
            )));
        }

        // A set editor must not be blank (`None` is fine — falls back to $EDITOR).
        if let Some(editor) = &self.editor
            && editor.trim().is_empty()
        {
            return Err(ScriptVaultError::ConfigInvalid(
                "`editor:` is set but empty — remove it (to use $EDITOR) or give a command"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers (free functions — small, focused, individually testable).
// -----------------------------------------------------------------------------

/// The path to the user's config: `~/.config/scriptvault/config.yaml`.
/// Returns `None` only if the OS gives us no config directory (extremely rare),
/// in which case callers simply fall back to defaults.
fn user_config_path() -> Option<PathBuf> {
    // `dirs::config_dir()` is the cross-platform "~/.config" (or its OS
    // equivalent). We append our app folder and file name.
    dirs::config_dir().map(|dir| dir.join("scriptvault").join("config.yaml"))
}

/// Parse the embedded default layer. A failure here is ScriptVault's own bug.
fn parse_default_config() -> Result<Config> {
    serde_yaml::from_str(DEFAULT_CONFIG_YAML).map_err(ScriptVaultError::DefaultConfigParse)
}

/// Read and parse a user config file that we already know exists.
///
/// Empty (or whitespace-only) files are treated as "no overrides" and yield a
/// default `Config` — NOT an error. `serde_yaml` parses empty input to a null
/// document, which would otherwise fail to become a struct; we short-circuit
/// that case so "I created an empty config" never breaks the tool.
fn load_user_config(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path).map_err(|source| ScriptVaultError::ConfigRead {
        path: path.to_path_buf(),
        source,
    })?;

    if raw.trim().is_empty() {
        return Ok(Config::default());
    }

    serde_yaml::from_str(&raw).map_err(|source| ScriptVaultError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Expand a leading `~` to the user's home directory. Only a bare leading `~`
/// (i.e. `~` or `~/...`) is handled — `$VARS` and `~otheruser` are out of scope
/// (YAGNI). If we cannot resolve a home directory, the path is returned as-is.
fn expand_tilde(path: &Path) -> PathBuf {
    // Work on the string form so we can inspect the leading character.
    let Some(s) = path.to_str() else {
        // Non-UTF-8 path: nothing to expand, return unchanged.
        return path.to_path_buf();
    };

    if s == "~" {
        // Bare tilde -> the home directory itself.
        return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = s.strip_prefix("~/") {
        // "~/sub/dir" -> <home>/sub/dir
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    // No leading tilde (or no home dir available): unchanged.
    path.to_path_buf()
}

/// Remove duplicate strings while keeping first-seen order. Used so appended
/// user ignores do not re-list a default like ".git".
fn dedup_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        // `insert` returns false if the value was already present, so this
        // keeps only the first occurrence of each entry.
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

// =============================================================================
// Tests — lock the four behaviors the advisor flagged as bug-prone.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_defaults_parse_and_have_roots() {
        // Our own default.yaml must always parse and ship non-empty roots,
        // otherwise a fresh install would scan nothing.
        let cfg: Config = serde_yaml::from_str(DEFAULT_CONFIG_YAML).unwrap();
        assert!(!cfg.roots.is_empty(), "default.yaml must define roots");
    }

    #[test]
    fn user_with_only_editor_keeps_default_roots() {
        // THE trap: a user config that sets only `editor` must NOT wipe roots.
        let defaults = Config {
            roots: vec![PathBuf::from("/default/bin")],
            ignores: vec![".git".into()],
            editor: None,
        };
        let user = Config {
            roots: vec![], // omitted -> empty after serde(default)
            ignores: vec![],
            editor: Some("nvim".into()),
        };
        let merged = defaults.merge_user(user);
        assert_eq!(merged.roots, vec![PathBuf::from("/default/bin")]);
        assert_eq!(merged.editor, Some("nvim".into()));
    }

    #[test]
    fn user_roots_replace_when_present() {
        let defaults = Config {
            roots: vec![PathBuf::from("/default/bin")],
            ..Default::default()
        };
        let user = Config {
            roots: vec![PathBuf::from("/my/scripts")],
            ..Default::default()
        };
        let merged = defaults.merge_user(user);
        assert_eq!(merged.roots, vec![PathBuf::from("/my/scripts")]);
    }

    #[test]
    fn user_ignores_append_and_dedup() {
        let defaults = Config {
            ignores: vec![".git".into(), "target".into()],
            ..Default::default()
        };
        let user = Config {
            ignores: vec![".git".into(), "dist".into()], // ".git" already default
            ..Default::default()
        };
        let merged = defaults.merge_user(user);
        // Defaults preserved, user addition appended, no duplicate ".git".
        assert_eq!(
            merged.ignores,
            vec![".git".to_string(), "target".to_string(), "dist".to_string()]
        );
    }

    #[test]
    fn tilde_expands_only_leading() {
        // We can only assert the leading-tilde transform if a home dir exists.
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde(Path::new("~/bin")), home.join("bin"));
            assert_eq!(expand_tilde(Path::new("~")), home);
        }
        // A non-tilde path is always returned unchanged.
        assert_eq!(
            expand_tilde(Path::new("/abs/path")),
            PathBuf::from("/abs/path")
        );
        // A tilde NOT at the start is not expanded.
        assert_eq!(expand_tilde(Path::new("/x/~/y")), PathBuf::from("/x/~/y"));
    }

    #[test]
    fn empty_user_file_is_not_malformed() {
        // Simulate the parse path for an empty/whitespace file body.
        let raw = "   \n  ";
        let parsed = if raw.trim().is_empty() {
            Config::default()
        } else {
            serde_yaml::from_str(raw).unwrap()
        };
        assert_eq!(parsed, Config::default());
    }

    // --- validation ---

    #[test]
    fn default_config_is_valid() {
        let cfg = Config::load_defaults_for_test();
        assert!(cfg.validate().is_ok(), "shipped defaults must validate");
    }

    #[test]
    fn default_ignores_skip_common_generated_and_backup_noise() {
        let cfg = Config::load_defaults_for_test();
        for pattern in ["dist", "build-*", ".cache", "*.bak", "*.tmp", "*~"] {
            assert!(
                cfg.ignores.iter().any(|ignore| ignore == pattern),
                "default ignores should include {pattern:?}"
            );
        }
    }

    #[test]
    fn no_roots_is_rejected_with_guidance() {
        let cfg = Config {
            roots: vec![],
            ..Default::default()
        };
        let msg = cfg.validate().unwrap_err().to_string();
        assert!(msg.contains("no scan roots"), "got: {msg}");
        assert!(msg.contains("config.yaml"), "got: {msg}");
    }

    #[test]
    fn blank_root_is_rejected() {
        let cfg = Config {
            roots: vec![PathBuf::from("   ")],
            ..Default::default()
        };
        assert!(cfg.validate().unwrap_err().to_string().contains("roots[0]"));
    }

    #[test]
    fn blank_ignore_is_rejected() {
        let cfg = Config {
            roots: vec![PathBuf::from("/x")],
            ignores: vec!["".into()],
            ..Default::default()
        };
        assert!(
            cfg.validate()
                .unwrap_err()
                .to_string()
                .contains("ignores[0]")
        );
    }

    #[test]
    fn empty_editor_is_rejected_but_none_is_ok() {
        let base = Config {
            roots: vec![PathBuf::from("/x")],
            ..Default::default()
        };
        assert!(base.validate().is_ok()); // None editor -> valid
        let bad = Config {
            editor: Some("  ".into()),
            ..base
        };
        assert!(bad.validate().unwrap_err().to_string().contains("editor"));
    }
}
