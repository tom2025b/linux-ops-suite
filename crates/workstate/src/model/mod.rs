//! The producer's view of the model.
//!
//! `raw` (untrusted upstream feed-INPUT shapes) is the only model that lives in
//! the `workstate` producer. The published contract — `normalized`, `provenance`,
//! `snapshot` — lives in the `workstate-schema` crate and is RE-EXPORTED here at
//! the same `workstate::model::...` paths consumers and internal code already use,
//! so no import path had to change when the contract was extracted.
pub mod raw; // RawFeed — untrusted upstream shapes (ingestion input).

pub use workstate_schema::model::{normalized, provenance, snapshot};
