//! `ctxrot toolguard` — PostToolUse hook.
//!
//! After a tool runs, measure how much text it dumped into context. A single
//! huge Read/Bash/Grep result is a classic rot source. When it exceeds the
//! threshold we:
//!   1. Truncate the response (head + tail, with an omission header) so the
//!      live context load is bounded (`updated`).
//!   2. Inject a short nudge to route the next heavy read through a sub-agent
//!      (`nudge`). The nudge is gated by per-key seen-state (each tool is
//!      nudged at most once per session) and a session-level nudge cap.
//!
//! Observation (tooldump metric) always fires. The nudge is the *act* on top
//! of the observation, and is suppressed once seen-state or cap is exhausted.
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

    // Observation always fires: record that a big payload landed, independent of
    // whether we go on to advise about it. The `tooldump` metric is the size
    // signal; the `nudge` decision below is the *act* on top of it.
    crate::metrics::emit(
        cfg,
        &input.session_id,
        "tooldump",
        serde_json::json!({ "tool": input.tool_name, "bytes": size }),
    );

    // Always truncate, regardless of nudge gating.
    let text = response_text(resp);
    let limit = cfg.huge_tool_output_bytes as usize;
    let updated = truncate_response(&text, limit);

    // Gate the *advice* on per-session seen-state + a nudge cap so the rot
    // detector does not itself become a rot source. Skip a rot-source key
    // already nudged this session, and stop once the session's nudge budget
    // (`toolguard_nudge_cap`, 0 = never advise) is spent.
    let (seen, total) = crate::metrics::nudge_state(cfg, &input.session_id);
    let nudge = if total >= cfg.toolguard_nudge_cap
        || seen.contains(&input.tool_name)
    {
        None
    } else {
        // Committing to a nudge: record it so the next invocation sees this key.
        crate::metrics::emit(
            cfg,
            &input.session_id,
            "nudge",
            serde_json::json!({ "tool": input.tool_name }),
        );
        let kb = size as f64 / 1024.0;
        let tok = size / 4;
        Some(format!(
            "[context-rot guard] {} が大きい出力 (~{:.0}KB, 推定~{}tok) を context に投入。\
             → 次回以降この種の重い読み込み/検索は Explore か general-purpose sub-agent に委譲し、\
             必要な該当行・結論だけ受け取ること。今回ぶんは用が済んだら蒸留/退避を検討。",
            input.tool_name, kb, tok
        ))
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

    /// Config with an isolated, real-on-disk state dir so the seen-state reader
    /// and the `nudge`/`tooldump` writes round-trip deterministically without
    /// touching the user's `~/.ctxrot` state. Metrics default on.
    fn temp_cfg(name: &str) -> Config {
        let dir = tempfile::Builder::new()
            .prefix(&format!("ctxrot-toolguard-{name}-"))
            .tempdir()
            .expect("tempdir")
            .keep();
        Config {
            state_dir: dir,
            ..Config::default()
        }
    }

    fn input(tool: &str, resp: Value) -> HookInput {
        HookInput {
            tool_name: tool.to_string(),
            tool_response: Some(resp),
            session_id: "S1".to_string(),
            ..Default::default()
        }
    }

    fn big_for(tool: &str) -> HookInput {
        input(tool, json!({ "content": "x".repeat(60_000) }))
    }

    /// (1) first occurrence of a rot-source key → nudge + truncation.
    #[test]
    fn first_occurrence_nudges() {
        let cfg = temp_cfg("first");
        let out = run(&big_for("Read"), &cfg);
        assert!(out.nudge.as_ref().unwrap().contains("Read"));
        assert!(out.updated.is_some());
        let (seen, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert!(seen.contains("Read"));
        assert_eq!(total, 1);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (2) immediate repeat of the same key → nudge suppressed, truncation continues.
    #[test]
    fn repeat_same_key_suppresses_nudge_keeps_truncation() {
        let cfg = temp_cfg("repeat");
        assert!(run(&big_for("Read"), &cfg).nudge.is_some());
        let second = run(&big_for("Read"), &cfg);
        assert!(second.nudge.is_none(), "nudge suppressed on repeat");
        assert!(second.updated.is_some(), "truncation continues");
        let (_, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 1);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (3) distinct keys keep nudging until the cap, then (4) every key is
    /// suppressed once the cap is reached — but truncation always continues.
    #[test]
    fn distinct_keys_nudge_until_cap_then_all_suppressed() {
        let mut cfg = temp_cfg("cap");
        cfg.toolguard_nudge_cap = 2;
        assert!(run(&big_for("Read"), &cfg).nudge.is_some());
        assert!(run(&big_for("Bash"), &cfg).nudge.is_some());
        let third = run(&big_for("Grep"), &cfg);
        assert!(third.nudge.is_none(), "cap reached — nudge suppressed");
        assert!(third.updated.is_some(), "truncation still occurs");
        let (seen, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 2);
        assert!(seen.contains("Read") && seen.contains("Bash"));
        assert!(!seen.contains("Grep"));
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// A cap of 0 means "never advise" — truncation still occurs.
    #[test]
    fn cap_zero_never_nudges_but_keeps_truncation() {
        let mut cfg = temp_cfg("cap0");
        cfg.toolguard_nudge_cap = 0;
        let out = run(&big_for("Read"), &cfg);
        assert!(out.nudge.is_none(), "nudge suppressed when cap is 0");
        assert!(out.updated.is_some(), "truncation still occurs");
        let (_, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 0);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// tooldump is always observed even when nudge is suppressed.
    #[test]
    fn tooldump_observed_even_when_nudge_suppressed() {
        let cfg = temp_cfg("observe");
        assert!(run(&big_for("Read"), &cfg).nudge.is_some());
        assert!(run(&big_for("Read"), &cfg).nudge.is_none()); // suppressed
        let dumps = crate::metrics::summarize(&cfg)
            .into_iter()
            .find(|s| s.session == "S1")
            .map(|s| s.tooldumps)
            .unwrap_or(0);
        assert_eq!(dumps, 2);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// Sub-threshold and unwatched tools stay silent.
    #[test]
    fn silent_on_small_and_unwatched() {
        let cfg = temp_cfg("silent");
        let small = run(&input("Read", json!({ "content": "small" })), &cfg);
        assert!(small.updated.is_none());
        assert!(small.nudge.is_none());
        let big_edit = run(&big_for("Edit"), &cfg);
        assert!(big_edit.updated.is_none());
        assert!(big_edit.nudge.is_none());
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// Big output on stdout is measured and nudged.
    #[test]
    fn measures_stdout() {
        let cfg = temp_cfg("stdout");
        let out = run(
            &input(
                "Bash",
                json!({ "stdout": "y".repeat(60_000), "stderr": "" }),
            ),
            &cfg,
        );
        assert!(out.updated.is_some());
        assert!(out.nudge.is_some());
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    #[test]
    fn truncate_short_text_unchanged() {
        let text = "hello\nworld\n";
        let result = truncate_response(text, 1000);
        assert_eq!(result, text);
    }

    #[test]
    fn truncate_long_text_has_header() {
        let lines: Vec<String> = (0..1000).map(|i| format!("line {:04}", i)).collect();
        let text = lines.join("\n");
        let limit = 200;
        let result = truncate_response(&text, limit);
        assert!(result.contains("[toolguard:"), "should have omission header");
        assert!(result.contains("行省略"), "header should mention skipped lines");
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
        let limit = 100;
        let result = truncate_response(&text, limit);
        assert!(result.contains("line 000"), "should contain head");
        assert!(result.contains("line 199"), "should contain tail");
        assert!(!result.contains("line 100"), "middle lines should be omitted");
    }

    #[test]
    fn truncate_result_within_limit_plus_header() {
        let lines: Vec<String> = (0..500)
            .map(|i| "a".repeat(20) + &format!(" {}", i))
            .collect();
        let text = lines.join("\n");
        let limit = 512;
        let result = truncate_response(&text, limit);
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
}
