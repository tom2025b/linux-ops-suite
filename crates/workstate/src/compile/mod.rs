use chrono::{DateTime, Duration, Utc};

// Snapshot WRITING (atomic publish) now lives in `workstate-schema` alongside the
// model and the canonical path, so the producer and all consumers share one I/O
// path. `build()` here stays pure (side-effect-free, testable); `main` calls
// `workstate::write_snapshot` (re-exported from the schema crate) to persist.

use crate::ingest::bulwark::BulwarkFeed;
use crate::ingest::proto::ProtoFeed;
use crate::ingest::scriptvault::ScriptVaultFeed;
use crate::ingest::toolfoundry::ToolFoundryFeed;
use crate::ingest::{FeedError, FeedSource};
use crate::model::normalized::DroppedCount;
use crate::model::provenance::{FeedStatus, Provenance, Section};
use crate::model::snapshot::Snapshot;

const STALE_AFTER: Duration = Duration::hours(24);

/// Builds a `Snapshot` from the project's fixed set of feed sources.
///
/// Three concrete feed fields: the `Snapshot`
/// has three FIXED, differently-typed, named sections (`scripts`, `tools`,
/// `findings`). Each feed produces a DIFFERENT canonical type via its associated
/// `Normalized` type, so a heterogeneous list of trait objects couldn't be placed
/// into those typed fields anyway. Holding the adapters as concrete fields keeps
/// everything statically typed; the shared fetch/error/normalize/wrap logic is
/// factored into one generic helper (`compile_section`) so we don't repeat it.
pub struct SnapshotBuilder {
    bulwark: BulwarkFeed,
    scriptvault: ScriptVaultFeed,
    toolfoundry: ToolFoundryFeed,
    proto: ProtoFeed,
}

impl SnapshotBuilder {
    /// Construct the builder from its three feed adapters.
    pub fn new(
        bulwark: BulwarkFeed,
        scriptvault: ScriptVaultFeed,
        toolfoundry: ToolFoundryFeed,
        proto: ProtoFeed,
    ) -> Self {
        SnapshotBuilder {
            bulwark,
            scriptvault,
            toolfoundry,
            proto,
        }
    }

    /// Compile the master snapshot from all feeds.
    ///
    /// Returns a `Snapshot` (NOT a `Result`): a degraded build is
    /// still a valid snapshot — failures are recorded per-section, never raised.
    ///
    /// The shape is now three explicit, type-safe calls — one per typed section —
    /// each delegating the uniform "fetch -> status -> normalize -> wrap" dance to
    /// `compile_section`. Adding a 4th feed means adding a field, a `Snapshot`
    /// field, and one more line here: visible and honest, no hidden registry.
    pub fn build(&self) -> Snapshot {
        // when this compile ran (UTC truth)
        let built_at = Utc::now();
        // Order matches Snapshot::new's signature (scripts, tools, findings, jobs).
        // Bulwark feeds the `findings` slot; scriptvault -> scripts, toolfoundry ->
        // tools, proto -> jobs. The static types make a wrong order a compile error.
        Snapshot::new(
            built_at,
            compile_section(&self.scriptvault),
            compile_section(&self.toolfoundry),
            compile_section(&self.bulwark),
            compile_section(&self.proto),
        )
    }
}

/// The ONE place the per-feed degradation dance lives, written generically over
/// any `FeedSource`. Fetch the feed; on success normalize and wrap as a healthy
/// section; on a typed `FeedError` map it to the matching `FeedStatus` and wrap an
/// empty section. Every error path still yields
/// a valid `Section`.
///
/// Generic (`F: FeedSource`) rather than `dyn` so each call resolves to a concrete
/// `F::Normalized` — exactly the type the matching Snapshot section expects.
// `F::Normalized: DroppedCount` so the generic compiler can lift each inventory's
// dropped-record count onto provenance without knowing the concrete type.
fn compile_section<F: FeedSource>(feed: &F) -> Section<F::Normalized>
where
    F::Normalized: DroppedCount,
{
    let feed_id = feed.feed_id(); // provenance: who this section came from

    match feed.fetch() {
        // Feed read cleanly: gate the version, normalize supported data, and
        // attach freshness/provenance.
        Ok(raw) => {
            let fetched_at = Utc::now();
            let source_observed_at = feed.source_observed_at(&raw);

            // A helper for the reject paths below: every gate that drops the feed
            // builds the same provenance shape (no data was normalized, so the drop
            // count is 0). Kept as a closure so each early return stays a one-liner.
            // `feed_id` is moved in by whichever reject path (or the success path)
            // actually runs; only one ever does.
            let reject_provenance = |feed_id| Provenance {
                feed_id,
                fetched_at: Some(fetched_at),
                source_observed_at,
                dropped_records: 0,
            };

            // SOURCE-TOOL CROSS-CHECK (#5): if this adapter knows what tool it
            // expects and the feed self-reports a DIFFERENT, non-empty `source_tool`,
            // reject it. This catches a misconfigured/swapped feed (e.g. the Bulwark
            // path pointed at a ScriptVault export) before its bytes get normalized
            // and stamped with the wrong feed_id in the source-of-truth snapshot. An
            // EMPTY source_tool is tolerated (not a mismatch), matching prior leniency.
            if let (Some(expected), Some(found)) =
                (feed.expected_source_tool(), feed.source_tool(&raw))
            {
                let found = found.trim();
                if !found.is_empty() && !found.eq_ignore_ascii_case(expected) {
                    return Section {
                        status: FeedStatus::SourceMismatch {
                            expected: expected.to_string(),
                            found: found.to_string(),
                        },
                        provenance: reject_provenance(feed_id),
                        data: None,
                    };
                }
            }

            // Version gate POLICY (deliberate, strict): a feed is accepted only
            // when its declared schema_version EXACTLY matches the one supported.
            // Both failure cases drop the data — Workstate is the single source of
            // truth RexOps consumes, so it refuses to bake an unverified shape into
            // the snapshot. This is intentionally STRICTER than RexOps' display
            // adapter, which tolerates unknown versions. The two cases are kept
            // STRUCTURALLY DISTINCT (#7):
            //   * a MISSING version (field absent)  -> `MissingVersion`,
            //   * a known-but-WRONG version (e.g. 99) -> `UnsupportedVersion`.
            // so a consumer can tell "producer forgot to stamp a version" from
            // "producer stamped one we don't support". (In practice every producer
            // stamps schema_version, so the missing arm is a guard, not a hot path.)
            if let Some(supported) = feed.supported_schema_version() {
                match feed.schema_version(&raw) {
                    // Declared the supported version: passes the gate.
                    Some(found) if found == supported => {}
                    // Declared a different version: wrong, but present.
                    Some(found) => {
                        return Section {
                            status: FeedStatus::UnsupportedVersion {
                                found: Some(found),
                                supported,
                            },
                            provenance: reject_provenance(feed_id),
                            data: None,
                        };
                    }
                    // No version declared at all: structurally distinct rejection.
                    None => {
                        return Section {
                            status: FeedStatus::MissingVersion { supported },
                            provenance: reject_provenance(feed_id),
                            data: None,
                        };
                    }
                }
            }

            // Normalize, then lift the adapter's drop count onto provenance so the
            // loss is visible even though the count physically rides on the data.
            let data = feed.normalize(raw);
            let dropped_records = data.dropped_records();
            Section {
                status: freshness_status(fetched_at, source_observed_at),
                provenance: Provenance {
                    feed_id,
                    fetched_at: Some(fetched_at),
                    source_observed_at,
                    dropped_records,
                },
                data: Some(data),
            }
        }
        // Feed absent: a Missing section (no data, no timestamp) — never a crash.
        Err(FeedError::NotFound(_)) => Section::missing(feed_id),
        // Feed present but unreadable/unparseable: a Failed section carrying the reason.
        Err(FeedError::Parse(reason)) => Section {
            status: FeedStatus::Failed { reason },
            provenance: Provenance {
                feed_id,
                fetched_at: None,         // we never got a usable read
                source_observed_at: None, // and so no source generation time either
                dropped_records: 0,       // nothing normalized -> nothing dropped
            },
            data: None,
        },
        // Unexpected I/O while reading: also Failed, with the io message as reason.
        Err(FeedError::Io(err)) => Section {
            status: FeedStatus::Failed {
                reason: err.to_string(),
            },
            provenance: Provenance {
                feed_id,
                fetched_at: None,
                source_observed_at: None,
                dropped_records: 0, // nothing normalized -> nothing dropped
            },
            data: None,
        },
    }
}

/// Decide a successfully-read feed's freshness from its source generation time.
///
/// FAILS CLOSED on unknown age. Three outcomes:
///   * source time known AND older than the threshold -> `Stale` (known old);
///   * source time known AND within the threshold      -> `Fresh`;
///   * source time UNKNOWN (`None`: the feed's `generated_at` was absent, empty,
///     or unparseable) -> `FreshnessUnknown`, NOT `Fresh`.
///
/// The last arm is the fix for a real bug: an unknown-age feed used to default to
/// `Fresh`, so a genuinely stale feed could look current. We cannot vouch for a
/// feed whose age we don't know, so it must not be labeled `Fresh` — consistent
/// with the version gate, which also fails closed on what it can't verify.
fn freshness_status(
    fetched_at: DateTime<Utc>,
    source_observed_at: Option<DateTime<Utc>>,
) -> FeedStatus {
    match source_observed_at {
        // Known age, past the threshold -> definitely stale.
        Some(source_observed_at)
            if fetched_at.signed_duration_since(source_observed_at) > STALE_AFTER =>
        {
            FeedStatus::Stale
        }
        // Known age, within the threshold -> fresh.
        Some(_) => FeedStatus::Fresh,
        // Unknown age -> cannot claim fresh; fail closed.
        None => FeedStatus::FreshnessUnknown,
    }
}
