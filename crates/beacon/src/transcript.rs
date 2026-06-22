//! Best-effort extraction of Claude's last assistant text from the JSONL
//! transcript, used to give the Stop notification a useful one-line body.
//!
//! Everything here is fail-soft: any parse/read error just yields `None`, and
//! the caller falls back to a generic message. A transcript format change must
//! never break the hook.

use serde_json::Value;

/// Return the last assistant text message, flattened to one line and truncated
/// to `max_chars` (by characters, not bytes — safe for Japanese).
pub fn last_assistant_text(path: &str, max_chars: usize) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let content = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array);
        let Some(parts) = content else { continue };

        let joined: String = parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ");

        let flat = flatten(&joined);
        if flat.is_empty() {
            // assistant turn with only tool calls — keep scanning earlier turns
            continue;
        }
        return Some(truncate(&flat, max_chars));
    }
    None
}

fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_counts_chars_not_bytes() {
        assert_eq!(truncate("あいうえお", 3), "あいう…");
        assert_eq!(truncate("ok", 10), "ok");
    }

    #[test]
    fn flatten_collapses_whitespace() {
        assert_eq!(flatten("a\n  b\tc"), "a b c");
    }
}
