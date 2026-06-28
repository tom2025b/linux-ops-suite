mod finding;
mod job;
mod script;
mod tool;

// Flatten the per-domain types back up to `model::normalized::*`. Existing
// consumers (`ingest`, `compile`, `snapshot`, the tests) import from this path
// and are unaffected by which file a type physically lives in.
pub use finding::{Finding, FindingId, FindingInventory, Severity};
pub use job::{Job, JobId, JobInventory, JobOutcome};
pub use script::{Script, ScriptId, ScriptInventory};
pub use tool::{Tool, ToolId, ToolInventory};

/// Exposes the per-inventory dropped-record count to generic code.
///
/// Each canonical inventory carries its own `dropped_records` field (set by the
/// adapter during normalization). `compile_section` is generic over the inventory
/// type, so it can't read that field directly — this one-method trait lets it lift
/// the count off ANY inventory uniformly and copy it onto `Provenance`. This keeps
/// the `FeedSource::normalize` signature unchanged: the count rides on the data the
/// adapter already returns, and this trait is just the read-out seam.
pub trait DroppedCount {
    /// How many raw records this inventory's normalization dropped.
    fn dropped_records(&self) -> usize;
}

impl DroppedCount for ScriptInventory {
    fn dropped_records(&self) -> usize {
        self.dropped_records
    }
}

impl DroppedCount for ToolInventory {
    fn dropped_records(&self) -> usize {
        self.dropped_records
    }
}

impl DroppedCount for FindingInventory {
    fn dropped_records(&self) -> usize {
        self.dropped_records
    }
}

impl DroppedCount for JobInventory {
    fn dropped_records(&self) -> usize {
        self.dropped_records
    }
}
