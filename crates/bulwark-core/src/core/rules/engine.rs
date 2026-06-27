//! The rule engine: applies an ordered list of [`Rule`]s to discovered files.
//!
//! Matching is simple, obvious, and deterministic — rules are evaluated in order
//! and the *last* match wins, so user rules (appended after the defaults) can
//! override built-in classifications. The engine is pure: it never touches the
//! filesystem except in [`RuleEngine::load`], which reads the user rules file.

use std::path::PathBuf;

use super::defaults::{default_rules, safe_default_classification};
use super::matching::{path_fields, rule_matches};
use super::types::{Classification, Rule};
use crate::core::config_dir;
use crate::core::entry::{Language, ScriptEntry};
use crate::core::scanner::DiscoveredFile;
use crate::error::BulwarkError;

#[cfg(test)]
mod tests;

/// The rule engine. Holds an ordered list of rules and applies them to files.
#[derive(Debug, Clone)]
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// Create a new engine from a YAML string containing a list of rules.
    ///
    /// Example YAML shape:
    /// ```yaml
    /// - name: "user-bin-scripts"
    ///   match:
    ///     extensions: [".sh", ".py"]
    ///     executable: true
    ///   classify:
    ///     risk: low
    ///     category: script
    ///     owner: user
    /// ```
    pub fn from_yaml(yaml: &str) -> Result<Self, BulwarkError> {
        let rules: Vec<Rule> = serde_yaml::from_str(yaml)?;
        if rules.is_empty() {
            return Err(BulwarkError::rule("rule list must not be empty"));
        }
        Ok(Self { rules })
    }

    /// Return an engine pre-populated with the built-in default rules.
    ///
    /// The defaults are built as Rust values (see the `defaults` module), so this
    /// is infallible — no parsing, no `expect()`.
    pub fn with_defaults() -> Self {
        Self {
            rules: default_rules(),
        }
    }

    /// Load defaults + user rules from `~/.config/bulwark/rules.yaml` (if present).
    /// User rules are appended (last match wins for overrides).
    pub fn load() -> Result<Self, BulwarkError> {
        let mut engine = Self::with_defaults();

        if let Some(path) = user_rules_path()
            && path.exists()
        {
            let yaml = std::fs::read_to_string(&path).map_err(|e| BulwarkError::Path {
                path: path.clone(),
                message: format!("failed to read user rules file: {e}"),
            })?;
            let additional: Vec<Rule> = serde_yaml::from_str(&yaml)?;
            engine.append(additional);
        }

        Ok(engine)
    }

    fn append(&mut self, additional: Vec<Rule>) {
        self.rules.extend(additional);
    }

    /// Number of rules currently loaded (defaults + any user rules).
    /// Used by `config-check` to confirm the rule set parsed and merged.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    #[cfg(test)]
    pub(crate) fn from_defaults_and_additional(additional: Vec<Rule>) -> Self {
        let mut e = Self::with_defaults();
        e.append(additional);
        e
    }

    /// Classify a single discovered file according to the loaded rules.
    ///
    /// Rules are evaluated in order; the last rule that matches wins. If no rule
    /// matches, a safe default classification (low risk, "unknown") is returned.
    pub fn classify(&self, file: &DiscoveredFile) -> Classification {
        let (filename, extension, path_str) = path_fields(&file.path);
        self.classify_internal(
            filename,
            &extension,
            file.is_executable,
            None,
            Some(&path_str),
        )
    }

    /// Classify using the richer [`ScriptEntry`] (preferred for new code).
    /// This enables language-based matching in addition to filename/exec/extension.
    pub fn classify_entry(&self, entry: &ScriptEntry) -> Classification {
        let (filename, extension, path_str) = path_fields(&entry.discovered.path);

        self.classify_internal(
            filename,
            &extension,
            entry.discovered.is_executable,
            Some(entry.language),
            Some(&path_str),
        )
    }

    fn classify_internal(
        &self,
        filename: &str,
        extension: &str,
        is_executable: bool,
        language: Option<Language>,
        path: Option<&str>,
    ) -> Classification {
        // Start with the safe default. This is the only place in the entire
        // crate that decides "what do we say when nothing matched?"
        // All other code should go through `safe_default_classification()`
        // so we have exactly one point of truth.
        let mut result = safe_default_classification();

        for rule in &self.rules {
            if rule_matches(
                &rule.r#match,
                filename,
                extension,
                is_executable,
                language,
                path,
            ) {
                result = rule.classify.clone();
            }
        }

        result
    }
}

/// Location of the optional user rules file: `~/.config/bulwark/rules.yaml`.
fn user_rules_path() -> Option<PathBuf> {
    Some(config_dir()?.join("rules.yaml"))
}
