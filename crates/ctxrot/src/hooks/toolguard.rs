//! `ctxrot toolguard` — PostToolUse hook.
//!
//! After a tool runs, measure how much text it dumped into context. A single
//! huge Read/Bash/Grep result is a classic rot source. When it exceeds the
//! threshold we inject a short, ONE-LINE-ish nudge to route the next heavy read
//! through a sub-agent (Explore / general-purpose) and keep only conclusions.
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

pub fn run(input: &HookInput, cfg: &Config) -> Option<String> {
    if input.tool_name.is_empty() || !WATCHED.contains(&input.tool_name.as_str()) {
        return None;
    }
    let resp = input.tool_response.as_ref()?;
    let size = response_text_len(resp);
    if (size as u64) < cfg.huge_tool_output_bytes {
        return None;
    }

    crate::metrics::emit(
        cfg,
        &input.session_id,
        "tooldump",
        serde_json::json!({ "tool": input.tool_name, "bytes": size }),
    );

    let kb = size as f64 / 1024.0;
    let tok = size / 4;
    Some(format!(
        "[context-rot guard] {} が大きい出力 (~{:.0}KB, 推定~{}tok) を context に投入。\
         → 次回以降この種の重い読み込み/検索は Explore か general-purpose sub-agent に委譲し、\
         必要な該当行・結論だけ受け取ること。今回ぶんは用が済んだら蒸留/退避を検討。",
        input.tool_name, kb, tok
    ))
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

    #[test]
    fn warns_on_big_read() {
        let cfg = Config::default();
        let big = "x".repeat(60_000);
        let out = run(&input("Read", json!({ "content": big })), &cfg);
        assert!(out.unwrap().contains("Read"));
    }

    #[test]
    fn silent_on_small_and_unwatched() {
        let cfg = Config::default();
        assert!(run(&input("Read", json!({ "content": "small" })), &cfg).is_none());
        let big = "x".repeat(60_000);
        assert!(run(&input("Edit", json!({ "content": big })), &cfg).is_none());
    }

    #[test]
    fn measures_stdout() {
        let cfg = Config::default();
        let big = "y".repeat(60_000);
        let out = run(&input("Bash", json!({ "stdout": big, "stderr": "" })), &cfg);
        assert!(out.is_some());
    }
}
