//! Default [`Checkpointer`] — Stop / SubagentStop externalization. Writes
//! completed work to the backing store under a threshold gate. **Side effects
//! only**: the bin discards any result and exits 0, so this must never block
//! (the per-session block cap short-circuits the session after repeated blocks).

use crate::handlers::{BackingStore, Checkpointer};
use harness_core::hook::HookInput;
use harness_core::transcript::last_usage_tokens;

const DEFAULT_CHECKPOINT_THRESHOLD: u64 = 10_000;

pub struct DefaultCheckpointer;

impl Checkpointer for DefaultCheckpointer {
    fn checkpoint(&mut self, i: &HookInput, s: &mut dyn BackingStore) {
        // Re-entrant Stop (continuation of a prior Stop hook): do nothing.
        if i.stop_hook_active {
            return;
        }
        let threshold = std::env::var("CONTEXT_GOVERNOR_CHECKPOINT_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CHECKPOINT_THRESHOLD);
        // Only externalize once the conversation is heavy enough to be worth it.
        if last_usage_tokens(&i.transcript_path).unwrap_or(0) >= threshold {
            s.snapshot_transcript(&i.transcript_path);
        }
    }
}
