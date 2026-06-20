//! serde struct for the Claude Code **Stop** hook stdin payload. Every field is
//! optional so an empty/foreign payload still deserializes; a manual run with no
//! stdin JSON is a no-op.

use std::path::PathBuf;

use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub hook_event_name: String,
    /// Path to the JSONL transcript we aggregate token usage from.
    #[serde(default)]
    pub transcript_path: String,
    /// True when a Stop hook is already re-running after a previous block.
    #[serde(default)]
    pub stop_hook_active: bool,
}

impl HookInput {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        serde_json::from_str(raw).ok()
    }

    pub fn cwd_or_current(&self) -> PathBuf {
        if self.cwd.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(&self.cwd)
        }
    }

    /// Short, human-facing project label — the basename of `cwd`.
    pub fn project_name(&self) -> String {
        self.cwd_or_current()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project".to_string())
    }
}
