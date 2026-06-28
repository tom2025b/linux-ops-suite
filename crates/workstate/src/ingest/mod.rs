use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use thiserror::Error;

// Provenance id used to attribute every fact back to the feed that produced it.
use crate::model::provenance::FeedId;

// Per-tool adapter submodules. Each is a separate file implementing FeedSource.
pub mod bulwark; // security/posture feed
pub mod proto; // protocol-run (jobs) feed
pub mod scriptvault; // managed scripts feed
pub mod toolfoundry; // tools/binaries feed

// Where an adapter's raw bytes come from — a file OR a spawned producer command.
// The seam that lets Workstate ingest LIVE tool output instead of fixtures.
pub mod transport;
pub use transport::FeedTransport;

/// Errors an ingestion adapter can hit. A typed enum (via `thiserror`) so the
/// compiler/caller can react differently per case — e.g. `NotFound` becomes a
/// `Missing` section while `Parse` becomes a `Failed` section. Crucially, NONE
/// of these abort the snapshot build; the compiler maps them to section status.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FeedError {
    /// The feed's backing file/source was not present at all.
    #[error("feed not found: {0}")]
    NotFound(String),

    /// The feed was present but could not be read or parsed.
    #[error("failed to read or parse feed: {0}")]
    Parse(String),

    /// Catch-all for an unexpected I/O problem while reading the feed.
    /// `#[from]` lets `?` auto-convert a `std::io::Error` into this variant.
    #[error("io error reading feed: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse a source tool timestamp into UTC.
///
/// Source feeds use a few loose shapes; we accept all of them and normalize to UTC:
///   1. RFC3339 with an offset (`2026-06-04T12:00:00Z`, `...+02:00`) — the offset
///      is honored and converted to UTC.
///   2. A NAIVE datetime with NO timezone (`2026-06-04T12:00:00`, or space-
///      separated `2026-06-04 12:00:00`, with optional fractional seconds). This is
///      a very common serialization, so we MUST NOT drop it: a missing offset is
///      ASSUMED to be UTC. Failing to handle this was a real bug — such a value fell
///      through to `None`, which made a genuinely stale feed report as `Fresh`
///      (freshness needs a source timestamp; no timestamp ⇒ defaulted Fresh).
///   3. A date-only string (`2026-06-04`) — treated as midnight UTC.
///
/// Empty or unparseable values return `None` (source generation time unknown),
/// which is itself useful provenance — but we exhaust the common formats first so
/// `None` means "genuinely unrecognized", not "we forgot a format".
pub(crate) fn parse_source_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    // 1. Offset-aware RFC3339 first: if an explicit offset is present, honor it.
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&Utc));
    }

    // 2. Naive datetime (no offset) → assume UTC. Try the RFC3339-style `T`
    //    separator and the common space separator; `%.f` makes fractional seconds
    //    optional so both `...:00` and `...:00.123` parse.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, fmt) {
            return Some(naive.and_utc());
        }
    }

    // 3. Date-only → midnight UTC.
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

/// The single trait every tool adapter implements. ONE trait, MANY implementors.
///
/// Trait contract: we want one shared shape — "read a feed,
/// then normalize it" — that each tool fills in with its own logic. That's
/// exactly what traits are for.
///
/// NOTE ON DISPATCH: because of the associated types below, this trait is used by
/// STATIC dispatch (one concrete adapter per call site in the compiler), not via
/// `Box<dyn FeedSource>`.
pub trait FeedSource {
    /// The raw wire shape this adapter reads (e.g. `BulwarkRaw`). Associated type
    /// because each adapter has its own; the compiler resolves it per implementor.
    type Raw;

    /// The canonical model this adapter produces. This is what ends up inside the
    /// matching `Section<...>` of the Snapshot.
    type Normalized;

    /// A stable identifier for this feed, used in provenance so every fact in
    /// the snapshot is attributable back to its source.
    fn feed_id(&self) -> FeedId;

    /// The feed schema version Workstate currently understands.
    fn supported_schema_version(&self) -> Option<i64> {
        None
    }

    /// Extract the raw feed's declared schema version, if any.
    fn schema_version(&self, _raw: &Self::Raw) -> Option<i64> {
        None
    }

    /// The `source_tool` label this adapter expects its feed to carry (e.g.
    /// "bulwark"). `None` (the default) means "do not cross-check the source tool".
    fn expected_source_tool(&self) -> Option<&str> {
        None
    }

    /// Extract the feed's self-reported `source_tool` label. `None` (the default)
    /// means this adapter does not expose one, so no cross-check happens.
    fn source_tool<'a>(&self, _raw: &'a Self::Raw) -> Option<&'a str> {
        None
    }

    /// Extract the source tool's generation/observation timestamp, if any.
    fn source_observed_at(&self, _raw: &Self::Raw) -> Option<DateTime<Utc>> {
        None
    }

    /// Read the raw feed. READ-ONLY: returns data or a typed error, and takes
    /// `&self` so it cannot be coerced into mutating anything external.
    fn fetch(&self) -> Result<Self::Raw, FeedError>;

    /// Map the raw feed into Workstate's canonical model. This is the explicit
    /// anti-corruption seam: messy upstream shape in, clean internal truth out.
    ///
    /// Adapter-owned normalization: each tool's quirks are the
    /// adapter's concern. The compiler stays generic — it just calls this and
    /// wraps the result. Takes `&self` (read-only) and consumes `raw` by value
    /// since the raw envelope is spent once normalized.
    ///
    /// DROP ACCOUNTING lives on the returned inventory, not the signature: each
    /// canonical inventory carries a `dropped_records` count that normalize sets
    /// when it skips an id-less record (via `filter_map`). The compiler copies that
    /// count into `Provenance.dropped_records`, so a drop is accounted, never
    /// silent — and this trait's shape stays unchanged.
    fn normalize(&self, raw: Self::Raw) -> Self::Normalized;
}

#[cfg(test)]
mod timestamp_tests {
    use super::parse_source_timestamp;

    /// Helper: parse and render back to RFC3339, or "None" when unparseable.
    fn parsed(value: &str) -> String {
        parse_source_timestamp(value).map_or_else(|| "None".to_string(), |dt| dt.to_rfc3339())
    }

    #[test]
    fn parses_offset_aware_rfc3339() {
        assert_eq!(parsed("2026-06-04T12:00:00Z"), "2026-06-04T12:00:00+00:00");
        // A non-UTC offset is converted to UTC.
        assert_eq!(
            parsed("2026-06-04T12:00:00+02:00"),
            "2026-06-04T10:00:00+00:00"
        );
    }

    #[test]
    fn parses_naive_datetime_as_utc() {
        // THE regression: a naive datetime (no timezone) must NOT become None.
        // Before the fix this fell through and the feed was treated as Fresh.
        assert_eq!(parsed("2026-06-04T12:00:00"), "2026-06-04T12:00:00+00:00");
        // Space-separated variant.
        assert_eq!(parsed("2026-06-04 12:00:00"), "2026-06-04T12:00:00+00:00");
        // Fractional seconds are tolerated.
        assert_eq!(
            parsed("2026-06-04T12:00:00.500"),
            "2026-06-04T12:00:00.500+00:00"
        );
    }

    #[test]
    fn parses_date_only_as_midnight_utc() {
        assert_eq!(parsed("2026-06-04"), "2026-06-04T00:00:00+00:00");
        assert_eq!(parsed("  2026-06-04  "), "2026-06-04T00:00:00+00:00");
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(parsed(""), "None");
        assert_eq!(parsed("   "), "None");
        assert_eq!(parsed("not-a-date"), "None");
        assert_eq!(parsed("2026-13-99"), "None"); // invalid month/day
        assert_eq!(parsed("2026/06/04"), "None"); // wrong separator
    }
}
