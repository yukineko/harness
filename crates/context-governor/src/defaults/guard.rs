//! Default [`CompactionGuard`] — PreCompact backstop. Snapshots the transcript +
//! records verbatim spans, then **proceeds** (compression is delegated to
//! built-in compaction; no self-summarization). Block is reserved for the rare
//! case where the snapshot itself could not be secured.

use crate::handlers::{BackingStore, CompactDecision, CompactionGuard};
use harness_core::hook::HookInput;

pub struct DefaultGuard;

impl CompactionGuard for DefaultGuard {
    fn on_pre_compact(&mut self, i: &HookInput, s: &mut dyn BackingStore) -> CompactDecision {
        // Secure a transcript snapshot before built-in compaction runs (I1).
        // snapshot_transcript is fail-soft (empty/missing transcript → no-op), so
        // the backstop proceeds unconditionally; Block stays reserved for a future
        // case where the snapshot genuinely could not be secured.
        s.snapshot_transcript(&i.transcript_path);
        CompactDecision::Proceed
    }
}
