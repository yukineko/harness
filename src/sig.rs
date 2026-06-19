//! Turn a tool call into a stable *signature* (so identical actions collide) and
//! pull out edit details (file + before/after hashes) so we can spot revert
//! thrash. Hashing uses `DefaultHasher` (fixed-seed, deterministic across
//! processes) — we only need collision-equality, not crypto.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One observed tool event, persisted in the per-session ring buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub tool: String,
    /// Normalized signature: identical actions share it.
    pub sig: String,
    /// For edit-like tools, the target file (for thrash detection).
    #[serde(default)]
    pub file: Option<String>,
    /// Hash of the removed text (Edit.old_string).
    #[serde(default)]
    pub old_h: Option<u64>,
    /// Hash of the inserted text (Edit.new_string / Write.content).
    #[serde(default)]
    pub new_h: Option<u64>,
    /// Best-effort: did the tool response look like a failure?
    #[serde(default)]
    pub error: bool,
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Collapse runs of whitespace to a single space and trim, so cosmetically
/// different invocations of the same command still collide.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Build an Event (minus `seq`) from a tool call. `None` for tools we can't
/// signature (no name).
pub fn build(tool: &str, input: Option<&Value>, response: Option<&Value>) -> Option<Event> {
    if tool.trim().is_empty() {
        return None;
    }
    let inp = input.cloned().unwrap_or(Value::Null);
    let mut file = None;
    let mut old_h = None;
    let mut new_h = None;

    let sig_body = match tool {
        "Bash" => norm(field(&inp, "command").unwrap_or("")),
        "Edit" | "MultiEdit" => {
            let f = field(&inp, "file_path").unwrap_or("").to_string();
            let old = field(&inp, "old_string").unwrap_or("");
            let new = field(&inp, "new_string").unwrap_or("");
            old_h = Some(hash(old));
            new_h = Some(hash(new));
            file = Some(f.clone());
            format!("{f}\u{1}{old}\u{1}{new}")
        }
        "Write" => {
            let f = field(&inp, "file_path").unwrap_or("").to_string();
            let content = field(&inp, "content").unwrap_or("");
            old_h = Some(hash("")); // write replaces wholesale
            new_h = Some(hash(content));
            file = Some(f.clone());
            format!("{f}\u{1}{content}")
        }
        "Read" => field(&inp, "file_path").unwrap_or("").to_string(),
        "Grep" => format!(
            "{}\u{1}{}",
            field(&inp, "pattern").unwrap_or(""),
            field(&inp, "path").unwrap_or("")
        ),
        "Glob" => field(&inp, "pattern").unwrap_or("").to_string(),
        // Fallback: hash the whole input blob.
        _ => serde_json::to_string(&inp).unwrap_or_default(),
    };

    Some(Event {
        seq: 0,
        tool: tool.to_string(),
        sig: format!("{tool}:{:016x}", hash(&sig_body)),
        file,
        old_h,
        new_h,
        error: response.map(looks_error).unwrap_or(false),
    })
}

/// Best-effort failure detection from a tool response.
fn looks_error(resp: &Value) -> bool {
    match resp {
        Value::Object(m) => {
            if m.get("is_error").and_then(Value::as_bool) == Some(true) {
                return true;
            }
            if m.get("error").map(|e| !e.is_null()).unwrap_or(false) {
                return true;
            }
            if let Some(code) = m.get("exit_code").and_then(Value::as_i64) {
                if code != 0 {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bash_normalizes_whitespace() {
        let a = build("Bash", Some(&json!({"command": "cargo  test"})), None).unwrap();
        let b = build("Bash", Some(&json!({"command": "cargo test"})), None).unwrap();
        assert_eq!(a.sig, b.sig);
    }

    #[test]
    fn edit_captures_hashes_and_file() {
        let e = build(
            "Edit",
            Some(&json!({"file_path": "a.rs", "old_string": "A", "new_string": "B"})),
            None,
        )
        .unwrap();
        assert_eq!(e.file.as_deref(), Some("a.rs"));
        assert_eq!(e.old_h, Some(hash("A")));
        assert_eq!(e.new_h, Some(hash("B")));
    }

    #[test]
    fn error_detected_from_response() {
        let e = build(
            "Bash",
            Some(&json!({"command": "x"})),
            Some(&json!({"exit_code": 1})),
        )
        .unwrap();
        assert!(e.error);
    }
}
