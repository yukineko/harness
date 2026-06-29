//! Default [`ToolResultGroomer`] — the primary size lever (PostToolUse).
//!
//! Phase 2 (first, per force-priority): trim/summary-replace a bloated tool
//! result so the window's dominant growth term flattens (I4). The replacement
//! must be strictly smaller than the input; correctness is free here because the
//! input is an [`Evictable`] (never `Pinned`/`Verbatim`).

use crate::handlers::ToolResultGroomer;
use crate::io::HookOutput;
use crate::types::Evictable;
use harness_core::hook::HookInput;

pub struct DefaultGroomer;

impl ToolResultGroomer for DefaultGroomer {
    fn groom(&self, tool_output: Evictable<'_>, budget: u32) -> Option<serde_json::Value> {
        let _ = (tool_output, budget);
        todo!("Phase 2: trim/summary-replace the tool result under `budget` (I4)")
    }
}

impl DefaultGroomer {
    /// Bin entry point: read `input.tool_response`, wrap it as an `Evictable`,
    /// groom under budget, and emit a PostToolUse `updatedToolOutput` envelope
    /// (or `{}` when nothing is groomed). Phase 1 freezes the seam.
    pub fn to_output(&self, input: &HookInput) -> HookOutput {
        let _ = input;
        todo!("Phase 2: build Evictable from tool_response, groom, emit updatedToolOutput")
    }
}
