//! Default [`CompactionGuard`] — PreCompact backstop. Snapshots the transcript +
//! records verbatim spans, then **proceeds** (compression is delegated to
//! built-in compaction; no self-summarization). Block is reserved for the rare
//! case where the snapshot itself could not be secured.

use crate::handlers::{BackingStore, CompactDecision, CompactionGuard};
use harness_core::hook::HookInput;

pub struct DefaultGuard;

impl CompactionGuard for DefaultGuard {
    fn on_pre_compact(&mut self, i: &HookInput, s: &mut dyn BackingStore) -> CompactDecision {
        let _ = (i, s);
        todo!("Phase 2: snapshot transcript + verbatim to store, then Proceed (backstop)")
    }
}
