//! Pure matching predicates for the Bulwark rule engine.
//!
//! This module contains the low-level, side-effect-free logic that decides
//! whether a given file satisfies a [`MatchSpec`].
//!
//! It is deliberately extracted into its own file so that:
//! - The complex matching rules have one obvious home (a central point of truth).
//! - The logic is easy to unit-test in isolation.
//! - The main `RuleEngine` in `engine.rs` stays focused on orchestration and loading.
//!
//! All functions here are pure: they take simple data and return booleans or
//! normalized strings. No I/O, no mutation, no global state.

use super::types::MatchSpec;
use crate::core::entry::Language;

/// Extract the `(filename, ext, full_path_string)` fields a rule matches on.
///
/// The extension is returned *without* a leading dot (e.g. `"sh"`, not `".sh"`).
/// We normalize to the dotless form here, and [`ext_matches`] normalizes the
/// rule's extensions the same way, so a user can write either `sh` or `.sh` in
/// their YAML and it just works. (Historically the engine required the dot; that
/// silently broke rules written the obvious way.)
pub(crate) fn path_fields(path: &std::path::Path) -> (&str, String, String) {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_string();
    let path_str = path.to_string_lossy().into_owned();
    (filename, extension, path_str)
}

/// Compare a file's (dotless) extension against a rule's declared extensions,
/// tolerating a leading dot on the rule side.
///
/// So `["sh"]`, `[".sh"]`, and a mix all match a file whose extension is `sh`.
/// Comparison is case-sensitive (extensions are conventionally lowercase;
/// we don't second-guess the user).
pub(crate) fn ext_matches(rule_exts: &[String], file_ext: &str) -> bool {
    rule_exts
        .iter()
        .any(|e| e.strip_prefix('.').unwrap_or(e) == file_ext)
}

/// Returns true if `path` is at or under `prefix`, respecting path component
/// boundaries using `/` as the separator.
///
/// - `/usr/bin/ls` matches prefix `/usr/bin`
/// - `/usr/bin` matches prefix `/usr/bin`
/// - `/usr/binary-tools/foo` does **not** match prefix `/usr/bin`
///
/// Why this exists: a naive `str::starts_with` has no concept of a path
/// boundary, so a rule meaning "things in `/usr/bin`" would silently also
/// capture siblings like `/usr/binary-tools` whose names merely *begin* with
/// the prefix. We normalize away a single trailing slash on both sides (so
/// `/usr/bin/` and `/usr/bin` behave identically) and then require the
/// remainder to be empty (exact directory) or start with `/` (a descendant).
pub(crate) fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }

    let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
    let path = path.strip_suffix('/').unwrap_or(path);

    if let Some(rest) = path.strip_prefix(prefix) {
        rest.is_empty() || rest.starts_with('/')
    } else {
        false
    }
}

/// Return true if every populated condition in `spec` matches (AND semantics).
///
/// This is the heart of the rule matching system. A rule only fires when
/// *all* of its specified conditions are satisfied.
pub(crate) fn rule_matches(
    spec: &MatchSpec,
    filename: &str,
    extension: &str,
    is_executable: bool,
    language: Option<Language>,
    path: Option<&str>,
) -> bool {
    if !spec.names.is_empty() && !spec.names.iter().any(|n| n == filename) {
        return false;
    }

    if let Some(want_exec) = spec.executable
        && is_executable != want_exec
    {
        return false;
    }

    if !spec.extensions.is_empty() && !ext_matches(&spec.extensions, extension) {
        return false;
    }

    if !spec.languages.is_empty() {
        // `Language::matches_rule_token` owns the comparison (case-insensitive
        // for known languages, and never matches `Unknown`), so the rule engine
        // and the public `as_str` token can't drift.
        match language {
            Some(lang)
                if spec
                    .languages
                    .iter()
                    .any(|want| lang.matches_rule_token(want)) => {}
            _ => return false,
        }
    }

    if !spec.path_prefixes.is_empty() {
        match path {
            Some(p)
                if spec
                    .path_prefixes
                    .iter()
                    .any(|prefix| path_prefix_matches(p, prefix)) => {}
            _ => return false,
        }
    }

    true
}
