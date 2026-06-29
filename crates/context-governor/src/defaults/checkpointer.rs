//! Default [`Checkpointer`] — Stop / SubagentStop externalization. Writes
//! completed work to the backing store under a threshold gate. **Side effects
//! only**: the bin discards any result and exits 0, so this must never block
//! (the per-session block cap short-circuits the session after repeated blocks).

use crate::handlers::{BackingStore, Checkpointer};
use harness_core::hook::HookInput;

pub struct DefaultCheckpointer;

impl Checkpointer for DefaultCheckpointer {
    fn checkpoint(&mut self, i: &HookInput, s: &mut dyn BackingStore) {
        let _ = (i, s);
        todo!("Phase 2: externalize completed work under the threshold gate (no block)")
    }
}
