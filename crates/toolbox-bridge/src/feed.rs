//! Write side: the versioned sidecar feed the bridge publishes back into
//! Workstate's feeds directory for ScriptVault to consume.
//!
//! Envelope follows CONTRACT_RULES.md exactly: integer `schema_version`,
//! `source_tool`, `generated_at` — same as the Bulwark/ToolFoundry/Proto
//! feeds. The schema lives at
//! `contracts/toolbox-bridge.workstate-feed.v1.schema.json` in the umbrella
//! repo; this module is its producer.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::convert::SidecarRecord;
use crate::error::BridgeError;

/// Major version of the feed contract. Bumped only on a breaking change.
pub const FEED_SCHEMA_VERSION: u32 = 1;
/// `source_tool` stamped into every feed this bridge emits.
pub const SOURCE_TOOL: &str = "toolbox-bridge";
/// Sentinel for `source_generated_at` when the upstream snapshot carried no
/// usable generation stamp. Honest non-timestamp value (not a fake date) so a
/// consumer cannot mistake "age unknown" for a real, recent scan time.
pub const UNKNOWN_SOURCE_TIME: &str = "unknown";

/// The sidecar feed envelope (one file, regenerated whole on every run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarFeed {
    pub schema_version: u32,
    pub source_tool: String,
    /// When the bridge generated this feed (RFC3339, UTC).
    pub generated_at: String,
    /// Bulwark's own generation stamp, carried through from the snapshot's
    /// findings section so ScriptVault can judge how old the underlying scan
    /// is — not just how recently the bridge ran. Never empty: a blank upstream
    /// stamp is normalized to [`UNKNOWN_SOURCE_TIME`] so a consumer reading this
    /// field always gets an honest value rather than `""`.
    pub source_generated_at: String,
    /// Number of records in `sidecars` (denormalized for cheap consumers).
    pub item_count: usize,
    pub sidecars: Vec<SidecarRecord>,
}

impl SidecarFeed {
    pub fn new(
        sidecars: Vec<SidecarRecord>,
        source_generated_at: &str,
        generated_at: DateTime<Utc>,
    ) -> Self {
        // Normalize a blank/whitespace upstream stamp to the sentinel rather
        // than emitting "": the field exists so ScriptVault can judge scan age,
        // and an empty string satisfies `required` while being useless.
        let source_generated_at = match source_generated_at.trim() {
            "" => UNKNOWN_SOURCE_TIME.to_string(),
            stamp => stamp.to_string(),
        };
        SidecarFeed {
            schema_version: FEED_SCHEMA_VERSION,
            source_tool: SOURCE_TOOL.to_string(),
            generated_at: generated_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            source_generated_at,
            item_count: sidecars.len(),
            sidecars,
        }
    }
}

/// The shared suite location for this feed:
/// `$XDG_DATA_HOME/workstate/feeds/toolbox-bridge.json` (fallback
/// `~/.local/share/...`) — the same `workstate/feeds/` directory the
/// Bulwark, ToolFoundry, and Proto feeds land in (see INTEGRATION_MAP.md).
pub fn default_feed_path() -> Result<PathBuf, BridgeError> {
    crate::snapshot::xdg_data_home()
        .map(|base| base.join("workstate/feeds/toolbox-bridge.json"))
        .ok_or(BridgeError::NoDefaultPath { what: "feed" })
}

/// Serialize the feed and publish it atomically, mirroring Workstate's own
/// snapshot writer: write a temp file in the destination directory, flush and
/// fsync it, then rename over the final path so a consumer can never observe
/// a half-written feed. Best-effort temp cleanup on failure.
pub fn write_feed(feed: &SidecarFeed, path: &Path) -> Result<(), BridgeError> {
    let display = path.display().to_string();
    let wrap = |source: std::io::Error| BridgeError::FeedWrite {
        path: display.clone(),
        source,
    };

    // Serialize before touching the filesystem; a serialize failure must not
    // leave artifacts behind. Pretty-printed: feeds are diffed and read by
    // humans as well as tools.
    let mut json = serde_json::to_vec_pretty(feed).map_err(|e| BridgeError::FeedWrite {
        path: display.clone(),
        source: std::io::Error::other(e),
    })?;
    json.push(b'\n');

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = dir {
        fs::create_dir_all(dir).map_err(wrap)?;
    }

    // Temp file in the SAME directory so the rename is same-filesystem
    // (atomic). PID + nanos keeps parallel runs from clobbering each other.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = path.with_file_name(format!(
        "{}.{}.{nanos}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "toolbox-bridge.json".to_string()),
        std::process::id(),
    ));

    let result = (|| {
        let mut file = File::create(&tmp).map_err(wrap)?;
        file.write_all(&json).map_err(wrap)?;
        file.sync_all().map_err(wrap)?;
        fs::rename(&tmp, path).map_err(wrap)?;
        // Make the rename durable; non-fatal if the platform refuses.
        if let Some(dir) = dir {
            if let Ok(d) = File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(path: &str) -> SidecarRecord {
        SidecarRecord {
            path: path.to_string(),
            tags: vec!["risk:high".to_string()],
            desc: None,
        }
    }

    #[test]
    fn feed_envelope_carries_contract_fields() {
        let feed = SidecarFeed::new(vec![record("/x/a.sh")], "2026-06-10", Utc::now());
        assert_eq!(feed.schema_version, 1);
        assert_eq!(feed.source_tool, "toolbox-bridge");
        assert_eq!(feed.item_count, 1);
        assert_eq!(feed.source_generated_at, "2026-06-10");
        // RFC3339 with explicit UTC marker.
        assert!(feed.generated_at.ends_with('Z'), "{}", feed.generated_at);
    }

    #[test]
    fn blank_source_stamp_normalizes_to_the_unknown_sentinel() {
        // An empty or whitespace upstream generated_at must not surface as "" —
        // it becomes the honest "unknown" sentinel so a consumer can't read a
        // blank as a real recent scan time.
        for blank in ["", "   ", "\t"] {
            let feed = SidecarFeed::new(vec![record("/x/a.sh")], blank, Utc::now());
            assert_eq!(feed.source_generated_at, UNKNOWN_SOURCE_TIME);
        }
        // A real stamp is preserved (and trimmed).
        let feed = SidecarFeed::new(vec![record("/x/a.sh")], "  2026-06-10  ", Utc::now());
        assert_eq!(feed.source_generated_at, "2026-06-10");
    }

    #[test]
    fn write_feed_round_trips_and_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/feeds/toolbox-bridge.json");
        let feed = SidecarFeed::new(vec![record("/x/a.sh")], "2026-06-10", Utc::now());

        write_feed(&feed, &path).expect("write");

        let text = std::fs::read_to_string(&path).expect("read back");
        let parsed: SidecarFeed = serde_json::from_str(&text).expect("parse");
        assert_eq!(parsed, feed);
        // No temp litter left behind.
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "leftover files: {entries:?}");
    }
}
