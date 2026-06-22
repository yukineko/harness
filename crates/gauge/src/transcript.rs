//! Aggregate one session's JSONL transcript into per-model token usage, tool
//! call counts, and a timespan. Everything is fail-soft: any read/parse error
//! yields `None` (the hook then records nothing), and an unknown transcript
//! shape never breaks the turn.
//!
//! Each assistant line carries a `message.usage` block
//! (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`,
//! `cache_read_input_tokens`, and a `cache_creation` 5m/1h split) plus a
//! `message.model`; tool calls appear as `tool_use` parts in `message.content`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::store::Usage;

#[derive(Debug, Default)]
pub struct Aggregate {
    pub models: BTreeMap<String, Usage>,
    /// Number of assistant model requests (turns) with usage.
    pub turns: u64,
    pub tools: BTreeMap<String, u64>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
}

pub fn aggregate(path: &str) -> Option<Aggregate> {
    if path.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut agg = Aggregate::default();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if agg.first_ts.is_none() {
                agg.first_ts = Some(ts.to_string());
            }
            agg.last_ts = Some(ts.to_string());
        }

        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };

        if let Some(content) = msg.get("content").and_then(Value::as_array) {
            for p in content {
                if p.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(name) = p.get("name").and_then(Value::as_str) {
                        *agg.tools.entry(name.to_string()).or_default() += 1;
                    }
                }
            }
        }

        let Some(usage) = msg.get("usage") else {
            continue;
        };
        let model = msg
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let u = agg.models.entry(model).or_default();
        let get = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);

        u.input += get("input_tokens");
        u.output += get("output_tokens");
        u.cache_read += get("cache_read_input_tokens");

        // Split cache-creation tokens by TTL when the breakdown is present;
        // otherwise attribute the whole amount to the 5-minute tier.
        if let Some(cc) = usage.get("cache_creation") {
            let g = |k: &str| cc.get(k).and_then(Value::as_u64).unwrap_or(0);
            u.cache_write_5m += g("ephemeral_5m_input_tokens");
            u.cache_write_1h += g("ephemeral_1h_input_tokens");
        } else {
            u.cache_write_5m += get("cache_creation_input_tokens");
        }

        agg.turns += 1;
    }

    if agg.turns == 0 && agg.tools.is_empty() {
        return None;
    }
    Some(agg)
}
