//! harness-core — the single source of truth for the unchanging infrastructure
//! shared across the yukineko Claude Code harness plugins.
//!
//! This crate is a BUILD-TIME dependency only: each plugin links it statically
//! into its self-contained binary, so the distributed `crates/<plugin>/bin/`
//! never references `../harness-core` at runtime.
//!
//! What lives here is the part that MUST be identical in every plugin —
//! especially the parallel-session-safe note store and the never-break-a-turn
//! hook wrapper (see the harness invariants). Plugin-specific domain logic and
//! config/metrics *fields* stay in each plugin crate and compose these.

pub mod config;
pub mod daily;
pub mod hook;
pub mod install;
pub mod interrogate;
pub mod metrics;
pub mod pricing;
pub mod projkey;
pub mod shell;
pub mod store;
pub mod transcript;
pub mod trust;
pub mod usage;
