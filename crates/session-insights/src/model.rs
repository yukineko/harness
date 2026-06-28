//! The Claude Code hook stdin payload. Consolidated onto the canonical
//! [`harness_core::hook::HookInput`], which provides `parse`, `cwd_or_current`,
//! `session_key`, `project_name`, and `target` (touched-file extraction).

pub use harness_core::hook::HookInput;
