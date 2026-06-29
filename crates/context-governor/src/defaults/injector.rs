//! Default [`ContextInjector`] — retrieval / reference-body injection
//! (UserPromptSubmit). Injects the relevant reference body + pins beside the
//! prompt (reduce-before). Phase 2 wires the selection; Phase 3 may wrap
//! `playbook` behind this trait.

use crate::handlers::ContextInjector;
use crate::io::HookOutput;
use harness_core::hook::HookInput;

pub struct DefaultInjector;

impl ContextInjector for DefaultInjector {
    fn inject(&self, input: &HookInput) -> HookOutput {
        let _ = input;
        todo!("Phase 2: select relevant reference body + pins, emit additionalContext")
    }
}
