// ============================================================================
// crates/scriptvault-core/src/parser/lang.rs
// ============================================================================
// Language inference. Two DISTINCT jobs (conflating them is circular):
//
//   1. leader_language(path, first_line)
//        Uses ONLY the shebang + file extension. Its sole purpose is to pick
//        the comment leader the header scanner uses to FIND `scriptvault.*` lines.
//        It must not depend on parsed metadata, because we have not parsed any
//        metadata yet at that point.
//
//   2. resolve_language(leader_lang, meta.lang)
//        The FINAL language stored on the ScriptEntry. Precedence per the spec:
//            explicit meta.lang  >  shebang  >  extension  >  Unknown
//        Computed AFTER the header+sidecar merge, since meta.lang may come from
//        either source.
//
// Why the split avoids circularity: an annotation is always written using its
// own file's real comment leader (a Python file's `# scriptvault.lang: bash` still
// starts with `#`). So leader-language (from shebang/extension) is always
// enough to read the header; the declared `lang` only changes how we LABEL the
// script afterwards, never how we parse it.
// ============================================================================

use std::path::Path;

use crate::model::Language;

/// Determine the language used to choose a comment leader for header scanning.
/// Shebang wins over extension; falls back to `Unknown`.
///
/// `first_line` is the file's first line (may or may not be a shebang).
pub fn leader_language(path: &Path, first_line: &str) -> Language {
    // 1. Shebang takes precedence — it states the real interpreter regardless
    //    of (or in the absence of) a file extension.
    if let Some(lang) = language_from_shebang(first_line) {
        return lang;
    }
    // 2. Otherwise fall back to the file extension.
    if let Some(lang) = language_from_extension(path) {
        return lang;
    }
    // 3. Nothing recognized.
    Language::Unknown
}

/// The final language for a `ScriptEntry`: an explicit `lang` from metadata
/// wins; otherwise we keep the leader language (shebang/extension/Unknown).
pub fn resolve_language(leader_lang: Language, declared: Option<&str>) -> Language {
    match declared.and_then(language_from_label) {
        Some(lang) => lang,
        None => leader_lang,
    }
}

/// Map an explicit `scriptvault.lang` label (e.g. "bash", "python") to a Language.
/// Delegates to [`Language::from_label`] — the single source of truth for
/// label→Language, shared with the query parser's `lang:` operator so the two
/// can never diverge.
fn language_from_label(label: &str) -> Option<Language> {
    Language::from_label(label)
}

/// Identify the interpreter named in a shebang line.
///
/// Handles both direct (`#!/bin/bash`) and `env` indirection
/// (`#!/usr/bin/env python3`), skips `env` flags like `-S`, takes the program's
/// basename, and strips a trailing version digit (`python3` -> `python`).
fn language_from_shebang(first_line: &str) -> Option<Language> {
    // Must start with the shebang marker.
    let rest = first_line.trim_start().strip_prefix("#!")?;

    // Tokenize on whitespace: ["/usr/bin/env", "python3"] or ["/bin/bash"].
    let mut tokens = rest.split_whitespace();

    // The first token is the executable path; take its basename.
    let first = tokens.next()?;
    let mut prog = basename(first);

    // If the interpreter is `env`, the real program is the next non-flag token
    // (e.g. `env -S python3` -> skip `-S`, take `python3`).
    if prog == "env" {
        prog = tokens.find(|t| !t.starts_with('-')).map(basename)?;
    }

    // Strip a trailing version suffix so `python3`/`python3.11` -> `python`.
    let prog = strip_version_suffix(prog);

    interpreter_to_language(prog)
}

/// Map a bare interpreter name to a Language.
fn interpreter_to_language(prog: &str) -> Option<Language> {
    match prog {
        "bash" | "sh" | "zsh" | "dash" | "ksh" => Some(Language::Bash),
        "python" => Some(Language::Python),
        "node" | "nodejs" | "deno" | "bun" => Some(Language::Node),
        "ruby" => Some(Language::Ruby),
        "lua" | "luajit" => Some(Language::Lua),
        _ => None,
    }
}

/// True if the path has a file extension we recognize as a script language.
///
/// This is the SAME extension set the scanner uses to decide "does this file
/// look like a script worth reading?" — reusing `language_from_extension` so the
/// two never drift (one canonical list of known extensions). A file with no
/// known extension can still be a script via its shebang; the scanner checks
/// that separately by peeking at the first bytes.
pub fn is_known_script_ext(path: &Path) -> bool {
    language_from_extension(path).is_some()
}

/// Map a file extension to a Language.
fn language_from_extension(path: &Path) -> Option<Language> {
    // `extension()` is the part after the final dot, without the dot.
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "sh" | "bash" | "zsh" => Some(Language::Bash),
        "py" => Some(Language::Python),
        "rs" => Some(Language::Rust),
        "js" | "mjs" | "cjs" | "ts" => Some(Language::Node),
        "rb" => Some(Language::Ruby),
        "lua" => Some(Language::Lua),
        "sql" => Some(Language::Sql),
        _ => None,
    }
}

/// The final path component as a `&str` (e.g. "/usr/bin/env" -> "env").
fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// Trim a trailing version run: "python3" -> "python", "python3.11" -> "python".
/// We strip a trailing sequence of digits and dots only.
fn strip_version_suffix(prog: &str) -> &str {
    let trimmed = prog.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    // Guard against an all-digits token (shouldn't happen for interpreters):
    // if trimming emptied it, keep the original.
    if trimmed.is_empty() { prog } else { trimmed }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn shebang_beats_extension() {
        // File named .txt but shebang says python -> Python.
        let lang = leader_language(Path::new("foo.txt"), "#!/usr/bin/env python3");
        assert_eq!(lang, Language::Python);
    }

    #[test]
    fn env_indirection_and_flags() {
        assert_eq!(
            leader_language(Path::new("x"), "#!/usr/bin/env -S python3.11"),
            Language::Python
        );
        assert_eq!(
            leader_language(Path::new("x"), "#!/usr/bin/env node"),
            Language::Node
        );
    }

    #[test]
    fn direct_shebang() {
        assert_eq!(
            leader_language(Path::new("x"), "#!/bin/bash"),
            Language::Bash
        );
        assert_eq!(leader_language(Path::new("x"), "#!/bin/sh"), Language::Bash);
    }

    #[test]
    fn extension_fallback_when_no_shebang() {
        assert_eq!(
            leader_language(Path::new("a.rs"), "fn main(){}"),
            Language::Rust
        );
        assert_eq!(
            leader_language(Path::new("a.sql"), "SELECT 1;"),
            Language::Sql
        );
        assert_eq!(
            leader_language(Path::new("a.py"), "x = 1"),
            Language::Python
        );
    }

    #[test]
    fn unknown_when_nothing_matches() {
        assert_eq!(
            leader_language(Path::new("README"), "hello"),
            Language::Unknown
        );
    }

    #[test]
    fn explicit_lang_overrides_leader() {
        // A .py file (leader Python) that declares bash -> resolved Bash.
        let resolved = resolve_language(Language::Python, Some("bash"));
        assert_eq!(resolved, Language::Bash);
    }

    #[test]
    fn resolve_keeps_leader_when_no_declared_or_unknown_label() {
        assert_eq!(resolve_language(Language::Rust, None), Language::Rust);
        // Unrecognized label -> keep the leader language, don't guess.
        assert_eq!(
            resolve_language(Language::Rust, Some("brainfuck")),
            Language::Rust
        );
    }
}
