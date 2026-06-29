//! Default [`ToolResultGroomer`] — the primary size lever (PostToolUse).
//!
//! Phase 2 (first, per force-priority): trim/summary-replace a bloated tool
//! result so the window's dominant growth term flattens (I4). The replacement
//! must be strictly smaller than the input; correctness is free here because the
//! input is an [`Evictable`] (never `Pinned`/`Verbatim`).
//!
//! The groomer is deliberately tokenizer-free and deterministic: it estimates
//! tokens at ~4 chars each, and when a result exceeds the budget it keeps the
//! head and tail and elides the middle (where tool output is least load-bearing)
//! behind a single marker. No model call, no API key — pure string surgery.

use crate::handlers::ToolResultGroomer;
use crate::io::HookOutput;
use crate::types::{ContextItem, Evictable, ItemBody, ItemId, Lane};
use harness_core::hook::HookInput;

/// Default token budget for a single tool result before grooming engages.
/// Results at or under this pass through untouched; larger ones are head/tail
/// trimmed. Override per-environment with `CONTEXT_GOVERNOR_GROOM_BUDGET`.
const DEFAULT_GROOM_BUDGET_TOKENS: u32 = 2048;

/// Rough, deterministic token estimate (~4 chars/token, tokenizer-free). Any
/// non-empty body is at least 1 token so a tiny string never reads as free.
fn est_tokens(s: &str) -> u32 {
    let chars = s.chars().count();
    if chars == 0 {
        0
    } else {
        u32::try_from(chars.div_ceil(4).max(1)).unwrap_or(u32::MAX)
    }
}

/// Resolve the active grooming budget (env override → default).
fn groom_budget() -> u32 {
    std::env::var("CONTEXT_GOVERNOR_GROOM_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_GROOM_BUDGET_TOKENS)
}

/// Keep the head and tail of `body`, eliding the middle, so the result fits
/// roughly `budget` tokens. Deterministic and split only on char boundaries. May
/// return a string no smaller than the input when the budget leaves nothing to
/// elide — the caller enforces I4 and declines in that case.
fn trim_middle(body: &str, budget: u32) -> String {
    let chars: Vec<char> = body.chars().collect();
    let total = chars.len();
    // ~chars affordable for `budget` tokens, split evenly head/tail.
    let keep = (budget as usize).saturating_mul(4);
    let head_len = keep / 2;
    let tail_len = keep / 2;
    if head_len + tail_len >= total {
        return body.to_string(); // nothing to elide; caller rechecks I4
    }
    let elided = total - head_len - tail_len;
    let head: String = chars[..head_len].iter().collect();
    let tail: String = chars[total - tail_len..].iter().collect();
    format!("{head}\n…[context-governor: elided {elided} chars of {total}]…\n{tail}")
}

pub struct DefaultGroomer;

impl ToolResultGroomer for DefaultGroomer {
    fn groom(&self, tool_output: Evictable<'_>, budget: u32) -> Option<serde_json::Value> {
        // Only an inline body occupies the window; a Ref is already externalized.
        let body = match &tool_output.item().body {
            ItemBody::Inline(s) => s,
            ItemBody::Ref(_) => return None,
        };
        // Under budget → leave untouched (the common case).
        if tool_output.item().tokens <= budget {
            return None;
        }
        let trimmed = trim_middle(body, budget);
        // I4: the replacement must be *strictly* smaller than the input. If the
        // trim could not shrink it (degenerate budget, or the marker outweighs
        // what was elided), decline rather than grow the window.
        if trimmed.chars().count() >= body.chars().count() {
            return None;
        }
        Some(serde_json::Value::String(trimmed))
    }
}

impl DefaultGroomer {
    /// Bin entry point: read `input.tool_response`, wrap it as an `Evictable`,
    /// groom under the active budget, and emit a PostToolUse `updatedToolOutput`
    /// envelope (or `{}` when nothing is groomed).
    pub fn to_output(&self, input: &HookInput) -> HookOutput {
        self.to_output_with_budget(input, groom_budget())
    }

    /// Budget-injectable core of [`Self::to_output`] — lets tests pin a budget
    /// without touching the process-global env var.
    fn to_output_with_budget(&self, input: &HookInput, budget: u32) -> HookOutput {
        let Some(resp) = &input.tool_response else {
            return HookOutput::default();
        };
        // The string that actually occupies the window: a JSON string result is
        // its own text; anything structured is rendered compactly.
        let body = match resp {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let item = ContextItem {
            id: ItemId(0),
            lane: Lane::Evictable,
            tokens: est_tokens(&body),
            body: ItemBody::Inline(body),
        };
        // Infallible by construction (the item is `Lane::Evictable`), but we
        // honor the capability constructor rather than asserting.
        let Some(ev) = Evictable::new(&item) else {
            return HookOutput::default();
        };
        match self.groom(ev, budget) {
            Some(groomed) => HookOutput::groomed(groomed),
            None => HookOutput::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn evictable_item(body: &str) -> ContextItem {
        ContextItem {
            id: ItemId(0),
            lane: Lane::Evictable,
            tokens: est_tokens(body),
            body: ItemBody::Inline(body.to_string()),
        }
    }

    #[test]
    fn under_budget_passes_through() {
        let item = evictable_item("short result");
        let ev = Evictable::new(&item).unwrap();
        // tokens(~3) well under a generous budget → untouched.
        assert_eq!(DefaultGroomer.groom(ev, 2048), None);
    }

    #[test]
    fn over_budget_trims_strictly_smaller() {
        let big = "x".repeat(40_000); // ~10k tokens
        let item = evictable_item(&big);
        let ev = Evictable::new(&item).unwrap();
        let got = DefaultGroomer
            .groom(ev, 256)
            .expect("over budget must groom");
        let s = got.as_str().unwrap();
        // I4: strictly smaller than the input...
        assert!(s.chars().count() < big.chars().count());
        // ...and it carries the elision marker (head/tail kept, middle dropped).
        assert!(s.contains("context-governor: elided"));
    }

    #[test]
    fn ref_body_is_not_groomed() {
        // An externalized body holds no inline window cost to reclaim.
        let item = ContextItem {
            id: ItemId(0),
            lane: Lane::Evictable,
            tokens: 9999,
            body: ItemBody::Ref(crate::types::StoreKey(1)),
        };
        let ev = Evictable::new(&item).unwrap();
        assert_eq!(DefaultGroomer.groom(ev, 1), None);
    }

    fn post_tool_use(resp: Option<serde_json::Value>) -> HookInput {
        let mut obj = serde_json::json!({
            "session_id": "s",
            "transcript_path": "",
            "cwd": "",
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash"
        });
        if let Some(r) = resp {
            obj["tool_response"] = r;
        }
        serde_json::from_value(obj).unwrap()
    }

    #[test]
    fn to_output_without_tool_response_is_empty() {
        let input = post_tool_use(None);
        assert_eq!(
            DefaultGroomer.to_output_with_budget(&input, 256).to_json(),
            "{}"
        );
    }

    #[test]
    fn to_output_under_budget_is_empty() {
        let input = post_tool_use(Some(serde_json::json!("a small tool result")));
        assert_eq!(
            DefaultGroomer.to_output_with_budget(&input, 2048).to_json(),
            "{}"
        );
    }

    #[test]
    fn to_output_over_budget_emits_post_tool_use_envelope() {
        let big = "y".repeat(40_000);
        let input = post_tool_use(Some(serde_json::Value::String(big)));
        let json = DefaultGroomer.to_output_with_budget(&input, 256).to_json();
        assert!(json.contains("\"hookEventName\":\"PostToolUse\""));
        assert!(json.contains("\"updatedToolOutput\""));
        assert!(json.contains("context-governor: elided"));
    }

    proptest! {
        /// I4, exhaustively: whatever the body and budget, a groom result is
        /// never larger than the input — the groomer only ever returns a
        /// strictly-smaller string, never grows the window.
        #[test]
        fn groom_never_grows_the_window(body in ".{0,5000}", budget in 0u32..4096) {
            let item = evictable_item(&body);
            let ev = Evictable::new(&item).unwrap();
            if let Some(v) = DefaultGroomer.groom(ev, budget) {
                let s = v.as_str().expect("groomed value is a JSON string");
                prop_assert!(s.chars().count() < body.chars().count());
            }
        }
    }
}
