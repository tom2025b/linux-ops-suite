// Declare the files in this directory as submodules. `pub` so the CLI and tests
// can reach into them; the loader, for instance, is called directly by commands.
pub mod checks; // Check, CheckProfile  (the built-in check suites)
pub mod detector; // ProjectType, detect_project_type  (automatic language detection)
pub mod error; // ProtoError + the crate Result alias's error half
pub mod executor; // execute_check / run_streaming, CheckStatus  (the one run engine)
pub mod feed; // WorkstateFeed, FeedItem  (the Workstate producer artifact)
pub mod loader; // discover / load / validate / find
pub mod protocol; // Protocol, Step, StepKind  (the recipe)
pub mod render; // Session -> Markdown (pure, for `proto export`)
pub mod session; // Session, StepResult, StepStatus  (the run)
pub mod store; // save / list / load sessions on disk (the run's persistence)
pub mod summary; // CheckOutcome -> human/structured summary (auto-check runs)

// Re-export the most-used types at `core::` level so callers don't need to know
// which file each lives in. This is the public "shape" of the domain.
pub use checks::{Check, CheckProfile};
pub use detector::{ProjectType, detect_project_type};
pub use error::ProtoError;
pub use feed::{FeedItem, WorkstateFeed};
pub use protocol::{Protocol, Step, StepKind};
pub use session::{Session, StepResult, StepStatus};
pub use summary::{CheckRunSummary, CheckSummaryItem, CheckSummaryStatus, OverallCheckStatus};
