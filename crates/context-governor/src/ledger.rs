//! Append-only action ledger (§13), with size metrics.
//!
//! Distinct from `harness_core::ledger` (that one is budgetguard's *daily spend*
//! ledger). This records, for every hook decision, a single node — satisfying
//! I6 (observability): each hook judgement leaves exactly one
//! injected / groomed{saved} / snapshotted / pinned / recalled trace. Per turn
//! the governor records `resident_tokens`, `groom saved_tokens`, and the growth
//! slope, then ships them to the metrics sink (beacon/Langfuse) via
//! `harness_core::metrics::emit`.

use crate::types::{ItemId, StoreKey};

/// What a hook did, with the size delta where one applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Reference body / pins injected beside the prompt (size: retrieval).
    Injected,
    /// A tool result trimmed — `saved_tokens` is the size reclaimed (I4).
    Groomed { saved_tokens: u32 },
    /// Transcript/verbatim externalized to the backing store (correctness).
    Snapshotted { to: StoreKey },
    /// A pin re-asserted into the final context (I1).
    Pinned,
    /// An externalized item pulled back in (lossless round-trip, I2).
    Recalled { from: StoreKey },
}

/// One append-only ledger node. `reason` is a `&'static str` so the cause is a
/// fixed vocabulary, not free text — the ledger stays queryable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerNode {
    pub session: String,
    pub hook: String,
    pub item: Option<ItemId>,
    pub action: Action,
    pub reason: &'static str,
}
