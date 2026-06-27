// The two top-level modules. `core` is the CLI-agnostic domain; `cli` is the
// argument parsing + command handlers built ON TOP of core.
pub mod cli; // Cli/Commands definitions and the per-command handlers
pub mod core; // models + loader (no I/O of args, no stdout formatting policy)

// -----------------------------------------------------------------------------
// Crate-wide Result alias.
// -----------------------------------------------------------------------------
// Almost every fallible function in the library returns the same error type,
// `ProtoError`. Aliasing `Result<T> = std::result::Result<T, ProtoError>` lets
// those signatures read `-> Result<Protocol>` instead of repeating the error
// type everywhere. (The standard-library `Result` is still reachable as
// `std::result::Result` where a different error type is needed.)
pub type Result<T> = std::result::Result<T, core::error::ProtoError>;

// -----------------------------------------------------------------------------
// Public surface re-exports.
// -----------------------------------------------------------------------------
// Lift the most-used domain types to the crate root so external code and main.rs
// can use the short path `proto::Protocol`, `proto::Session`, etc. This is the
// library's "front door"; the module structure behind it can change freely.
pub use core::{
    CheckRunSummary, CheckSummaryItem, CheckSummaryStatus, FeedItem, OverallCheckStatus,
    ProtoError, Protocol, Session, Step, StepKind, StepResult, StepStatus, WorkstateFeed,
};
