//! Hook I/O envelope.
//!
//! The *input* side is [`harness_core::hook::HookInput`] (re-exported from the
//! crate root) — not redefined here. That struct already carries the real
//! Claude Code payload schema, and three field-name facts the brief's draft got
//! slightly wrong are worth pinning down as contract notes:
//!
//! * the PostToolUse result field is **`tool_response`**, not `tool_output`;
//!   the groomer reads `input.tool_response`.
//! * there is no `last_assistant_message` field — the Stop/SubagentStop
//!   checkpointer reads the last turn from the transcript
//!   (`harness_core::transcript::recent_turns`) instead.
//! * `source` ("startup"|"resume"|"clear"|"compact") and `trigger`
//!   ("auto"|"manual") are already present for SessionStart and PreCompact.
//!
//! Only the *output* side lives here: a typed envelope for the two write-backs
//! the governor performs — `additionalContext` (UserPromptSubmit / SessionStart)
//! and `updatedToolOutput` (PostToolUse). Existing plugins hand-roll this as a
//! `serde_json::json!` literal; we give it a type so the contract is explicit
//! and the field renames are checked once.

use serde::Serialize;

/// Top-level hook response. All fields are optional: the default (everything
/// `None`) serializes to `{}`, which Claude Code reads as "no-op, proceed" — the
/// correct response for the common case where a handler decides to do nothing.
#[derive(Serialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct HookOutput {
    /// `false` asks Claude Code to stop. The governor leaves this `None`
    /// (proceed) on every event: blocking is expressed out-of-band (PreCompact
    /// exit 2), and Stop/SubagentStop must never block (I-adjacent: the block
    /// cap short-circuits the session after 8 consecutive blocks).
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub cont: Option<bool>,

    /// A short message surfaced to the user (not the model). Used sparingly,
    /// e.g. to report a snapshot was taken.
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,

    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub specific: Option<HookSpecific>,
}

/// The event-specific payload. `hook_event_name` echoes the event being
/// answered (Claude Code requires the echo); exactly one of the write-back
/// fields is set depending on the event.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct HookSpecific {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,

    /// UserPromptSubmit / SessionStart: reference body + relevant pins injected
    /// *beside* the prompt (the prompt itself is never replaced).
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,

    /// PostToolUse: the trimmed/summary-replaced tool result that takes the
    /// place of the bloated original in the window (the primary size lever).
    #[serde(rename = "updatedToolOutput", skip_serializing_if = "Option::is_none")]
    pub updated_tool_output: Option<serde_json::Value>,
}

impl HookOutput {
    /// An `additionalContext` injection for `event` (UserPromptSubmit /
    /// SessionStart). Inject *beside* the prompt; never a replacement.
    pub fn inject(event: &str, context: String) -> Self {
        HookOutput {
            specific: Some(HookSpecific {
                hook_event_name: event.to_string(),
                additional_context: Some(context),
                updated_tool_output: None,
            }),
            ..Default::default()
        }
    }

    /// A PostToolUse `updatedToolOutput` replacement (the groomed result).
    pub fn groomed(updated: serde_json::Value) -> Self {
        HookOutput {
            specific: Some(HookSpecific {
                hook_event_name: "PostToolUse".to_string(),
                additional_context: None,
                updated_tool_output: Some(updated),
            }),
            ..Default::default()
        }
    }

    /// Serialize to the JSON line a hook writes to stdout.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}
