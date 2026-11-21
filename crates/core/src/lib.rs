//! Core library for AccountSweep.
//!
//! The tool does all deterministic work (grouping, classification, state);
//! the agent only relays questions, reads through its connectors and acts in
//! the browser. See `SPEC.md` §0 and §5.

pub mod classify;
pub mod error;
pub mod flow;
pub mod ingest;
pub mod model;
pub mod output;
pub mod rules;
pub mod schemas;
pub mod store;

pub use error::{Error, Result};

/// Version of this crate, shared by the whole workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
