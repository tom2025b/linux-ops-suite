//! Core library for ToolFoundry manifests, validation, lifecycle, install, and Workstate export.
//!
//! The core crate owns typed data models and deterministic reports. The CLI crate owns argument
//! parsing, terminal output, and top-level `anyhow` error handling.

#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

pub mod config;
pub mod error;
pub mod health;
pub mod install;
pub mod lifecycle;
pub mod manifest;
pub mod paths;
pub mod registry;
pub mod tui;
pub mod workstate;

#[cfg(test)]
mod test_support;
