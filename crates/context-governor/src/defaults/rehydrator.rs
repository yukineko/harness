//! Default [`StateRehydrator`] — SessionStart restore. Re-injects normative core
//! / verbatim from the backing store so pins survive compaction (I1) and resume
//! reseeds durably. Most relevant on `source == "compact"`.

use crate::backing::SNAPSHOT_KEY;
use crate::handlers::{BackingStore, StateRehydrator};
use crate::io::HookOutput;
use crate::types::ItemBody;
use harness_core::hook::HookInput;

pub struct DefaultRehydrator;

impl StateRehydrator for DefaultRehydrator {
    fn rehydrate(&self, _i: &HookInput, s: &dyn BackingStore) -> HookOutput {
        match s.recall(&SNAPSHOT_KEY) {
            Some(item) => match item.body {
                ItemBody::Inline(text) if !text.is_empty() => {
                    HookOutput::inject("SessionStart", text)
                }
                _ => HookOutput::default(),
            },
            None => HookOutput::default(),
        }
    }
}
