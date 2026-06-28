use crate::model::normalized::Severity;

/// Bucket a free-form severity string into the canonical `Severity` enum.
///
/// `pub(crate)` so the adapter (its sibling `super`) can call it. It is an internal
/// seam, not part of Workstate's public API.
///
/// Do not deserialize directly: the wire type (`BulwarkRawItem.severity`) is an
/// `Option<String>` for good reason: we must NOT deserialize straight into the
/// `Severity` enum. If we did, serde's `#[serde(rename_all = "snake_case")]`
/// would map lowercase `"critical"` correctly, BUT it would route `"Critical"`,
/// `"HIGH"`, or `"  high  "` directly into `#[serde(other)] Unknown` — the wrong
/// bucket for a recognized severity that merely has unexpected casing or whitespace.
/// Bucketing by hand (trim + to_ascii_lowercase + match) is the safe, exhaustive
/// approach that matches what RexOps's feed consumer already does.
///
/// TWO NON-SEVERITY OUTCOMES (these are DISTINCT — see `Severity`'s `Unrated` vs
/// `Unknown`):
///
/// * `None` (severity field absent entirely) → `Severity::Unrated` ("no risk
///   signal" — the feed said nothing about risk for this item).
/// * `Some(value)` we don't recognize (e.g. "spicy") → `Severity::Unknown` (a
///   value WAS present, just not a bucket we know).
///
/// This preserves the unrated/unknown distinction RexOps tracks in its `RiskTally`,
/// which an earlier version of this adapter collapsed into a single `Unknown`.
pub(crate) fn bucket_severity(raw: Option<&str>) -> Severity {
    // Absent field → Unrated up front: there is genuinely no severity information,
    // which is different from an unrecognized value (handled by the wildcard below).
    let Some(value) = raw else {
        return Severity::Unrated;
    };
    // Present: trim whitespace and normalize casing before matching, so a recognized
    // severity in any casing/padding ("  Critical ") buckets correctly.
    match value.trim().to_ascii_lowercase().as_str() {
        "info" => Severity::Info,
        "low" => Severity::Low,
        "medium" => Severity::Medium,
        "high" => Severity::High,
        "critical" => Severity::Critical,
        // Present but unrecognized → Unknown (NOT Unrated: a value was supplied).
        _ => Severity::Unknown,
    }
}

// =============================================================================
// Tests — unit tests for the pure bucketing rule, co-located with the helper.
// =============================================================================
// These stay IN-CRATE (not in the external `tests/` dir) because `bucket_severity`
// is `pub(crate)`: an external integration test could not see it. The behavioral
// end-to-end tests (which exercise bucketing THROUGH the public `normalize`) live
// in `tests/bulwark.rs`; these guard the helper's own edge cases directly.
#[cfg(test)]
mod tests {
    use super::*;

    /// The critical test for `bucket_severity`. Two things it proves:
    ///   1. HAND-BUCKETING (not enum deserialization): a recognized value in
    ///      non-canonical form (`"  Critical "`) buckets correctly. Direct enum
    ///      deserialization (or a missing trim/lower) would route it to `Unknown`.
    ///   2. The `Unrated` vs `Unknown` DISTINCTION: an absent severity (`None`) →
    ///      `Unrated` ("no risk signal"), while a present-but-unrecognized value
    ///      ("spicy") → `Unknown`. These must NOT collapse to the same variant.
    #[test]
    fn severity_bucketing_trims_and_lowercases() {
        // This is THE discriminating case: caps + whitespace → Critical.
        // If trim/lowercase are missing, this becomes Unknown (wrong).
        assert_eq!(bucket_severity(Some("  Critical ")), Severity::Critical);

        // Standard lowercase matches:
        assert_eq!(bucket_severity(Some("critical")), Severity::Critical);
        assert_eq!(bucket_severity(Some("high")), Severity::High);
        assert_eq!(bucket_severity(Some("medium")), Severity::Medium);
        assert_eq!(bucket_severity(Some("low")), Severity::Low);
        assert_eq!(bucket_severity(Some("info")), Severity::Info);

        // PRESENT but unrecognized value → Unknown (a value WAS supplied).
        assert_eq!(bucket_severity(Some("spicy")), Severity::Unknown);

        // ABSENT severity → Unrated (no risk signal at all) — DISTINCT from Unknown.
        assert_eq!(bucket_severity(None), Severity::Unrated);
        // Guard the distinction explicitly so a future regression that collapses
        // them back into one variant fails loudly here.
        assert_ne!(bucket_severity(None), bucket_severity(Some("spicy")));
    }
}
