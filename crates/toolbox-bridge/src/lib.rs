//! Toolbox-Bridge: Bulwark -> Workstate -> ScriptVault, with no direct
//! tool-to-tool communication.
//!
//! The bridge is a pure adapter between two Workstate artifacts:
//!
//! 1. **Read** Bulwark's findings from the compiled Workstate snapshot
//!    (`workstate.snapshot.json`) — never from Bulwark itself.
//! 2. **Convert** each finding into ScriptVault sidecar metadata
//!    (`tags: [risk:…, owner:…]` + a `desc` with a risk badge), the same
//!    shape ScriptVault's `.scriptvault.yaml` sidecars use.
//! 3. **Write** the records as a versioned Workstate feed
//!    (`feeds/toolbox-bridge.json`) for ScriptVault to consume.
//!
//! The pipeline mirrors Workstate's own ingest -> normalize -> write split:
//! [`snapshot`] reads, [`convert`] is pure, [`feed`] writes atomically.

pub mod convert;
pub mod error;
pub mod feed;
pub mod snapshot;

// Convenience re-exports of the types that form the bridge's public surface.
pub use convert::{convert, Conversion, SidecarRecord};
pub use error::BridgeError;
pub use feed::{write_feed, SidecarFeed};
pub use snapshot::{findings_view, load_snapshot, FindingsView};
