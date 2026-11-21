//! Core library for AccountSweep.
//!
//! M0 scope: the versioned JSON Schemas that define the contract between the
//! tool and an agent, and the output envelope every `--json` command prints.
//! Scanning, enrichment and the workflow state machine arrive in later
//! milestones (see `SPEC.md` §13).

pub mod output;
pub mod schemas;

/// Version of this crate, shared by the whole workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
