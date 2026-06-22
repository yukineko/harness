//! serde struct for the Claude Code hook stdin payloads (PostToolUse + Stop).
//! Every field is optional so an empty/foreign payload still deserializes.

use serde::Deserialize;
use serde_json::Value;

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub hook_event_name: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Option<Value>,
}

impl HookInput {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        serde_json::from_str(raw).ok()
    }

    pub fn cwd_or_current(&self) -> std::path::PathBuf {
        if self.cwd.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            std::path::PathBuf::from(&self.cwd)
        }
    }

    pub fn session_key(&self) -> String {
        if self.session_id.is_empty() {
            "_local".to_string()
        } else {
            self.session_id.clone()
        }
    }

    pub fn project_name(&self) -> String {
        self.cwd_or_current()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project".to_string())
    }

    /// A short detail for the touched-file / command set, if extractable.
    pub fn target(&self) -> Option<String> {
        let ti = self.tool_input.as_ref()?;
        match self.tool_name.as_str() {
            "Edit" | "Write" | "MultiEdit" | "Read" | "NotebookEdit" => ti
                .get("file_path")
                .or_else(|| ti.get("notebook_path"))
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            _ => None,
        }
    }
}
