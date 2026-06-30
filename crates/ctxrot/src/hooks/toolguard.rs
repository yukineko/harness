//! `ctxrot toolguard` — PostToolUse hook.
//!
//! After a tool runs, measure how much text it dumped into context. A single
//! huge Read/Bash/Grep result is a classic rot source. When it exceeds the
//! threshold we:
//!   1. Truncate the response (head + tail, with an omission header) so the
//!      live context load is bounded (`updated`).
//!   2. Inject a short nudge to route the next heavy read through a sub-agent
//!      (`nudge`).
//!
//! We do NOT block the tool — the output is already in context this turn; the
//! goal is to steer the *next* heavy read.

use serde_json::Value;

use crate::config::Config;
use harness_core::hook::HookInput;

/// Tools whose output is the usual rot vector. Others (Edit, Write, TodoWrite…)
/// are skipped even if their echoed response is large.
const WATCHED: &[&str] = &[
    "Read",
    "Bash",
    "Grep",
    "Glob",
    "WebFetch",
    "BashOutput",
    "NotebookRead",
];

/// Output from the toolguard hook.
///
/// - `updated`: truncated tool response text (replaces the raw output in context).
/// - `nudge`:   advisory text injected as `additionalContext` to steer the model.
pub struct ToolguardOutput {
    pub updated: Option<String>,
    pub nudge: Option<String>,
}

pub fn run(input: &HookInput, cfg: &Config) -> ToolguardOutput {
    if input.tool_name.is_empty() || !WATCHED.contains(&input.tool_name.as_str()) {
        return ToolguardOutput {
            updated: None,
            nudge: None,
        };
    }
    let resp = match input.tool_response.as_ref() {
        Some(r) => r,
        None => {
            return ToolguardOutput {
                updated: None,
                nudge: None,
            }
        }
    };
    let size = response_text_len(resp);
    if (size as u64) < cfg.huge_tool_output_bytes {
        return ToolguardOutput {
            updated: None,
            nudge: None,
        };
    }

    // Read session's existing tooldump count BEFORE emitting, so we count how
    // many nudges have already been sent (not including this turn).
    let existing_dumps = crate::metrics::summarize(cfg)
        .into_iter()
        .find(|s| s.session == input.session_id)
        .map(|s| s.tooldumps)
        .unwrap_or(0);

    crate::metrics::emit(
        cfg,
        &input.session_id,
        "tooldump",
        serde_json::json!({ "tool": input.tool_name, "bytes": size }),
    );

    // Extract text from response for truncation.
    let text = response_text(resp);
    let limit = cfg.huge_tool_output_bytes as usize;
    let updated = truncate_response(&text, limit);

    // Suppress the advisory nudge once the cap is reached; truncation continues.
    let nudge = if existing_dumps < u64::from(cfg.toolguard_nudge_cap) {
        let kb = size as f64 / 1024.0;
        let tok = size / 4;
        Some(format!(
            "[context-rot guard] {} が大きい出力 (~{:.0}KB, 推定~{}tok) を context に投入。\
             → 次回以降この種の重い読み込み/検索は Explore か general-purpose sub-agent に委譲し、\
             必要な該当行・結論だけ受け取ること。今回ぶんは用が済んだら蒸留/退避を検討。",
            input.tool_name, kb, tok
        ))
    } else {
        None
    };

    ToolguardOutput {
        updated: Some(updated),
        nudge,
    }
}

/// Truncate `text` to at most `limit` characters using head/tail strategy.
///
/// If the text fits within `limit`, it is returned unchanged (no header).
/// Otherwise the text is split into lines; the first and last halves (by line
/// count) are kept so that the total character count stays below `limit`, and
/// an omission header is prepended:
///
/// ```text
/// [toolguard: {orig_kb:.0}KB → {limit_kb:.0}KB, 中間{skipped}行省略]
/// ```
pub fn truncate_response(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();
    let orig_kb = text.chars().count() as f64 / 1024.0;
    let limit_kb = limit as f64 / 1024.0;

    // Reserve space for the header line itself (approximate).
    let header_placeholder =
        format!("[toolguard: {orig_kb:.0}KB → {limit_kb:.0}KB, 中間{total_lines}行省略]");
    let usable = limit.saturating_sub(header_placeholder.chars().count() + 1);
    let half = usable / 2;

    // Greedily accumulate head lines.
    let mut head_lines: Vec<&str> = Vec::new();
    let mut head_chars = 0usize;
    for line in &lines {
        let lc = line.chars().count() + 1; // +1 for newline
        if head_chars + lc > half {
            break;
        }
        head_lines.push(line);
        head_chars += lc;
    }

    // Greedily accumulate tail lines (from the end).
    let mut tail_lines: Vec<&str> = Vec::new();
    let mut tail_chars = 0usize;
    let tail_budget = usable.saturating_sub(head_chars);
    for line in lines.iter().rev() {
        let lc = line.chars().count() + 1;
        if tail_chars + lc > tail_budget {
            break;
        }
        tail_lines.push(line);
        tail_chars += lc;
    }
    tail_lines.reverse();

    // Determine which lines were skipped.
    let head_count = head_lines.len();
    let tail_count = tail_lines.len();
    // If head and tail overlap (very short text that somehow got here), just return text.
    if head_count + tail_count >= total_lines {
        return text.to_string();
    }
    let skipped = total_lines - head_count - tail_count;

    let header = format!("[toolguard: {orig_kb:.0}KB → {limit_kb:.0}KB, 中間{skipped}行省略]");

    let mut result = String::with_capacity(limit + 64);
    result.push_str(&header);
    result.push('\n');
    for line in &head_lines {
        result.push_str(line);
        result.push('\n');
    }
    for line in &tail_lines {
        result.push_str(line);
        result.push('\n');
    }

    result
}

/// Extract the primary text content from a tool response value.
fn response_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(response_text)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            let mut parts = Vec::new();
            for key in ["stdout", "stderr", "text", "output", "result"] {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    if !s.is_empty() {
                        parts.push(s.to_string());
                    }
                }
            }
            if let Some(c) = map.get("content") {
                let t = response_text(c);
                if !t.is_empty() {
                    parts.push(t);
                }
            }
            if parts.is_empty() {
                v.to_string()
            } else {
                parts.join("\n")
            }
        }
        _ => String::new(),
    }
}

/// Best-effort character length of a tool_response, whether it's a bare string,
/// an object with text-ish fields, or an array of content blocks.
fn response_text_len(v: &Value) -> usize {
    match v {
        Value::String(s) => s.chars().count(),
        Value::Array(items) => items.iter().map(response_text_len).sum(),
        Value::Object(map) => {
            // common shapes: {stdout, stderr}, {content:[{type,text}]}, {text}
            let mut total = 0;
            for key in ["stdout", "stderr", "text", "output", "result"] {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    total += s.chars().count();
                }
            }
            if let Some(c) = map.get("content") {
                total += response_text_len(c);
            }
            if total == 0 {
                // fall back to serialized size as a proxy
                total = v.to_string().chars().count();
            }
            total
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(tool: &str, resp: Value) -> HookInput {
        HookInput {
            tool_name: tool.to_string(),
            tool_response: Some(resp),
            ..Default::default()
        }
    }

    /// Config suitable for unit tests: metrics writes disabled and state_dir
    /// pointed at a nonexistent path so summarize() always returns an empty
    /// list, keeping tooldump counts at zero regardless of the developer's
    /// local ctxrot state.
    fn test_cfg() -> Config {
        let mut cfg = Config::default();
        cfg.metrics = false;
        cfg.state_dir = std::path::PathBuf::from("/nonexistent-ctxrot-test-state");
        cfg
    }

    #[test]
    fn warns_on_big_read() {
        let cfg = test_cfg();
        let big = "x".repeat(60_000);
        let out = run(&input("Read", json!({ "content": big })), &cfg);
        assert!(out.nudge.unwrap().contains("Read"));
        assert!(out.updated.is_some());
    }

    #[test]
    fn silent_on_small_and_unwatched() {
        let cfg = test_cfg();
        let small_out = run(&input("Read", json!({ "content": "small" })), &cfg);
        assert!(small_out.updated.is_none());
        assert!(small_out.nudge.is_none());

        let big = "x".repeat(60_000);
        let edit_out = run(&input("Edit", json!({ "content": big })), &cfg);
        assert!(edit_out.updated.is_none());
        assert!(edit_out.nudge.is_none());
    }

    #[test]
    fn measures_stdout() {
        let cfg = test_cfg();
        let big = "y".repeat(60_000);
        let out = run(&input("Bash", json!({ "stdout": big, "stderr": "" })), &cfg);
        assert!(out.updated.is_some());
        assert!(out.nudge.is_some());
    }

    #[test]
    fn truncate_short_text_unchanged() {
        let text = "hello\nworld\n";
        let result = truncate_response(text, 1000);
        assert_eq!(result, text);
    }

    #[test]
    fn truncate_long_text_has_header() {
        // Build a text that is well over the limit.
        let lines: Vec<String> = (0..1000).map(|i| format!("line {:04}", i)).collect();
        let text = lines.join("\n");
        let limit = 200;
        let result = truncate_response(&text, limit);
        assert!(
            result.contains("[toolguard:"),
            "should have omission header"
        );
        assert!(
            result.contains("行省略"),
            "header should mention skipped lines"
        );
        assert!(
            result.chars().count() <= limit + 128,
            "result should be close to limit ({}), got {}",
            limit,
            result.chars().count()
        );
    }

    #[test]
    fn truncate_contains_head_and_tail() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {:03}", i)).collect();
        let text = lines.join("\n");
        // Use a modest limit so truncation kicks in.
        let limit = 100;
        let result = truncate_response(&text, limit);
        // The first line should appear.
        assert!(result.contains("line 000"), "should contain head");
        // The last line should appear.
        assert!(result.contains("line 199"), "should contain tail");
        // Middle lines should be omitted.
        assert!(
            !result.contains("line 100"),
            "middle lines should be omitted"
        );
    }

    #[test]
    fn truncate_result_within_limit_plus_header() {
        let lines: Vec<String> = (0..500)
            .map(|i| "a".repeat(20) + &format!(" {}", i))
            .collect();
        let text = lines.join("\n");
        let limit = 512;
        let result = truncate_response(&text, limit);
        // Allow a small slack for the header line itself (which can be ~50 chars).
        let result_len = result.chars().count();
        assert!(
            result_len <= limit + 128,
            "truncated result ({}) should not far exceed limit ({})",
            result_len,
            limit
        );
    }

    #[test]
    fn nudge_cap_default_is_3() {
        assert_eq!(Config::default().toolguard_nudge_cap, 3);
    }

    #[test]
    fn nudge_cap_zero_suppresses_nudge_but_keeps_truncation() {
        let mut cfg = test_cfg();
        cfg.toolguard_nudge_cap = 0; // cap=0 means never nudge
        let big = "x".repeat(60_000);
        let out = run(&input("Read", json!({ "content": big })), &cfg);
        assert!(out.updated.is_some(), "truncation should still occur");
        assert!(
            out.nudge.is_none(),
            "nudge must be suppressed when cap is 0"
        );
    }
}
