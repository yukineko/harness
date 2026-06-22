//! serde structs for Claude Code hook stdin payloads.
//!
//! All fields are optional with sane defaults so a single struct can absorb the
//! payloads of every hook event (UserPromptSubmit / PreCompact / SessionStart /
//! PostToolUse) without failing to deserialize when a field is absent.

use serde::Deserialize;
use serde_json::Value;

// Some fields (hook_event_name, tool_input) are part of the payload schema but
// not consumed yet; kept for completeness and future hooks.
#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub hook_event_name: String,

    /// UserPromptSubmit
    #[serde(default)]
    pub prompt: String,

    /// PreCompact: "manual" | "auto"
    #[serde(default)]
    pub trigger: String,

    /// SessionStart: "startup" | "resume" | "clear" | "compact"
    #[serde(default)]
    pub source: String,

    /// PostToolUse
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Option<Value>,
    #[serde(default)]
    pub tool_response: Option<Value>,
}

impl HookInput {
    /// Parse a hook payload from a raw stdin string. Returns None on empty/invalid
    /// input so callers can stay silent (never break the user's turn).
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        serde_json::from_str(raw).ok()
    }

    /// cwd, falling back to the process cwd if the hook did not supply one.
    pub fn cwd_or_current(&self) -> std::path::PathBuf {
        if self.cwd.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            std::path::PathBuf::from(&self.cwd)
        }
    }
}
