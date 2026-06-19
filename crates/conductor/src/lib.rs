//! conductor — the Linux Ops Suite's guided operator.
//!
//! Phase 1 (this build) is the Ring 0, read-only foundation: read the suite's
//! contract files, derive a deterministic ordered plan, and render it. The
//! library does the work and returns values; the binary only parses flags and
//! prints. See `CONDUCTOR_DESIGN.md` at the repo root.

pub mod error;
pub mod util;

pub use error::ConductorError;
