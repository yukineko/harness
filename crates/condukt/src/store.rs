//! Per-project addressing for run-state files.
//!
//! The implementation now lives in `harness_core::projkey` (the single source of
//! truth) so sibling plugins — notably autoflow, which reads condukt's run-state
//! directory — derive byte-identical keys and can never drift. Re-exported here
//! so existing `crate::store::{project_key, repo_root, fnv1a32}` call sites keep
//! working unchanged.

pub use harness_core::projkey::{project_key, repo_root};
