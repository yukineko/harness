//! Default [`StateRehydrator`] — SessionStart restore. Re-injects normative core
//! / verbatim from the backing store so pins survive compaction (I1) and resume
//! reseeds durably. Most relevant on `source == "compact"`.

use crate::handlers::{BackingStore, StateRehydrator};
use crate::io::HookOutput;
use harness_core::hook::HookInput;

pub struct DefaultRehydrator;

impl StateRehydrator for DefaultRehydrator {
    fn rehydrate(&self, i: &HookInput, s: &dyn BackingStore) -> HookOutput {
        let _ = (i, s);
        todo!("Phase 2: recall pinned/verbatim from store, emit additionalContext (I1)")
    }
}
