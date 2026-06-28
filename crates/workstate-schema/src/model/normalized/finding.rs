use serde::{Deserialize, Serialize};

/// Severity of a security finding, normalized to a fixed bucket set.
///
/// Two non-severity sentinel values:
///   RexOps's Bulwark feed consumer reads severity as a free *string* and buckets
///   it, mapping anything it doesn't recognize to "unknown" rather than rejecting
///   the feed — AND it separately tracks items that carry no severity field at all
///   (its `RiskTally::unrated`). We mirror BOTH of those distinctions with a closed
///   enum that gives consumers a clean type to `match` on:
///     * `Unrated` — the finding carried NO severity information at all (the field
///       was absent). This is "we have no risk signal", NOT "zero risk".
///     * `Unknown` — a severity value WAS present but is not one we recognize
///       (Bulwark's contract is explicitly PROVISIONAL, so an unexpected string
///       must degrade, not fail). `#[serde(other)]` routes unrecognized values here
///       on deserialize instead of erroring.
///   Keeping these apart lets a consumer say "risk breakdown unavailable" (all
///   `Unrated`) versus "present but unclassifiable" (`Unknown`) — a distinction the
///   adapter previously had to collapse, now preserved end-to-end.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
    /// The finding carried NO severity field at all → "no risk signal" (not zero
    /// risk). Distinct from `Unknown`. Serializes as "unrated".
    Unrated,
    /// A severity value was present but is not one we recognize. `serde(other)`
    /// routes unrecognized strings here on deserialize instead of erroring.
    #[serde(other)]
    Unknown,
}

impl Severity {
    const fn rank(self) -> u8 {
        match self {
            Severity::Unrated | Severity::Unknown => 0,
            Severity::Info => 1,
            Severity::Low => 2,
            Severity::Medium => 3,
            Severity::High => 4,
            Severity::Critical => 5,
        }
    }
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Identifies the SUBJECT a finding was raised against (in Bulwark's exported
/// feed this is the item `id`, e.g. the script filename the scan flagged).
///
/// HONESTY NOTE: this is NOT a per-finding uniqueness guarantee. One subject can
/// have several findings (several rules can fire on the same script), and the
/// same rule can fire on many subjects. It is a stable label for "what was
/// scanned", which is what RexOps groups and displays by — not a primary key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FindingId(pub String);

/// A canonical security finding as reported by Bulwark.
///
/// Findings are GLOBAL (no HostId): Bulwark scans content/scripts, not a host
/// inventory, so a finding belongs to a subject, not a machine.
///
/// FIELD CHOICES (matches RexOps's `BulwarkFinding` + feed consumer):
/// * `id` — the scanned subject (see `FindingId`). MANDATORY: a finding with no
///   subject cannot be a stable thing, so the adapter drops such records rather
///   than inventing one.
/// * `rule_id` — which Bulwark rule fired (e.g. "aws-key"). `Option<String>`,
///   kept separate from `id`: the finding rule is the *reason*, the id is the *subject*.
///   `Option` because the provisional Bulwark export does not guarantee it.
/// * `description` — human-readable explanation, if the feed provided one
///   (`Option<String>`; `None` = absent, not blank).
/// * `severity` — normalized bucket (see `Severity`); drives RexOps's risk tally
///   and should-block decision. Absent severity → `Unrated` (no risk signal);
///   present-but-unrecognized → `Unknown`.
/// * `category` — kind of issue (e.g. "secret_leakage"). `Option<String>`, NOT an
///   enum: Bulwark's own category type has a `Custom(String)` escape hatch, so
///   the set is genuinely open.
/// * `location` — where in the subject it was found, flattened to a display
///   `String` for v1. Stays a plain `String` (NOT `Option`): the adapter always
///   produces a value ("unknown" when absent), so there is no "missing" case.
///
/// IntentionalLY OMITTED for v1:
///   * `snippet` — the matched text, i.e. the ACTUAL leaked secret/PII. The
///     snapshot is persisted to `snapshot.json` on disk; copying the secret in
///     would widen its exposure. Workstate observes risk, it must not become a
///     new place secrets leak to. Omitted from the snapshot.
///   * `action` (log/redact/block) — observational disposition; lean-cut for v1,
///     re-addable behind a schema bump if RexOps needs it.
///
/// `deny_unknown_fields` enforces the v4 allowlist at the Rust level. The
/// snapshot schema sets `additionalProperties` to false for findings precisely
/// so an unmodeled Bulwark wire field (which may hold matched secrets or PII)
/// can never reach the snapshot. Without this attribute, deserialization would
/// silently drop an unexpected field instead of rejecting it; with it, the
/// closed-allowlist contract holds symmetrically on both serialize (no
/// passthrough bag exists) and deserialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// The scanned subject this finding was raised against.
    pub id: FindingId,
    /// Human subject name, if the feed supplied one separately from `id`.
    pub name: Option<String>,
    /// Which Bulwark rule produced the finding, if the feed reported it.
    /// `None` means absent in the feed (not the same as a present empty value).
    pub rule_id: Option<String>,
    /// Human-readable description of what was found, if present (`None` = absent).
    pub description: Option<String>,
    /// Normalized severity bucket (`Unrated` if the feed carried no severity).
    pub severity: Severity,
    /// Original free-form severity string, if present.
    pub raw_severity: Option<String>,
    /// Open-set category label (e.g. "secret_leakage", "pii", or a custom value),
    /// if the feed provided one. `None` = absent.
    pub category: Option<String>,
    /// Where in the subject the issue was located, as a display string. Always
    /// present ("unknown" when the feed gave no location), so not an `Option`.
    pub location: String,
    /// Filesystem path of the scanned script, from Bulwark's `path` field, if
    /// present. Part of the explicit allowlist (NOT a passthrough bag): downstream
    /// consumers (e.g. toolbox-bridge) key sidecar records by this path, so it is
    /// modeled as a first-class field rather than left in an open `rest` map.
    /// `None` = the feed did not report a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Bulwark's free-form `risk` label, if present (e.g. "critical"). Distinct
    /// from `severity`/`raw_severity`: it is Bulwark's own display risk string that
    /// consumers surface as a tag. `None` = absent. Allowlisted, not passthrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// Owning user/team for the scanned subject, from Bulwark's `owner` field, if
    /// present. `None` = absent. Allowlisted, not passthrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// Canonical Bulwark finding inventory plus feed-level fields RexOps reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingInventory {
    /// Bulwark's source generation string.
    pub generated_at: String,
    /// Normalized finding records.
    pub findings: Vec<Finding>,
    /// How many raw items normalization DROPPED (no usable subject). The compiler
    /// copies this onto `Provenance.dropped_records` so the loss is never silent.
    /// `#[serde(default)]` so older snapshots without the field deserialize as `0`.
    #[serde(default)]
    pub dropped_records: usize,
}

#[cfg(test)]
mod tests {
    use super::{Finding, Severity};

    #[test]
    fn critical_sorts_above_sentinel_values() {
        assert!(Severity::Critical > Severity::Unrated);
        assert!(Severity::Critical > Severity::Unknown);
    }

    #[test]
    fn finding_rejects_unknown_fields_so_pii_cannot_sneak_in() {
        // A minimal valid finding deserializes fine.
        let ok = r#"{"id":"x","name":null,"rule_id":null,"description":null,
            "severity":"Unrated","raw_severity":null,"category":null,"location":"unknown"}"#;
        assert!(serde_json::from_str::<Finding>(ok).is_ok());

        // The same payload plus an unmodeled field (e.g. Bulwark's `snippet`,
        // which would carry the matched secret) MUST be rejected, not silently
        // dropped — that is the v4 allowlist guarantee.
        let with_secret = r#"{"id":"x","name":null,"rule_id":null,"description":null,
            "severity":"Unrated","raw_severity":null,"category":null,"location":"unknown",
            "snippet":"AKIA-leaked-secret"}"#;
        let err = serde_json::from_str::<Finding>(with_secret);
        assert!(
            err.is_err(),
            "an unknown field (a potential PII/secret leak) must be rejected"
        );
    }
}
