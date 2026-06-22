//! Per-session transcript aggregation.
//!
//! The aggregation logic now lives in `harness_core::usage` so any plugin can
//! compute a session's per-model token tallies, tool counts, and timespan from
//! a transcript without depending on gauge. This module re-exports it so gauge's
//! call sites (`transcript::aggregate`, `transcript::Aggregate`) are unchanged.

#[allow(unused_imports)]
pub use harness_core::usage::{aggregate, Aggregate};
