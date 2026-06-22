//! serde struct for the Claude Code **Stop** (and SubagentStop) hook stdin
//! payload. Every field is optional so an empty/foreign payload still
//! deserializes — when `reviewgate review` is run by hand (no stdin JSON) we
//! fall back to a default and behave as a plain CLI reviewer.

use serde::Deserialize;

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

    /// Stop / SubagentStop: true when this stop is itself the result of a
    /// previous stop-hook continuation. We rely primarily on our own per-session
    /// diff-hash + attempt state, but keep this for completeness.
    #[serde(default)]
    pub stop_hook_active: bool,
}

impl HookInput {
    /// Parse a hook payload from raw stdin. `None` means there was no JSON on
    /// stdin (manual CLI run) — the caller then runs in human-report mode.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        serde_json::from_str(raw).ok()
    }

    /// Working directory the hook fired in, falling back to the process cwd.
    pub fn cwd_or_current(&self) -> std::path::PathBuf {
        if self.cwd.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            std::path::PathBuf::from(&self.cwd)
        }
    }

    /// A stable session key for the per-session state. Empty session ids (manual
    /// runs) collapse to a shared "_local" bucket.
    pub fn session_key(&self) -> String {
        if self.session_id.is_empty() {
            "_local".to_string()
        } else {
            self.session_id.clone()
        }
    }
}
