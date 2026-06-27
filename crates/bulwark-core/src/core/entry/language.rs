//! Language detection primitives for personal scripts and tools.
//!
//! The enum is intentionally small and explicit. Bulwark detects the common
//! Linux scripting languages it understands well, then falls back to
//! [`Language::Unknown`] rather than guessing.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize, Serializer};

/// Supported languages for personal tools and scripts.
///
/// Language inference uses a conservative precedence order:
/// 1. A recognized shebang on the first line.
/// 2. A recognized file extension.
/// 3. [`Language::Unknown`].
///
/// # Serialized form (stable contract)
/// The string Bulwark emits for a language (the `language` field in
/// `scan --json` / the Workstate feed, and the value users match against in a
/// rule's `languages:` list) is defined by [`Language::as_str`], **not** by the
/// derived `Debug` representation. Serialization routes through `as_str` so the
/// machine contract can never drift just because someone reorders or renames a
/// variant — that now requires editing `as_str` on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum Language {
    Bash,
    Zsh,
    Fish,
    Python,
    Ruby,
    Perl,
    Node,
    /// Rust scripts, cargo scripts, or direct `rustc` compilation.
    Rust,
    /// Bulwark could not confidently determine the language.
    Unknown,
}

impl Language {
    /// The stable, user-facing token for this language.
    ///
    /// This is the single source of truth for how a language appears in
    /// `scan --json`, the Markdown/terminal table, and what a rule's
    /// `languages:` list is compared against (see [`matches_rule_token`]).
    /// It is deliberately spelled out per variant rather than derived from
    /// `Debug`, so the public contract is explicit and stable.
    ///
    /// [`matches_rule_token`]: Language::matches_rule_token
    pub fn as_str(&self) -> &'static str {
        // Intentionally exhaustive (no `_` arm): adding a variant to this
        // #[non_exhaustive] enum must force a deliberate decision about its
        // public token here, rather than silently falling through.
        match self {
            Language::Bash => "Bash",
            Language::Zsh => "Zsh",
            Language::Fish => "Fish",
            Language::Python => "Python",
            Language::Ruby => "Ruby",
            Language::Perl => "Perl",
            Language::Node => "Node",
            Language::Rust => "Rust",
            Language::Unknown => "Unknown",
        }
    }

    /// Whether this detected language should match a rule's `languages:` entry.
    ///
    /// Known languages match case-insensitively (so `Bash`, `bash`, and `BASH`
    /// all work, as the README documents). [`Language::Unknown`] never matches:
    /// it represents the *absence* of a detected language, not a language a user
    /// can target. Allowing `languages: ["unknown"]` to match would let one rule
    /// silently capture every file Bulwark could not classify — a surprising
    /// foot-gun rather than a feature.
    pub fn matches_rule_token(&self, token: &str) -> bool {
        match self {
            Language::Unknown => false,
            known => known.as_str().eq_ignore_ascii_case(token),
        }
    }

    /// Returns the conventional line-comment leader for this language.
    ///
    /// # Examples
    /// ```
    /// use bulwark_core::Language;
    ///
    /// // Shell/Python-family languages use `#`.
    /// assert_eq!(Language::Bash.comment_leader(), "#");
    /// // C-family (Rust, Node) use `//`.
    /// assert_eq!(Language::Rust.comment_leader(), "//");
    /// ```
    pub fn comment_leader(&self) -> &'static str {
        match self {
            Language::Bash
            | Language::Zsh
            | Language::Fish
            | Language::Python
            | Language::Ruby
            | Language::Perl => "#",
            Language::Node | Language::Rust => "//",
            Language::Unknown => "#",
        }
    }

    /// Infer language from a shebang line such as `#!/usr/bin/env python3`.
    ///
    /// Supports both direct interpreters (`#!/bin/bash`) and the common
    /// `/usr/bin/env` wrapper. Returns `None` when the line is not a shebang
    /// or when the interpreter is outside Bulwark's known language set.
    ///
    /// # Examples
    /// ```
    /// use bulwark_core::Language;
    ///
    /// // Direct interpreter.
    /// assert_eq!(Language::from_shebang("#!/bin/bash"), Some(Language::Bash));
    ///
    /// // The `/usr/bin/env` wrapper is unwrapped to the real interpreter.
    /// assert_eq!(
    ///     Language::from_shebang("#!/usr/bin/env python3"),
    ///     Some(Language::Python),
    /// );
    ///
    /// // A plain comment is not a shebang.
    /// assert_eq!(Language::from_shebang("# just a comment"), None);
    /// ```
    pub fn from_shebang(line: &str) -> Option<Self> {
        let line = line.trim();
        if !line.starts_with("#!") {
            return None;
        }

        let shebang = line.trim_start_matches("#!").trim();
        let parts: Vec<&str> = shebang.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        // When the shebang goes through `/usr/bin/env`, the real interpreter is
        // the first *non-flag* argument. Modern kernels support `env -S` (split
        // string) to pass multiple args in one shebang, e.g.
        //   #!/usr/bin/env -S python3 -I
        // Naively taking `parts[1]` would read the interpreter as `-S` and miss
        // the language, so we skip leading `env` flags. `-S`/`--split-string`
        // takes its program inline; other env flags like `-i` or `-u NAME` also
        // appear before the interpreter, so skipping all leading `-`-prefixed
        // tokens lands on the actual interpreter in every common form.
        let interpreter = if parts[0].ends_with("env") {
            parts[1..]
                .iter()
                .copied()
                .find(|arg| !arg.starts_with('-'))?
        } else {
            parts[0]
        };

        let base = Path::new(interpreter)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(interpreter);

        match base {
            "bash" | "sh" => Some(Language::Bash),
            "zsh" => Some(Language::Zsh),
            "fish" => Some(Language::Fish),
            "python" | "python3" | "python2" => Some(Language::Python),
            "ruby" | "ruby3" => Some(Language::Ruby),
            "perl" | "perl5" => Some(Language::Perl),
            "node" | "nodejs" => Some(Language::Node),
            "rustc" | "cargo" => Some(Language::Rust),
            _ => None,
        }
    }

    /// Infer language from the final file extension.
    ///
    /// # Examples
    /// ```
    /// use std::path::Path;
    /// use bulwark_core::Language;
    ///
    /// assert_eq!(Language::from_extension(Path::new("deploy.sh")), Language::Bash);
    /// assert_eq!(Language::from_extension(Path::new("tool.py")), Language::Python);
    ///
    /// // An unrecognized (or missing) extension falls back to `Unknown`.
    /// assert_eq!(Language::from_extension(Path::new("notes.txt")), Language::Unknown);
    /// ```
    pub fn from_extension(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("sh" | "bash") => Language::Bash,
            Some("zsh") => Language::Zsh,
            Some("fish") => Language::Fish,
            Some("py") => Language::Python,
            Some("rb") => Language::Ruby,
            Some("pl" | "pm") => Language::Perl,
            Some("js" | "mjs" | "cjs") => Language::Node,
            Some("rs") => Language::Rust,
            _ => Language::Unknown,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Language {
    /// Serialize as the stable [`as_str`](Language::as_str) token so the JSON /
    /// feed contract is owned by `as_str`, not by the derive macro's field-name
    /// reflection.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_from_shebang() {
        assert_eq!(
            Language::from_shebang("#!/usr/bin/env python3"),
            Some(Language::Python)
        );
        assert_eq!(Language::from_shebang("#!/bin/bash"), Some(Language::Bash));
        assert_eq!(
            Language::from_shebang("#!/usr/bin/env node"),
            Some(Language::Node)
        );
        assert_eq!(Language::from_shebang("# just a comment"), None);
    }

    #[test]
    fn shebang_handles_env_split_string_and_flags() {
        // `env -S` (split string) is the common way to pass multiple args in one
        // shebang; the real interpreter follows the flag, not `-S` itself.
        assert_eq!(
            Language::from_shebang("#!/usr/bin/env -S python3 -I"),
            Some(Language::Python)
        );
        // Multiple leading flags are skipped to reach the interpreter.
        assert_eq!(
            Language::from_shebang("#!/usr/bin/env -S -v node"),
            Some(Language::Node)
        );
        // A bare `env` with no interpreter is not a known language.
        assert_eq!(Language::from_shebang("#!/usr/bin/env"), None);
        assert_eq!(Language::from_shebang("#!/usr/bin/env -S"), None);
    }

    #[test]
    fn detects_language_from_extension() {
        assert_eq!(
            Language::from_extension(Path::new("deploy.sh")),
            Language::Bash
        );
        assert_eq!(
            Language::from_extension(Path::new("tool.py")),
            Language::Python
        );
        assert_eq!(
            Language::from_extension(Path::new("unknown.tool")),
            Language::Unknown
        );
    }

    #[test]
    fn comment_leader_is_correct_for_known_families() {
        assert_eq!(Language::Bash.comment_leader(), "#");
        assert_eq!(Language::Rust.comment_leader(), "//");
    }

    #[test]
    fn as_str_is_the_stable_token_for_every_variant() {
        // `as_str()` is the single source of truth for the user-facing language
        // token (JSON `language`, the table, and rule `languages:` matching). It
        // must be explicit per variant — NOT the derived Debug output — so that
        // adding a variant to this #[non_exhaustive] enum is a deliberate,
        // compile-checked decision instead of silently changing a public token.
        assert_eq!(Language::Bash.as_str(), "Bash");
        assert_eq!(Language::Zsh.as_str(), "Zsh");
        assert_eq!(Language::Fish.as_str(), "Fish");
        assert_eq!(Language::Python.as_str(), "Python");
        assert_eq!(Language::Ruby.as_str(), "Ruby");
        assert_eq!(Language::Perl.as_str(), "Perl");
        assert_eq!(Language::Node.as_str(), "Node");
        assert_eq!(Language::Rust.as_str(), "Rust");
        assert_eq!(Language::Unknown.as_str(), "Unknown");
        // Display delegates to as_str so `{}` and the token never diverge.
        assert_eq!(Language::Python.to_string(), "Python");
    }

    #[test]
    fn matches_rule_token_is_case_insensitive_for_known_languages_only() {
        // Known languages match case-insensitively (README documents `Bash`,
        // `python`, etc.).
        assert!(Language::Bash.matches_rule_token("bash"));
        assert!(Language::Bash.matches_rule_token("BASH"));
        assert!(Language::Python.matches_rule_token("Python"));
        assert!(!Language::Bash.matches_rule_token("python"));

        // `Unknown` is the absence of a detected language, not a language a user
        // can target. A rule `languages: ["unknown"]` must NOT match undetected
        // files — otherwise a single rule silently captures everything Bulwark
        // could not classify (an easy, surprising foot-gun).
        assert!(!Language::Unknown.matches_rule_token("unknown"));
        assert!(!Language::Unknown.matches_rule_token("Unknown"));
    }
}
