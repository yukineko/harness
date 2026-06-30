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

    // Observation always fires: record that a big payload landed, independent of
    // whether we go on to advise about it. The `tooldump` metric is the size
    // signal; the `nudge` decision below is the *act* on top of it.
    crate::metrics::emit(
        cfg,
        &input.session_id,
        "tooldump",
        serde_json::json!({ "tool": input.tool_name, "bytes": size }),
    );

    // Gate the *advice* on per-session seen-state + a nudge cap so the rot
    // detector does not itself become a rot source (observe→act, symmetric with
    // the context-governor injector/checkpointer seen-state dedup). Skip a
    // rot-source key already nudged this session, and stop once the session's
    // nudge budget (`toolguard_nudge_cap`, 0 = never advise) is spent. The
    // tooldump above is already recorded, so observation continues uncapped.
    let (seen, total) = crate::metrics::nudge_state(cfg, &input.session_id);
    if total >= cfg.toolguard_nudge_cap || seen.contains(&input.tool_name) {
        return None;
    }

    // Committing to a nudge: record it so the next invocation sees this key and
    // counts it toward the cap.
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

    /// (1) first occurrence of a rot-source key → nudge.
    #[test]
    fn first_occurrence_nudges() {
        let cfg = temp_cfg("first");
        let out = run(&big_for("Read"), &cfg);
        assert!(out.unwrap().contains("Read"));
        // The nudge was recorded as seen-state for the next invocation.
        let (seen, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert!(seen.contains("Read"));
        assert_eq!(total, 1);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (2) immediate repeat of the same key → suppressed (already nudged).
    #[test]
    fn repeat_same_key_suppressed() {
        let cfg = temp_cfg("repeat");
        assert!(run(&big_for("Read"), &cfg).is_some()); // first nudges
        assert!(run(&big_for("Read"), &cfg).is_none()); // repeat suppressed
                                                        // tooldump still observed on every oversized output (3 here: see below),
                                                        // but only one nudge was emitted.
        let (_, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 1);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (3) distinct keys keep nudging until the cap, then (4) every key is
    /// suppressed once the cap is reached.
    #[test]
    fn distinct_keys_nudge_until_cap_then_all_suppressed() {
        let mut cfg = temp_cfg("cap");
        cfg.toolguard_nudge_cap = 2;
        // Two distinct keys fill the cap.
        assert!(run(&big_for("Read"), &cfg).is_some());
        assert!(run(&big_for("Bash"), &cfg).is_some());
        // (4) cap reached → a fresh, never-seen key is still suppressed.
        assert!(run(&big_for("Grep"), &cfg).is_none());
        let (seen, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 2);
        assert!(seen.contains("Read") && seen.contains("Bash"));
        assert!(!seen.contains("Grep"));
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// A cap of 0 means "never advise" — the very first oversized output is
    /// suppressed (but still observed as a tooldump).
    #[test]
    fn cap_zero_never_nudges() {
        let mut cfg = temp_cfg("cap0");
        cfg.toolguard_nudge_cap = 0;
        assert!(run(&big_for("Read"), &cfg).is_none());
        let (_, total) = crate::metrics::nudge_state(&cfg, "S1");
        assert_eq!(total, 0);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (5) reader fail-soft: with no prior state, the first oversized output is
    /// treated as unseen and nudges (covered by `first_occurrence_nudges`); here
    /// a tooldump is always emitted even when the nudge is later suppressed, so
    /// observation never depends on the gate.
    #[test]
    fn tooldump_observed_even_when_nudge_suppressed() {
        let cfg = temp_cfg("observe");
        assert!(run(&big_for("Read"), &cfg).is_some());
        assert!(run(&big_for("Read"), &cfg).is_none()); // suppressed
                                                        // Both oversized outputs were observed as tooldumps.
        let dumps = crate::metrics::summarize(&cfg)
            .into_iter()
            .find(|s| s.session == "S1")
            .map(|s| s.tooldumps)
            .unwrap_or(0);
        assert_eq!(dumps, 2);
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// (6) sub-threshold and unwatched tools stay silent — and never write state.
    #[test]
    fn silent_on_small_and_unwatched() {
        let cfg = temp_cfg("silent");
        assert!(run(&input("Read", json!({ "content": "small" })), &cfg).is_none());
        assert!(run(&big_for("Edit"), &cfg).is_none()); // unwatched tool
                                                        // Nothing oversized+watched happened → no metrics file at all.
        assert!(!path_exists(&cfg));
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    /// Big output on stdout (not just `content`) is measured and nudged.
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
        assert!(out.is_some());
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    fn path_exists(cfg: &Config) -> bool {
        crate::metrics::path(cfg).exists()
    }
}
