//! High-level orchestration engine for Bulwark.
//!
//! This module is the "glue" layer (application service). It composes the
//! lower-level pieces (scanner, entry enrichment, rule engine) into two
//! convenient, high-level APIs that most consumers should use.
//!
//! The types it produces (`Inventory`, `ClassifiedEntry`) are canonical and
//! re-exported via `core::model` and `app` for the core/app/tui separation.
//!
//! # Data Flow (the big picture)
//! ```text
//! Config
//!   → scanner::scan()                    (raw file discovery)
//!   → ScriptEntry::from_discovered()     (language + description + sidecar)
//!   → RuleEngine::classify_entry()       (apply YAML rules)
//!   → Vec<ClassifiedEntry>               (final result)
//! ```
//!
//! # Two Public APIs
//!
//! - `collect_inventory` — gives you the enriched but unclassified data.
//!   Useful if you want to apply your own rules or do custom processing.
//!
//! - `collect_classified_inventory` — the most common entry point. It runs
//!   the full pipeline including the built-in default rules + any user rules.
//!
//! This design also makes future UIs (TUI, GUI, web) easy: they just call
//! one of these two functions and get pure data they can render however
//! they want.

use crate::Config;
use crate::core::entry::ScriptEntry;
use crate::core::rules::{Classification, RuleEngine};
use crate::core::scanner::{self, ScanWarning};
use crate::error::BulwarkError;

/// The result of running a full inventory scan (before classification).
///
/// This is the output of `collect_inventory`. It contains every file we
/// discovered, turned into rich `ScriptEntry` objects (with language,
/// description, sidecar metadata, etc.), already sorted by path.
#[derive(Debug)]
pub struct Inventory {
    pub entries: Vec<ScriptEntry>,

    /// Non-fatal problems encountered during the scan (e.g. an unreadable
    /// directory). Empty on a clean scan. Carried up from the scanner so
    /// callers can honestly report a partial inventory instead of presenting
    /// it as complete.
    pub warnings: Vec<ScanWarning>,
}

/// Run the full discovery + enrichment pipeline using the given configuration.
///
/// This is the primary high-level API most consumers (CLI, future TUI, etc.)
/// should use instead of calling scanner + `ScriptEntry` manually.
///
/// It performs these steps:
/// 1. Resolve and walk the configured paths (respecting depth and ignores).
/// 2. For every regular file found, create a rich `ScriptEntry`
///    (language inference, header extraction, optional sidecar).
///
/// The resulting `Inventory` is always sorted by path (determinism guarantee).
pub fn collect_inventory(config: &Config) -> Result<Inventory, BulwarkError> {
    let outcome = scanner::scan(config)?;

    let entries: Vec<ScriptEntry> = outcome
        .files
        .into_iter()
        .map(ScriptEntry::from_discovered)
        .collect();

    Ok(Inventory {
        entries,
        warnings: outcome.warnings,
    })
}

/// A fully processed and classified item from a scan.
///
/// This is the "final answer" type that most users of the library care about.
/// It pairs the rich file metadata (`ScriptEntry`) with the result of
/// applying your rules (`Classification`).
#[derive(Debug, Clone)]
pub struct ClassifiedEntry {
    pub entry: ScriptEntry,
    pub classification: Classification,
}

/// The result of running the full classify pipeline: every classified entry
/// plus any non-fatal scan warnings.
///
/// We return this instead of a bare `Vec<ClassifiedEntry>` so the warnings the
/// scanner collected (e.g. an unreadable directory) reach the presentation
/// layer. The CLI prints them to stderr; the TUI shows a count in its status
/// bar. Either way the user is told when the inventory is partial.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedInventory {
    /// Classified entries, in the same path-sorted order as the scan.
    pub entries: Vec<ClassifiedEntry>,

    /// Non-fatal problems encountered during the scan. Empty on a clean run.
    pub warnings: Vec<ScanWarning>,
}

/// Run the full pipeline including classification using the built-in default rules.
///
/// This is the most convenient high-level entry point for most use cases.
/// It internally calls `collect_inventory` and then applies `RuleEngine::load()`
/// (which loads the defaults + any user `rules.yaml`).
///
/// Returns a [`ClassifiedInventory`] carrying both the classified entries and
/// any non-fatal scan warnings.
///
/// # Why we load the RuleEngine here (design rationale)
/// - For the common case, people want "just give me the classified results."
/// - By loading the engine inside this function we keep the API extremely
///   simple: one call, one `Config`, done.
/// - If you need more control (different rules, custom engine, etc.) you can
///   call `collect_inventory` yourself and then use a `RuleEngine` directly.
pub fn collect_classified_inventory(config: &Config) -> Result<ClassifiedInventory, BulwarkError> {
    let inventory = collect_inventory(config)?;
    let engine = RuleEngine::load()?;

    let mut warnings = inventory.warnings;
    let entries = inventory
        .entries
        .into_iter()
        .map(|entry| {
            // A present-but-malformed sidecar was detected during enrichment but
            // can't carry a warning on its own (ScriptEntry is infallible). Lift
            // it into the inventory warnings here so the user is told their
            // annotation was ignored instead of it vanishing silently.
            if let Some(message) = &entry.sidecar_warning {
                warnings.push(ScanWarning {
                    path: Some(entry.discovered.path.clone()),
                    message: message.clone(),
                });
            }

            let mut classification = engine.classify_entry(&entry);
            // A sidecar (`*.bulwark.yaml`) may intentionally override the
            // rule-engine verdict for its script. Apply it here, after rules,
            // so the user's explicit per-file annotation wins.
            apply_sidecar_override(&entry, &mut classification, &mut warnings);
            ClassifiedEntry {
                entry,
                classification,
            }
        })
        .collect();

    // INVARIANT: `entries` is in the same order as the input inventory
    // (which is already sorted by path). We never re-sort here.
    Ok(ClassifiedInventory { entries, warnings })
}

/// Overlay a script's sidecar metadata onto its rule-derived classification.
///
/// The sidecar (`*.bulwark.yaml`) is the user's explicit, per-file statement of
/// intent, so when it specifies `risk`, `category`, or `owner` those values win
/// over whatever the rules produced. Each field is applied independently — a
/// sidecar that sets only `owner` leaves the rule's risk and category intact.
///
/// A `risk:` value that is not one of `low|medium|high|critical` is **not**
/// silently ignored: we leave the rule's risk in place and push a [`ScanWarning`]
/// so the user learns their annotation was malformed instead of believing it
/// took effect. `category` and `owner` are free-form strings, so any non-empty
/// value is accepted as-is.
fn apply_sidecar_override(
    entry: &ScriptEntry,
    classification: &mut Classification,
    warnings: &mut Vec<ScanWarning>,
) {
    let Some(sidecar) = &entry.sidecar else {
        return;
    };

    if let Some(risk_token) = &sidecar.risk {
        match crate::core::rules::RiskLevel::from_token(risk_token) {
            Some(level) => classification.risk = level,
            None => warnings.push(ScanWarning {
                path: Some(entry.discovered.path.clone()),
                message: format!(
                    "sidecar risk {risk_token:?} is not one of low|medium|high|critical; \
                     keeping the rule-derived risk"
                ),
            }),
        }
    }

    if let Some(category) = sidecar.category.as_deref().map(str::trim)
        && !category.is_empty()
    {
        classification.category = category.to_string();
    }

    if let Some(owner) = sidecar.owner.as_deref().map(str::trim)
        && !owner.is_empty()
    {
        classification.owner = owner.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::entry::{Language, SidecarMetadata};
    use crate::core::rules::RiskLevel;
    use crate::core::scanner::DiscoveredFile;
    use std::path::PathBuf;

    fn entry_with_sidecar(sidecar: Option<SidecarMetadata>) -> ScriptEntry {
        ScriptEntry {
            discovered: DiscoveredFile {
                path: PathBuf::from("/home/u/bin/tool.sh"),
                size: 10,
                is_executable: true,
            },
            language: Language::Bash,
            description: None,
            sidecar,
            sidecar_warning: None,
        }
    }

    fn rule_classification() -> Classification {
        Classification {
            risk: RiskLevel::Low,
            category: "script".to_string(),
            owner: "user".to_string(),
        }
    }

    #[test]
    fn sidecar_overrides_each_field_independently() {
        let entry = entry_with_sidecar(Some(SidecarMetadata {
            description: None,
            tags: vec![],
            risk: Some("high".to_string()),
            category: Some("destructive".to_string()),
            owner: None, // left unset → keep the rule's owner
        }));
        let mut class = rule_classification();
        let mut warnings = Vec::new();

        apply_sidecar_override(&entry, &mut class, &mut warnings);

        assert_eq!(class.risk, RiskLevel::High);
        assert_eq!(class.category, "destructive");
        assert_eq!(class.owner, "user", "unset sidecar field must not override");
        assert!(warnings.is_empty());
    }

    #[test]
    fn sidecar_risk_is_case_insensitive() {
        let entry = entry_with_sidecar(Some(SidecarMetadata {
            risk: Some("CRITICAL".to_string()),
            ..Default::default()
        }));
        let mut class = rule_classification();
        let mut warnings = Vec::new();

        apply_sidecar_override(&entry, &mut class, &mut warnings);

        assert_eq!(class.risk, RiskLevel::Critical);
        assert!(warnings.is_empty());
    }

    #[test]
    fn malformed_sidecar_risk_warns_and_keeps_rule_risk() {
        let entry = entry_with_sidecar(Some(SidecarMetadata {
            risk: Some("spicy".to_string()),
            ..Default::default()
        }));
        let mut class = rule_classification();
        let mut warnings = Vec::new();

        apply_sidecar_override(&entry, &mut class, &mut warnings);

        assert_eq!(
            class.risk,
            RiskLevel::Low,
            "bad risk must not change verdict"
        );
        assert_eq!(warnings.len(), 1, "a malformed risk must be surfaced");
        assert!(warnings[0].message.contains("spicy"));
        assert_eq!(
            warnings[0].path.as_deref(),
            Some(entry.discovered.path.as_path())
        );
    }

    #[test]
    fn empty_or_absent_sidecar_changes_nothing() {
        // No sidecar at all.
        let mut class = rule_classification();
        let mut warnings = Vec::new();
        apply_sidecar_override(&entry_with_sidecar(None), &mut class, &mut warnings);
        assert_eq!(class, rule_classification());
        assert!(warnings.is_empty());

        // Sidecar present but blank fields → no-op (blank strings ignored).
        let entry = entry_with_sidecar(Some(SidecarMetadata {
            category: Some("   ".to_string()),
            owner: Some(String::new()),
            ..Default::default()
        }));
        apply_sidecar_override(&entry, &mut class, &mut warnings);
        assert_eq!(class, rule_classification());
        assert!(warnings.is_empty());
    }
}
