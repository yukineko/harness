//! Append-only JSONL metrics — the measurement substrate for "does ctxrot
//! actually keep N down?".
//!
//! Every hook emits one line to `<state_dir>/metrics.jsonl`:
//!   * `budget`   (guard, per prompt) — est_tokens / frac / band / crossed: the
//!                token TRAJECTORY and every band crossing.
//!   * `rescue`   (rescue + preemptive) — note path + bytes + trigger.
//!   * `restore`  (SessionStart)        — carryover bytes + which sections hit.
//!   * `gate`     (preguard deny)       — the file we kept OUT of context.
//!   * `tooldump` (toolguard)           — a big payload that DID land.
//!
//! Writes are best-effort and never break a hook: all errors are swallowed, and
//! each line is a single `O_APPEND` write well under PIPE_BUF (4096B), so
//! parallel sessions appending to one file don't interleave. Reading is a
//! forward streaming pass (no whole-file load), per repo policy.

use std::io::Write;

use serde_json::{json, Value};

use crate::config::Config;

/// Path to the metrics log under the state dir.
pub fn path(cfg: &Config) -> std::path::PathBuf {
    cfg.state_dir.join("metrics.jsonl")
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Append one event line `{ts, session, event, ...extra}`. No-op when metrics are
/// disabled. `extra` must be a JSON object; non-object values are ignored.
pub fn emit(cfg: &Config, session: &str, event: &str, extra: Value) {
    if !cfg.metrics {
        return;
    }
    let mut obj = serde_json::Map::new();
    obj.insert("ts".into(), json!(now_iso()));
    obj.insert("session".into(), json!(session));
    obj.insert("event".into(), json!(event));
    if let Value::Object(m) = extra {
        for (k, v) in m {
            obj.insert(k, v);
        }
    }
    let line = Value::Object(obj).to_string();

    let _ = std::fs::create_dir_all(&cfg.state_dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path(cfg))
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Per-session rollup for `ctxrot metrics`.
#[derive(Default)]
pub struct SessionStat {
    pub session: String,
    pub prompts: u64,
    pub crossings: u64,
    pub max_band: u64,
    pub last_tokens: u64,
    pub peak_tokens: u64,
    pub rescues: u64,
    pub rescue_bytes: u64,
    pub restores: u64,
    pub gates: u64,
    pub gate_bytes_saved: u64,
    pub tooldumps: u64,
    pub tooldump_bytes: u64,
}

/// Stream the metrics log and roll up per session, preserving first-seen order.
pub fn summarize(cfg: &Config) -> Vec<SessionStat> {
    use std::io::{BufRead, BufReader};

    let mut order: Vec<String> = Vec::new();
    let mut by: std::collections::HashMap<String, SessionStat> = std::collections::HashMap::new();

    let file = match std::fs::File::open(path(cfg)) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let o: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let session = o.get("session").and_then(Value::as_str).unwrap_or("?");
        let event = o.get("event").and_then(Value::as_str).unwrap_or("");
        let u = |k: &str| o.get(k).and_then(Value::as_u64).unwrap_or(0);

        if !by.contains_key(session) {
            order.push(session.to_string());
            by.insert(
                session.to_string(),
                SessionStat {
                    session: session.to_string(),
                    ..Default::default()
                },
            );
        }
        let s = by.get_mut(session).unwrap();
        match event {
            "budget" => {
                s.prompts += 1;
                let band = u("band");
                if band > s.max_band {
                    s.max_band = band;
                }
                if o.get("crossed").and_then(Value::as_bool).unwrap_or(false) {
                    s.crossings += 1;
                }
                let t = u("est_tokens");
                s.last_tokens = t;
                if t > s.peak_tokens {
                    s.peak_tokens = t;
                }
            }
            "rescue" => {
                s.rescues += 1;
                s.rescue_bytes += u("note_bytes");
            }
            "restore" => s.restores += 1,
            "gate" => {
                s.gates += 1;
                s.gate_bytes_saved += u("file_bytes");
            }
            "tooldump" => {
                s.tooldumps += 1;
                s.tooldump_bytes += u("bytes");
            }
            _ => {}
        }
    }

    order.into_iter().filter_map(|k| by.remove(&k)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cfg(name: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("ctxrot-metrics-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Config {
            state_dir: dir,
            ..Config::default()
        }
    }

    #[test]
    fn emit_and_summarize() {
        let cfg = temp_cfg("emit");
        emit(&cfg, "S1", "budget", json!({"est_tokens": 100_000, "band": 1, "crossed": true}));
        emit(&cfg, "S1", "budget", json!({"est_tokens": 150_000, "band": 2, "crossed": true}));
        emit(&cfg, "S1", "rescue", json!({"trigger": "band-75%", "note_bytes": 2048}));
        emit(&cfg, "S1", "gate", json!({"tool": "Read", "file_bytes": 1_900_000}));
        emit(&cfg, "S2", "budget", json!({"est_tokens": 40_000, "band": 0, "crossed": false}));

        let stats = summarize(&cfg);
        assert_eq!(stats.len(), 2);
        let s1 = &stats[0]; // first-seen order
        assert_eq!(s1.session, "S1");
        assert_eq!(s1.prompts, 2);
        assert_eq!(s1.crossings, 2);
        assert_eq!(s1.max_band, 2);
        assert_eq!(s1.peak_tokens, 150_000);
        assert_eq!(s1.last_tokens, 150_000);
        assert_eq!(s1.rescues, 1);
        assert_eq!(s1.rescue_bytes, 2048);
        assert_eq!(s1.gates, 1);
        assert_eq!(s1.gate_bytes_saved, 1_900_000);
        assert_eq!(stats[1].session, "S2");

        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    #[test]
    fn disabled_writes_nothing() {
        let mut cfg = temp_cfg("disabled");
        cfg.metrics = false;
        emit(&cfg, "S1", "budget", json!({"est_tokens": 1}));
        assert!(summarize(&cfg).is_empty());
        assert!(!path(&cfg).exists());
    }
}
