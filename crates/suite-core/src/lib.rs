//! `suite-core` — the shared, dependency-free foundation for the Linux Ops
//! Suite's tool crates.
//!
//! Every tool (conductor, tripwire, rewind, portman, rex-check, rex-doctor,
//! pulse, …) used to carry its own byte-identical copy of these helpers. They
//! all do exactly one small thing each, with no third-party dependencies, so
//! they belong in one place:
//!
//! - [`env`] — is stdout a TTY, are we root, where is `$HOME`.
//! - [`path`] — resolve an executable on `$PATH`, test the exec bit.
//! - [`xdg`] — the suite's per-tool `$XDG_DATA_HOME` / `$XDG_CONFIG_HOME`
//!   directories, plus leading-`~` expansion.
//! - [`fmt`] — human-readable byte sizes (`2.1 KB`).
//!
//! This crate deliberately has **no dependencies** (std + two libc externs
//! only) and no UI code; terminal chrome lives in `thomas-tui` / `suite-ui`.

pub mod env;
pub mod fmt;
pub mod path;
pub mod xdg;
