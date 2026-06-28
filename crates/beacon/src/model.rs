//! The Claude Code hook stdin payload. Consolidated onto the canonical
//! [`harness_core::hook::HookInput`], which carries the `message` (Notification)
//! and `stop_hook_active` (Stop) fields plus `project_name()` that beacon uses.

pub use harness_core::hook::HookInput;
