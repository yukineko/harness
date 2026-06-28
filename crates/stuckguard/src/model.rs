//! The Claude Code hook stdin payload. Consolidated onto the canonical
//! [`harness_core::hook::HookInput`], which provides `parse`, `cwd_or_current`,
//! `session_key`, and the `tool_name`/`tool_input`/`tool_response` fields
//! stuckguard inspects on PostToolUse.

pub use harness_core::hook::HookInput;
