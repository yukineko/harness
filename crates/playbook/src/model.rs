//! serde struct for the Claude Code **UserPromptSubmit** hook stdin payload.
//!
//! playbook only consumes a subset (`session_id`, `cwd`, `hook_event_name`,
//! `prompt`), all of which exist on the shared `harness_core::hook::HookInput`
//! with identical `#[serde(default)]` semantics and the same `parse` /
//! `cwd_or_current` helpers — so we re-export it rather than duplicate it.
pub use harness_core::hook::HookInput;
