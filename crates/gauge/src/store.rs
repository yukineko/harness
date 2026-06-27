//! The session record store.
//!
//! The record type and its read/write helpers now live in
//! `harness_core::session` (shared with the other plugins so cost/turn/tool
//! numbers can't drift). This module re-exports them so gauge's internal call
//! sites are unchanged; gauge owns the *write* path via its Stop hook.

pub use harness_core::session::{load_all, upsert, SessionRecord, Usage};
