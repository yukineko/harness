//! Per-model token/usage aggregation from a session's JSONL transcript.
//!
//! Aggregate one session's transcript into per-model token usage, tool call
//! counts, and a timespan. Everything is fail-soft: any read/parse error yields
//! `None` (the caller then records nothing), and an unknown transcript shape
//! never breaks the turn.
//!
//! Each assistant line carries a `message.usage` block (`input_tokens`,
//! `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`,
//! and a `cache_creation` 5m/1h split) plus a `message.model`; tool calls appear
//! as `tool_use` parts in `message.content`.
//!
//! NOTE: this module's [`ModelUsage`] is the per-model *cost-accounting* tally
//! (uncached input/output plus the cache write/read split). It is distinct from
//! [`crate::transcript::Usage`], which is the live-window occupancy snapshot used
//! for context estimation. The two are intentionally separate types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Agent bucket key for the main (top-level) conversation.
pub const AGENT_MAIN: &str = "main";
/// Agent bucket key for sub-agent (Task / sidechain) activity, aggregated.
pub const AGENT_SUB: &str = "sub-agent";

/// Token counts for one model within a session. `input`/`output` are the
/// uncached counts; cache writes/reads are tracked separately for pricing.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    #[serde(default)]
    pub cache_read: u64,
}

impl ModelUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input + self.output + self.cache_write_5m + self.cache_write_1h + self.cache_read
    }

    pub fn add(&mut self, other: &ModelUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
    }
}

/// Per-agent token tally: which models a single agent bucket (main or
/// sub-agent) used and how many turns it took. Lets reports attribute cost to
/// the main thread vs sub-agent fan-out instead of collapsing them.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentUsage {
    #[serde(default)]
    pub models: BTreeMap<String, ModelUsage>,
    #[serde(default)]
    pub turns: u64,
}

impl AgentUsage {
    pub fn total_tokens(&self) -> u64 {
        self.models.values().map(|u| u.total_tokens()).sum()
    }
}

/// The whole-transcript aggregation: per-model token tallies, the number of
/// assistant turns, per-tool call counts, the first/last timestamp, and a
/// per-agent (main vs sub-agent) breakdown.
///
/// `models`/`turns` are the session grand total (main + sub-agent); `agents`
/// splits the same totals by [`AGENT_MAIN`] / [`AGENT_SUB`] so reports can show
/// where the spend went.
#[derive(Debug, Default)]
pub struct Aggregate {
    pub models: BTreeMap<String, ModelUsage>,
    /// Number of assistant model requests (turns) with usage.
    pub turns: u64,
    pub tools: BTreeMap<String, u64>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// Per-agent breakdown keyed by [`AGENT_MAIN`] / [`AGENT_SUB`].
    pub agents: BTreeMap<String, AgentUsage>,
}

/// Aggregate one session's transcript at `path` into per-model usage.
///
/// Returns `None` for an empty path, an unreadable file, or a transcript with
/// no assistant turns and no tool calls. Reads the whole transcript (cost
/// accounting must see every line); parse errors on individual lines are
/// skipped rather than aborting.
///
/// Sub-agent (Task) spend is attributed to the [`AGENT_SUB`] bucket and folded
/// into the grand total. Two layouts are handled: sidechain lines inline in the
/// main transcript (`isSidechain: true`), and the newer layout where each
/// sub-agent has its own file under `<transcript-stem>/subagents/*.jsonl`. The
/// sibling files are only read when the main transcript had no inline sidechain
/// lines, so the two layouts never double-count.
pub fn aggregate(path: &str) -> Option<Aggregate> {
    if path.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut agg = Aggregate::default();

    // Main transcript: bucket per-line by isSidechain, track timestamps + tools.
    let saw_inline_sub = ingest(&mut agg, &text, None, true, true);

    // Newer layout: sub-agent transcripts live in sibling files. Only fold them
    // in when the main transcript carried no inline sidechain turns.
    if !saw_inline_sub {
        for file in subagent_files(path) {
            if let Ok(sub_text) = std::fs::read_to_string(&file) {
                ingest(&mut agg, &sub_text, Some(AGENT_SUB), false, false);
            }
        }
    }

    if agg.turns == 0 && agg.tools.is_empty() {
        return None;
    }
    Some(agg)
}

/// Fold one transcript's lines into `agg`.
///
/// - `force_bucket`: if set, every turn is attributed to that agent bucket;
///   otherwise the bucket is derived from each line's `isSidechain` flag.
/// - `track_ts`: update `first_ts`/`last_ts` (only the main transcript should).
/// - `count_tools`: tally `tool_use` calls into `agg.tools` (main thread only).
///
/// Returns whether any sub-agent (sidechain) turn was seen.
fn ingest(
    agg: &mut Aggregate,
    text: &str,
    force_bucket: Option<&str>,
    track_ts: bool,
    count_tools: bool,
) -> bool {
    let mut saw_sub = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if track_ts {
            if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
                if agg.first_ts.is_none() {
                    agg.first_ts = Some(ts.to_string());
                }
                agg.last_ts = Some(ts.to_string());
            }
        }

        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };

        let is_sub = force_bucket == Some(AGENT_SUB)
            || v.get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        if is_sub {
            saw_sub = true;
        }
        let bucket = match force_bucket {
            Some(b) => b,
            None if is_sub => AGENT_SUB,
            None => AGENT_MAIN,
        };

        if count_tools {
            if let Some(content) = msg.get("content").and_then(Value::as_array) {
                for p in content {
                    if p.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let Some(name) = p.get("name").and_then(Value::as_str) {
                            *agg.tools.entry(name.to_string()).or_default() += 1;
                        }
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

        let get = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let mut delta = ModelUsage {
            input: get("input_tokens"),
            output: get("output_tokens"),
            cache_read: get("cache_read_input_tokens"),
            ..Default::default()
        };
        // Split cache-creation tokens by TTL when the breakdown is present;
        // otherwise attribute the whole amount to the 5-minute tier.
        if let Some(cc) = usage.get("cache_creation") {
            let g = |k: &str| cc.get(k).and_then(Value::as_u64).unwrap_or(0);
            delta.cache_write_5m = g("ephemeral_5m_input_tokens");
            delta.cache_write_1h = g("ephemeral_1h_input_tokens");
        } else {
            delta.cache_write_5m = get("cache_creation_input_tokens");
        }

        // Grand total (every caller's existing source of truth) …
        agg.models.entry(model.clone()).or_default().add(&delta);
        agg.turns += 1;
        // … and the per-agent breakdown.
        let agent = agg.agents.entry(bucket.to_string()).or_default();
        agent.models.entry(model).or_default().add(&delta);
        agent.turns += 1;
    }
    saw_sub
}

/// Locate sub-agent transcript files for a main transcript at `main_path`:
/// `<dir>/<stem>/subagents/*.jsonl`. Returns empty if the directory is absent.
fn subagent_files(main_path: &str) -> Vec<PathBuf> {
    let p = Path::new(main_path);
    let (Some(parent), Some(stem)) = (p.parent(), p.file_stem()) else {
        return Vec::new();
    };
    let dir = parent.join(stem).join("subagents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, body: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "harness-core-usage-{}-{name}.jsonl",
            std::process::id()
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn usage_totals_and_add() {
        let mut u = ModelUsage {
            input: 10,
            output: 20,
            cache_write_5m: 1,
            cache_write_1h: 2,
            cache_read: 3,
        };
        assert_eq!(u.total_tokens(), 36);
        let other = ModelUsage {
            input: 5,
            output: 5,
            ..Default::default()
        };
        u.add(&other);
        assert_eq!(u.input, 15);
        assert_eq!(u.output, 25);
        assert_eq!(u.total_tokens(), 46);
    }

    #[test]
    fn aggregates_multi_model_with_cache_split_and_tools() {
        // Two assistant turns from two different models, plus a tool_use.
        // One usage block carries the 5m/1h cache_creation split; the other
        // uses the flat cache_creation_input_tokens (→ 5m tier).
        let body = concat!(
            r#"{"type":"user","timestamp":"2026-06-22T10:00:00Z","message":{"role":"user","content":"hi"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:01Z","message":{"model":"claude-opus-4-8","content":[{"type":"text","text":"x"},{"type":"tool_use","name":"Bash"}],"usage":{"input_tokens":100,"output_tokens":200,"cache_read_input_tokens":50,"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":20}}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:05Z","message":{"model":"claude-sonnet-4-5","content":[{"type":"text","text":"y"}],"usage":{"input_tokens":1,"output_tokens":2,"cache_creation_input_tokens":7}}}"#,
            "\n",
        );
        let path = write_temp("multi", body);
        let agg = aggregate(path.to_str().unwrap()).expect("aggregate");

        assert_eq!(agg.turns, 2);
        assert_eq!(agg.tools.get("Bash"), Some(&1));
        assert_eq!(agg.first_ts.as_deref(), Some("2026-06-22T10:00:00Z"));
        assert_eq!(agg.last_ts.as_deref(), Some("2026-06-22T10:00:05Z"));

        let opus = agg.models.get("claude-opus-4-8").unwrap();
        assert_eq!(opus.input, 100);
        assert_eq!(opus.output, 200);
        assert_eq!(opus.cache_read, 50);
        assert_eq!(opus.cache_write_5m, 10);
        assert_eq!(opus.cache_write_1h, 20);

        let sonnet = agg.models.get("claude-sonnet-4-5").unwrap();
        assert_eq!(sonnet.input, 1);
        assert_eq!(sonnet.output, 2);
        // Flat cache_creation_input_tokens lands in the 5m tier.
        assert_eq!(sonnet.cache_write_5m, 7);
        assert_eq!(sonnet.cache_write_1h, 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_transcript_yields_none() {
        let path = write_temp("empty", "{\"type\":\"user\",\"message\":{}}\n");
        assert!(aggregate(path.to_str().unwrap()).is_none());
        let _ = std::fs::remove_file(&path);
        assert!(aggregate("").is_none());
        assert!(aggregate("/no/such/transcript.jsonl").is_none());
    }

    #[test]
    fn pure_main_transcript_is_all_main_bucket() {
        let body = concat!(
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:01Z","isSidechain":false,"message":{"model":"claude-opus-4-8","content":[],"usage":{"input_tokens":100,"output_tokens":200}}}"#,
            "\n",
        );
        let path = write_temp("main_only", body);
        let agg = aggregate(path.to_str().unwrap()).expect("aggregate");
        assert_eq!(agg.turns, 1);
        assert_eq!(agg.agents.len(), 1);
        let main = agg.agents.get(AGENT_MAIN).expect("main bucket");
        assert_eq!(main.turns, 1);
        assert_eq!(main.total_tokens(), 300);
        assert!(!agg.agents.contains_key(AGENT_SUB));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inline_sidechain_splits_main_vs_sub() {
        // Main turn + a sidechain (sub-agent) turn in the SAME transcript.
        let body = concat!(
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:01Z","isSidechain":false,"message":{"model":"claude-opus-4-8","content":[],"usage":{"input_tokens":100,"output_tokens":100}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:02Z","isSidechain":true,"message":{"model":"claude-opus-4-8","content":[],"usage":{"input_tokens":10,"output_tokens":20}}}"#,
            "\n",
        );
        let path = write_temp("inline_sub", body);
        let agg = aggregate(path.to_str().unwrap()).expect("aggregate");

        // Grand total includes both.
        assert_eq!(agg.turns, 2);
        assert_eq!(
            agg.models.get("claude-opus-4-8").unwrap().total_tokens(),
            230
        );
        // Split by bucket.
        assert_eq!(agg.agents.get(AGENT_MAIN).unwrap().total_tokens(), 200);
        assert_eq!(agg.agents.get(AGENT_SUB).unwrap().total_tokens(), 30);
        assert_eq!(agg.agents.get(AGENT_SUB).unwrap().turns, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sibling_subagent_files_fold_into_sub_bucket() {
        // Newer layout: main transcript at <dir>/<stem>.jsonl, sub-agents under
        // <dir>/<stem>/subagents/*.jsonl. Build that tree in a temp dir.
        let base =
            std::env::temp_dir().join(format!("harness-core-usage-sib-{}", std::process::id()));
        let stem = "sess";
        let sub_dir = base.join(stem).join("subagents");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let main_path = base.join(format!("{stem}.jsonl"));
        std::fs::write(
            &main_path,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-06-22T10:00:01Z","isSidechain":false,"message":{"model":"claude-opus-4-8","content":[],"usage":{"input_tokens":100,"output_tokens":100}}}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            sub_dir.join("agent-aaa.jsonl"),
            concat!(
                r#"{"type":"assistant","timestamp":"2026-06-22T10:00:03Z","isSidechain":true,"message":{"model":"claude-sonnet-4-6","content":[],"usage":{"input_tokens":40,"output_tokens":60}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let agg = aggregate(main_path.to_str().unwrap()).expect("aggregate");
        assert_eq!(agg.turns, 2); // 1 main + 1 sub
        assert_eq!(agg.agents.get(AGENT_MAIN).unwrap().total_tokens(), 200);
        assert_eq!(agg.agents.get(AGENT_SUB).unwrap().total_tokens(), 100);
        // Grand total spans both models.
        assert!(agg.models.contains_key("claude-opus-4-8"));
        assert!(agg.models.contains_key("claude-sonnet-4-6"));
        // last_ts comes only from the main transcript (sibling files don't move it).
        assert_eq!(agg.last_ts.as_deref(), Some("2026-06-22T10:00:01Z"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
