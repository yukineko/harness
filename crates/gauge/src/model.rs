//! The Claude Code hook stdin payload. Consolidated onto the canonical
//! [`harness_core::hook::HookInput`] (covers `stop_hook_active`, `transcript_path`,
//! `project_name()`, etc.) so every plugin shares one struct + parse contract.

pub use harness_core::hook::HookInput;
