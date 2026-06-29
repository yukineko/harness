//! Claude Code hook I/O: the stdin payload struct and the never-break-a-turn
//! execution wrapper.
//!
//! Invariant (shared by every plugin): a hook must NEVER break the user's turn.
//! On any error or panic we exit 0 and stay silent. `run_hook` enforces that.

use std::io::Read;
use std::panic::UnwindSafe;

use serde::Deserialize;
use serde_json::Value;

// Some fields (hook_event_name, tool_input) are part of the payload schema but
// not consumed by every plugin; kept for completeness and future hooks.
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

    /// Notification: text supplied by Claude Code on a `Notification` event
    /// (e.g. "Claude needs your permission to use Bash").
    #[serde(default)]
    pub message: String,

    /// Stop / SubagentStop: true when this stop is itself the result of a
    /// previous stop-hook continuation.
    #[serde(default)]
    pub stop_hook_active: bool,
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

    /// Short, human-facing project label — the basename of `cwd`.
    pub fn project_name(&self) -> String {
        self.cwd_or_current()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project".to_string())
    }

    /// A stable session key for per-session state. Empty session ids (manual
    /// runs) collapse to a shared "_local" bucket.
    pub fn session_key(&self) -> String {
        if self.session_id.is_empty() {
            "_local".to_string()
        } else {
            self.session_id.clone()
        }
    }

    /// A short detail for the touched-file / command set, if extractable from
    /// the tool input (the path a file-oriented tool acted on).
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

/// Upper bound on how much stdin a hook will read (10 MiB). Hook payloads are
/// small JSON; anything larger is treated as hostile or garbage. Bounding the
/// read stops an oversized or endless stdin from exhausting memory (DoS guard).
pub const MAX_STDIN_BYTES: u64 = 10 * 1024 * 1024;

/// Read stdin into a String, capped at [`MAX_STDIN_BYTES`]. Errors are swallowed
/// (returns what was read, possibly empty) so a hook never aborts on a read
/// hiccup; input past the cap is truncated rather than read unbounded. Reads as
/// bytes and lossily decodes so a multi-byte char split at the cap can't error.
pub fn read_stdin() -> String {
    let mut buf = Vec::new();
    let _ = std::io::stdin().take(MAX_STDIN_BYTES).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Run `f`, swallowing any panic. Returns `true` if `f` completed without
/// panicking, `false` if it unwound. The testable core of `run_hook`.
pub fn catch_silent<F: FnOnce() + UnwindSafe>(f: F) -> bool {
    std::panic::catch_unwind(f).is_ok()
}

/// Run a hook handler with all panics swallowed; always exits 0. The handler does
/// its own stdout/stderr writing — this only guarantees the turn is never broken.
pub fn run_hook<F: FnOnce() + UnwindSafe>(f: F) -> ! {
    let _ = catch_silent(f);
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_absorbs_any_event_and_rejects_empty() {
        assert!(HookInput::parse("").is_none());
        assert!(HookInput::parse("   \n").is_none());
        assert!(HookInput::parse("not json").is_none());
        // A PostToolUse-shaped payload with fields absent from other events.
        let h = HookInput::parse(r#"{"session_id":"S","tool_name":"Read"}"#).unwrap();
        assert_eq!(h.session_id, "S");
        assert_eq!(h.tool_name, "Read");
        assert_eq!(h.prompt, ""); // missing field → default, never a parse failure
    }

    #[test]
    fn catch_silent_contains_panic_and_reports_outcome() {
        // Suppress the default panic message so the test output stays clean.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = catch_silent(|| panic!("boom"));
        let ok = catch_silent(|| { /* no-op */ });
        std::panic::set_hook(prev);
        assert!(
            !panicked,
            "a panicking handler must be caught, not propagated"
        );
        assert!(ok, "a clean handler reports success");
    }
}
