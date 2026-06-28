// Bring serde's derive macros into scope so we can annotate our types.
use serde::{Deserialize, Serialize};
// chrono's UTC timestamp type. We always store time in UTC to avoid timezone
// ambiguity; display/localization is the consumer's concern, not the truth's.
use chrono::{DateTime, Utc};

/// A newtype wrapper identifying which feed a fact came from (e.g. "bulwark").
///
/// Newtype wrapper: it makes function signatures
/// self-documenting and prevents accidentally passing, say, a host name where a
/// feed id is expected. It costs nothing at runtime — it's just a String with a
/// name the compiler enforces.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeedId(pub String);

/// Records WHERE a piece of state came from and WHEN it was fetched.
///
/// Every section of the snapshot carries one of these so RexOps (and humans)
/// can always answer "who told us this, and how old is it?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Which feed produced this data.
    pub feed_id: FeedId,
    /// The instant Workstate compiled this section (UTC). `Option` because a
    /// Missing feed was never read, so there is genuinely no timestamp.
    ///
    /// NOT a freshness input. It is captured with `Utc::now()` during the build,
    /// so it is essentially the snapshot's own `built_at` and is the SAME for every
    /// section of a run. It records "when Workstate last produced this", which is
    /// useful provenance, but it says nothing about how old the underlying data is
    /// — a feed file written 8 days ago still gets a just-now `fetched_at`. The
    /// only real staleness signal is `source_observed_at` below; freshness
    /// (`Fresh`/`Stale`/`FreshnessUnknown`) is decided from that, never from this.
    pub fetched_at: Option<DateTime<Utc>>,
    /// When the SOURCE TOOL itself generated the data (e.g. Bulwark's
    /// `generated_at`), as an absolute UTC instant.
    ///
    /// Distinct from `fetched_at`:
    ///   `fetched_at` = when *Workstate* compiled the section (≈ build time, same
    ///   for every section). `source_observed_at` is when the upstream tool
    ///   *produced* the truth. The age of the data is `now - source_observed_at`;
    ///   `fetched_at` does not enter that calculation. A feed whose source wrote it
    ///   8 days ago is STALE, and only this field can reveal that.
    ///
    /// Stored as an absolute timestamp rather than an age value:
    ///   An absolute instant is self-contained truth — it does not silently rot
    ///   the moment it is serialized. An "age in seconds" is only correct at the
    ///   instant it is computed; the second it lands in `snapshot.json` it is
    ///   already wrong, and RexOps would have to know exactly when it was measured
    ///   to fix it. Store the fact (when it happened); let consumers compute age
    ///   against their own `now`.
    ///
    /// Stored on `Provenance` so all statuses expose the same freshness metadata:
    ///   Provenance is recorded for EVERY section regardless of health — a Fresh
    ///   feed has a source timestamp too, and we want it available so RexOps can
    ///   judge freshness itself rather than trusting our `Fresh`/`Stale` verdict.
    ///   Hanging it off the Stale variant would only expose it for already-stale
    ///   feeds. `Option` because not every tool reports
    ///   when it generated its data; `None` means "source generation time
    ///   unknown", which is itself useful provenance.
    pub source_observed_at: Option<DateTime<Utc>>,
    /// How many raw records this section's adapter DROPPED during normalization
    /// (e.g. a record with no usable id). Recorded here, on provenance, for the
    /// same reason as `source_observed_at`: provenance is present on EVERY section
    /// regardless of health, so the loss count survives even when `data` is `None`
    /// and is always visible to RexOps. `0` for a clean compile (and for Missing /
    /// Failed / UnsupportedVersion sections, which never normalized any records).
    ///
    /// This closes a silent-data-loss gap: normalization drops id-less records via
    /// `filter_map`, and without this count a lossy section was indistinguishable
    /// from a complete one. `#[serde(default)]` so older snapshots (no field) still
    /// deserialize as `0`.
    #[serde(default)]
    pub dropped_records: usize,
}

/// The health of a feed at compile time. This is the heart of graceful
/// degradation: the snapshot build inspects this, never panics on a bad feed.
///
/// Enum form: these states are mutually exclusive and we want the compiler to
/// force every consumer to handle each case (exhaustive `match`). That is far
/// safer than a pair of booleans like `is_stale`/`is_missing` that can express
/// nonsense combinations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum FeedStatus {
    /// Feed read successfully and is recent enough to trust.
    Fresh,
    /// Feed read successfully but is older than its freshness threshold. The
    /// data is still attached but RexOps should treat it with caution. This means
    /// "we KNOW it is old" — distinct from `FreshnessUnknown` below.
    Stale,
    /// Feed read successfully, but its source generation time could not be
    /// determined (`source_observed_at` is `None`: the upstream `generated_at` was
    /// absent, empty, or in a format we don't parse), so Workstate CANNOT vouch
    /// that the data is recent. The data is still attached, but the section is
    /// deliberately NOT labeled `Fresh`.
    ///
    /// This exists because freshness must fail CLOSED, consistent with the version
    /// gate: an unknown-age feed previously defaulted to `Fresh`, which let a
    /// genuinely stale feed look current. `FreshnessUnknown` is kept distinct from
    /// `Stale` because the two carry different meaning for RexOps — "known old"
    /// versus "age unknown" — and collapsing them would lose that signal.
    FreshnessUnknown,
    /// Feed was present and parseable and DECLARED a schema version, but it is not
    /// the one Workstate understands. The data is intentionally rejected rather
    /// than normalized as if it were fresh. Distinct from `MissingVersion`, which
    /// is the "no version declared at all" case.
    UnsupportedVersion {
        /// The version the feed declared. Always `Some` here — a feed that declared
        /// NO version is `MissingVersion`, not this. (`Option` is retained for
        /// backward-compatible deserialization of older snapshots.)
        found: Option<i64>,
        /// Version Workstate currently supports for this feed.
        supported: i64,
    },
    /// Feed was present and parseable but declared NO schema version at all (the
    /// field was absent). Treated as strictly as a wrong version — data is rejected
    /// — but kept STRUCTURALLY distinct from `UnsupportedVersion` so a consumer can
    /// tell "producer forgot to stamp a version" apart from "producer stamped a
    /// version we don't support". Both fail closed; only the diagnosis differs.
    MissingVersion {
        /// Version Workstate currently supports for this feed.
        supported: i64,
    },
    /// Feed read and parsed, but its self-reported `source_tool` does not match the
    /// tool this adapter expects (e.g. the Bulwark path was pointed at a ScriptVault
    /// export). The data is rejected rather than normalized under the wrong feed_id,
    /// which would otherwise bake mislabeled data into the source-of-truth snapshot.
    /// An EMPTY `source_tool` is tolerated (not a mismatch), matching prior leniency.
    SourceMismatch {
        /// The `source_tool` this adapter expected (e.g. "bulwark").
        expected: String,
        /// The `source_tool` the feed actually declared.
        found: String,
    },
    /// Feed was not present at all (tool hasn't run, file absent). No data.
    Missing,
    /// Feed was present but could not be read or parsed. The payload carries the
    /// reason so it can be surfaced without crashing the build.
    Failed { reason: String },
    /// Forward-compat catch-all: a status produced by a NEWER Workstate whose
    /// representation this version does not recognize. The raw JSON is preserved
    /// so nothing is lost, and the snapshot still deserializes (the schema marks
    /// `status` as intentionally open — "an unrecognized value degrades on the
    /// consumer rather than failing validation"). Consumers should treat this as
    /// "unknown health" rather than trusting the data. Never produced by this
    /// version directly; it only appears when reading a future snapshot.
    Unknown(serde_json::Value),
}

/// The externally-tagged variant names this version of Workstate understands.
/// A status whose tag is in this set MUST deserialize successfully or it is a
/// hard error; only a tag OUTSIDE this set degrades to [`FeedStatus::Unknown`].
const KNOWN_FEED_STATUS_TAGS: &[&str] = &[
    "Fresh",
    "Stale",
    "FreshnessUnknown",
    "UnsupportedVersion",
    "MissingVersion",
    "SourceMismatch",
    "Missing",
    "Failed",
];

// `FeedStatus` is deserialized by consumers (e.g. RexOps) reading a snapshot
// that may have been written by a NEWER Workstate. A plain derive would hard-
// FAIL the whole snapshot on an unrecognized variant — but the schema declares
// `status` open by design, so an UNKNOWN tag must degrade to `Unknown(raw)`.
//
// Critically, this only catches an *unrecognized tag*. A KNOWN tag whose payload
// is malformed (e.g. `UnsupportedVersion` with a non-integer `supported`) is a
// real corruption and MUST surface as an error, not be silently swallowed into
// `Unknown`. So we identify the tag first, and only fall back to `Unknown` when
// the tag itself is one this version does not know.
impl<'de> Deserialize<'de> for FeedStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        // Mirror of the KNOWN variants only. Externally tagged, identical wire
        // shape to the public enum's known variants, so a value this version
        // understands round-trips through it unchanged.
        #[derive(Deserialize)]
        enum Known {
            Fresh,
            Stale,
            FreshnessUnknown,
            UnsupportedVersion { found: Option<i64>, supported: i64 },
            MissingVersion { supported: i64 },
            SourceMismatch { expected: String, found: String },
            Missing,
            Failed { reason: String },
        }

        // Buffer once so we can inspect the tag before committing to a shape.
        let raw = serde_json::Value::deserialize(deserializer)?;

        // The externally-tagged discriminant: a bare string IS the tag (unit
        // variants like "Fresh"); an object's single key is the tag (data
        // variants like {"Failed": {...}}). Anything else is malformed.
        let tag: Option<&str> = match &raw {
            serde_json::Value::String(s) => Some(s.as_str()),
            serde_json::Value::Object(map) if map.len() == 1 => {
                map.keys().next().map(String::as_str)
            }
            _ => None,
        };

        match tag {
            // Known tag: deserialize strictly. A malformed payload is a real
            // error and propagates — it is NOT swallowed into Unknown.
            Some(t) if KNOWN_FEED_STATUS_TAGS.contains(&t) => {
                let known = Known::deserialize(&raw).map_err(D::Error::custom)?;
                Ok(match known {
                    Known::Fresh => FeedStatus::Fresh,
                    Known::Stale => FeedStatus::Stale,
                    Known::FreshnessUnknown => FeedStatus::FreshnessUnknown,
                    Known::UnsupportedVersion { found, supported } => {
                        FeedStatus::UnsupportedVersion { found, supported }
                    }
                    Known::MissingVersion { supported } => FeedStatus::MissingVersion { supported },
                    Known::SourceMismatch { expected, found } => {
                        FeedStatus::SourceMismatch { expected, found }
                    }
                    Known::Missing => FeedStatus::Missing,
                    Known::Failed { reason } => FeedStatus::Failed { reason },
                })
            }
            // Unrecognized tag (a status from a newer Workstate): degrade.
            Some(_) => Ok(FeedStatus::Unknown(raw)),
            // No identifiable tag at all (not a string, not a single-key object):
            // this is not forward-compat, it is malformed input — error.
            None => Err(D::Error::custom(
                "FeedStatus must be a string or a single-key tagged object",
            )),
        }
    }
}

/// Wraps any domain payload `T` together with its status and provenance.
///
/// This is the uniform envelope used by every section of the Snapshot. The
/// payload is `Option<T>` because Missing/Failed sections legitimately have no
/// data, yet must still appear in the snapshot (so RexOps sees the gap
/// explicitly rather than silently missing a whole domain).
///
/// `T: Serialize + Deserialize` is NOT required here — serde derives the bounds
/// automatically per concrete `T` when the section is (de)serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section<T> {
    /// Health of the feed backing this section.
    pub status: FeedStatus,
    /// Where this section's data came from and when.
    pub provenance: Provenance,
    /// The normalized data itself; `None` when status is Missing/Failed.
    pub data: Option<T>,
}

impl<T> Section<T> {
    /// Convenience constructor for a section whose feed never showed up.
    ///
    /// Note: building "absence" is common during a degraded compile, so giving it
    /// a name keeps the compiler code readable.
    pub fn missing(feed_id: FeedId) -> Self {
        Section {
            status: FeedStatus::Missing,
            provenance: Provenance {
                feed_id,
                fetched_at: None,         // never fetched -> no read timestamp
                source_observed_at: None, // never read it -> source time unknown too
                dropped_records: 0,       // never normalized anything -> nothing dropped
            },
            data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FeedStatus;

    #[test]
    fn known_feed_statuses_round_trip() {
        // Each known variant serializes and deserializes back to itself, so the
        // custom Deserialize did not regress the normal path.
        for status in [
            FeedStatus::Fresh,
            FeedStatus::Stale,
            FeedStatus::FreshnessUnknown,
            FeedStatus::Missing,
            FeedStatus::UnsupportedVersion {
                found: Some(2),
                supported: 4,
            },
            FeedStatus::MissingVersion { supported: 4 },
            FeedStatus::SourceMismatch {
                expected: "bulwark".into(),
                found: "scriptvault".into(),
            },
            FeedStatus::Failed {
                reason: "boom".into(),
            },
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: FeedStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back, "round-trip failed for {json}");
        }
    }

    #[test]
    fn unknown_bare_string_status_degrades_instead_of_failing() {
        // A future bare-string status (the schema's open-string clause) must not
        // fail deserialization — it lands in Unknown, preserving the raw value.
        let back: FeedStatus = serde_json::from_str("\"Quarantined\"").unwrap();
        match back {
            FeedStatus::Unknown(v) => assert_eq!(v, serde_json::json!("Quarantined")),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn unknown_tagged_object_status_degrades_instead_of_failing() {
        // A future tagged-object variant must also degrade rather than hard-fail
        // the whole snapshot for a consumer pinned to this version.
        let raw = r#"{"Quarantined":{"reason":"policy"}}"#;
        let back: FeedStatus = serde_json::from_str(raw).unwrap();
        assert!(
            matches!(back, FeedStatus::Unknown(_)),
            "an unknown tagged variant must degrade to Unknown, got {back:?}"
        );
    }

    #[test]
    fn malformed_known_variant_errors_and_is_not_swallowed_into_unknown() {
        // The whole point of the fix: a KNOWN tag with a bad payload is real
        // corruption and MUST error, not silently become Unknown.
        // `supported` is an i64; a string is wrong.
        let bad = r#"{"UnsupportedVersion":{"found":2,"supported":"four"}}"#;
        let err = serde_json::from_str::<FeedStatus>(bad);
        assert!(
            err.is_err(),
            "a malformed known variant must error, got {err:?}"
        );

        // Missing a required field on a known variant is likewise an error.
        let missing = r#"{"MissingVersion":{}}"#;
        assert!(
            serde_json::from_str::<FeedStatus>(missing).is_err(),
            "a known variant missing a required field must error"
        );

        // A known unit variant carrying object data is malformed for that tag.
        let bad_unit = r#"{"Fresh":{"oops":1}}"#;
        assert!(
            serde_json::from_str::<FeedStatus>(bad_unit).is_err(),
            "a known unit tag with a payload must error"
        );
    }

    #[test]
    fn structurally_untagged_input_errors() {
        // Not a string and not a single-key object: malformed, not forward-compat.
        for bad in ["123", "true", "null", "[]", r#"{"a":1,"b":2}"#] {
            assert!(
                serde_json::from_str::<FeedStatus>(bad).is_err(),
                "structurally-invalid status {bad} must error"
            );
        }
    }
}
