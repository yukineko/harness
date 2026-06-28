//! The Claude Code hook stdin payload. Consolidated onto the canonical
//! [`harness_core::hook::HookInput`], which provides `parse`, `cwd_or_current`,
//! `session_key`, and the `stop_hook_active` field tdd's gate relies on.

pub use harness_core::hook::HookInput;
