use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A newtype for a script identifier. Same reasoning as `FeedId`: a named type
/// prevents mixing a script id up with some other string and documents intent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptId(pub String);

/// A canonical managed script as reported by ScriptVault.
///
/// `name` and `description` are `Option` because ScriptVault's records are not
/// guaranteed to carry them — an honest model says "maybe absent" rather than
/// substituting an empty string that downstream code can't distinguish from a
/// genuinely blank value. The `id` is mandatory: without identity, a record
/// cannot be a stable thing RexOps refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    /// Stable identity of the script.
    pub id: ScriptId,
    /// Human-readable name, if ScriptVault provided one.
    pub name: Option<String>,
    /// Free-text description, if present.
    pub description: Option<String>,
    /// Extra per-script fields ScriptVault emitted but Workstate does not model
    /// yet. Preserved so the snapshot remains a superset of the raw RexOps feed.
    #[serde(default, flatten)]
    pub rest: BTreeMap<String, Value>,
}

/// Canonical ScriptVault inventory plus the envelope-level UI state RexOps reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptInventory {
    /// ScriptVault's source generation string.
    pub generated_at: String,
    /// Normalized script records with stable ids.
    pub scripts: Vec<Script>,
    /// Favorite script ids from ScriptVault's export envelope.
    pub favorites: Vec<String>,
    /// Recently launched script ids from ScriptVault's export envelope.
    pub recents: Vec<String>,
    /// How many raw records normalization DROPPED (no usable id). The compiler
    /// copies this onto `Provenance.dropped_records` so the loss is never silent.
    /// `#[serde(default)]` so older snapshots without the field deserialize as `0`.
    #[serde(default)]
    pub dropped_records: usize,
}
