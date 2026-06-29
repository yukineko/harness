//! Default [`BackingStore`] backed by the durable note store
//! (`harness_core::store`), which is the parallel-session-safe externalization
//! substrate the rest of the harness already uses.
//!
//! Phase 1 freezes the type and its construction seam; the round-trip bodies are
//! `todo!()` until Phase 2 (rehydrator + guard), which is where snapshot/recall
//! first run for real.

use std::path::PathBuf;

use crate::handlers::BackingStore;
use crate::types::{ContextItem, StoreKey};

/// Externalizes context to the note store rooted at `state_dir`. Keyed by
/// session so parallel sessions never clobber each other's snapshots (the same
/// fallback discipline `harness_core::store` enforces).
pub struct TranscriptBackingStore {
    #[allow(dead_code)] // wired in Phase 2; the field pins the construction seam.
    state_dir: PathBuf,
}

impl TranscriptBackingStore {
    /// Open (or lazily create) the store under `cwd`. The dispatch binary calls
    /// this once per invocation.
    pub fn open(cwd: &str) -> anyhow::Result<Self> {
        let _ = cwd;
        todo!("Phase 2: resolve state_dir from cwd via harness_core::store")
    }
}

impl BackingStore for TranscriptBackingStore {
    fn snapshot_transcript(&mut self, transcript_path: &str) -> StoreKey {
        let _ = transcript_path;
        todo!("Phase 2: copy the transcript span into the note store, return its key")
    }

    fn put(&mut self, item: &ContextItem) -> StoreKey {
        let _ = item;
        todo!("Phase 2: externalize the item losslessly, return its key")
    }

    fn recall(&self, key: &StoreKey) -> Option<ContextItem> {
        let _ = key;
        todo!("Phase 2: read the externalized item back by key")
    }
}
