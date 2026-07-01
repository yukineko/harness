//! Append-only JSONL metrics — the measurement substrate for "does ctxrot
//! actually keep N down?".
//!
//! Every hook emits one line to `<state_dir>/metrics.jsonl`:
//!   * `budget`   (guard, per prompt) — est_tokens / frac / band / crossed: the token trajectory and every band crossing.
//!   * `rescue`   (rescue + preemptive) — note path + bytes + trigger.
//!   * `restore`  (SessionStart)        — carryover bytes + which sections hit.
//!   * `gate`     (preguard deny)       — the file we kept OUT of context.
//!   * `tooldump` (toolguard)           — a big payload that DID land.
//!   * `inject`   (guard, per prompt)   — chars ctxrot itself injected post-cap;
//!     the in-repo seed for a cross-harness injection budget (ADR 0001).
//!
//! Writes are best-effort and never break a hook: all errors are swallowed, and
//! each line is a single `O_APPEND` write well under PIPE_BUF (4096B), so
//! parallel sessions appending to one file don't interleave. Reading is a
//! forward streaming pass (no whole-file load), per repo policy.

use serde_json::Value;

use crate::config::Config;

/// Path to the metrics log under the state dir.
pub fn path(cfg: &Config) -> std::path::PathBuf {
    cfg.state_dir.join("metrics.jsonl")
}

/// Append one event line `{ts, session, event, ...extra}` via the shared
/// `harness_core` sink. No-op when metrics are disabled. `extra` must be a JSON
/// object; non-object values are ignored. ctxrot keeps its own `SessionStat`
/// rollup below; only the append/row-schema is shared.
pub fn emit(cfg: &Config, session: &str, event: &str, extra: Value) {
    if !cfg.metrics {
        return;
    }
    harness_core::metrics::emit(&path(cfg), session, event, extra);
}

/// Per-session toolguard nudge history, read from the metrics log (forward
/// streaming, fail-soft). `toolguard` emits a `nudge` row (`{event:"nudge",
/// tool}`) each time it actually injects a big-output nudge; this reader replays
/// those rows so the toolguard can dedup an already-nudged rot-source key and cap
/// its total per-session nudges — the seen-state that makes the detector
/// observe→act instead of advising on every oversized output.
///
/// Returns `(seen_keys, total)`: `seen_keys` is the set of rot-source keys (tool
/// names) this session has already been nudged about, and `total` is the
/// session's total nudge count. A missing, empty, or corrupt log — or one with no
/// `nudge` rows for `session` — yields an empty set and `0`. Never panics or
/// unwraps: unreadable lines and malformed/incomplete JSON rows are skipped, so a
/// truncated trailing write or a foreign line can't break the caller.
pub fn nudge_state(cfg: &Config, session: &str) -> (std::collections::HashSet<String>, u64) {
    use std::io::{BufRead, BufReader};

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut total: u64 = 0;

    let file = match std::fs::File::open(path(cfg)) {
        Ok(f) => f,
        Err(_) => return (seen, total),
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
        if o.get("event").and_then(Value::as_str) != Some("nudge") {
            continue;
        }
        if o.get("session").and_then(Value::as_str) != Some(session) {
            continue;
        }
        total += 1;
        if let Some(tool) = o.get("tool").and_then(Value::as_str) {
            seen.insert(tool.to_string());
        }
    }
    (seen, total)
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
    pub anchors: u64,
    /// Prompts spent in each band (index = band: 0 below the lowest band … N at
    /// the top band). The *shape* of occupancy, not just its peak — guard-ON
    /// should spend fewer prompts in the high bands than guard-OFF.
    pub band_prompts: Vec<u64>,
    /// Total chars ctxrot itself injected across the session (post-cap). The
    /// guard's own contribution to per-turn injection — sum it across the harness
    /// family to bound the combined load (ADR 0001).
    pub inject_chars: u64,
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
                let bi = band as usize;
                if s.band_prompts.len() <= bi {
                    s.band_prompts.resize(bi + 1, 0);
                }
                s.band_prompts[bi] += 1;
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
            "anchor" => s.anchors += 1,
            "inject" => s.inject_chars += u("chars"),
            _ => {}
        }
    }

    order.into_iter().filter_map(|k| by.remove(&k)).collect()
}

/// Roll several sessions into one synthetic group for A/B comparison: counts
/// (prompts/crossings/rescues/gates/dumps and their byte totals) sum, while
/// `max_band`/`peak_tokens` take the max across the group (the worst point any
/// one session reached — the figure the guard is meant to hold down).
/// `last_tokens` is meaningless for a group, so it stays 0. The synthetic
/// `session` is set to `label`.
fn fold_group(label: &str, members: &[&SessionStat]) -> SessionStat {
    let mut g = SessionStat {
        session: label.to_string(),
        ..Default::default()
    };
    for m in members {
        g.prompts += m.prompts;
        g.crossings += m.crossings;
        g.max_band = g.max_band.max(m.max_band);
        g.peak_tokens = g.peak_tokens.max(m.peak_tokens);
        g.rescues += m.rescues;
        g.rescue_bytes += m.rescue_bytes;
        g.restores += m.restores;
        g.gates += m.gates;
        g.gate_bytes_saved += m.gate_bytes_saved;
        g.tooldumps += m.tooldumps;
        g.tooldump_bytes += m.tooldump_bytes;
        g.anchors += m.anchors;
        g.inject_chars += m.inject_chars;
        for (i, c) in m.band_prompts.iter().enumerate() {
            if g.band_prompts.len() <= i {
                g.band_prompts.resize(i + 1, 0);
            }
            g.band_prompts[i] += c;
        }
    }
    g
}

/// Render band-dwell counts as `b0=.. b1=.. …` for the A/B occupancy report.
pub fn fmt_dwell(band_prompts: &[u64]) -> String {
    if band_prompts.is_empty() {
        return "(no samples)".to_string();
    }
    band_prompts
        .iter()
        .enumerate()
        .map(|(i, c)| format!("b{i}={c}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Aggregate every session whose id starts with `prefix` into one group stat
/// (so a paste of the truncated id from `ctxrot metrics` resolves, and a task
/// spanning several sessions folds together). Returns `(group, match_count)`,
/// or `None` when nothing matches.
pub fn group_by_prefix(stats: &[SessionStat], prefix: &str) -> Option<(SessionStat, usize)> {
    let members: Vec<&SessionStat> = stats
        .iter()
        .filter(|s| s.session.starts_with(prefix))
        .collect();
    if members.is_empty() {
        return None;
    }
    let n = members.len();
    Some((fold_group(prefix, &members), n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_cfg(name: &str) -> Config {
        // Unique state dir via atomic `mkdtemp` (no pid-collision TOCTOU).
        let dir = tempfile::Builder::new()
            .prefix(&format!("ctxrot-metrics-{name}-"))
            .tempdir()
            .expect("tempdir")
            .keep();
        Config {
            state_dir: dir,
            ..Config::default()
        }
    }

    #[test]
    fn emit_and_summarize() {
        let cfg = temp_cfg("emit");
        emit(
            &cfg,
            "S1",
            "budget",
            json!({"est_tokens": 100_000, "band": 1, "crossed": true}),
        );
        emit(
            &cfg,
            "S1",
            "budget",
            json!({"est_tokens": 150_000, "band": 2, "crossed": true}),
        );
        emit(
            &cfg,
            "S1",
            "rescue",
            json!({"trigger": "band-75%", "note_bytes": 2048}),
        );
        emit(
            &cfg,
            "S1",
            "gate",
            json!({"tool": "Read", "file_bytes": 1_900_000}),
        );
        emit(
            &cfg,
            "S2",
            "budget",
            json!({"est_tokens": 40_000, "band": 0, "crossed": false}),
        );

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
    fn group_by_prefix_folds_matching_sessions() {
        let cfg = temp_cfg("group");
        // Two "A" runs (guard ON) under prefix "a-", one "B" run (guard OFF).
        emit(
            &cfg,
            "a-run1",
            "budget",
            json!({"est_tokens": 120_000, "band": 2, "crossed": true}),
        );
        emit(
            &cfg,
            "a-run2",
            "budget",
            json!({"est_tokens": 90_000, "band": 1, "crossed": true}),
        );
        emit(&cfg, "a-run2", "rescue", json!({"note_bytes": 1000}));
        emit(
            &cfg,
            "b-run1",
            "budget",
            json!({"est_tokens": 180_000, "band": 3, "crossed": true}),
        );

        let stats = summarize(&cfg);
        let (a, na) = group_by_prefix(&stats, "a-").unwrap();
        assert_eq!(na, 2);
        assert_eq!(a.prompts, 2);
        assert_eq!(a.crossings, 2); // summed across both A runs
        assert_eq!(a.max_band, 2); // max across the group
        assert_eq!(a.peak_tokens, 120_000); // worst point any A run reached
        assert_eq!(a.rescues, 1);

        let (b, nb) = group_by_prefix(&stats, "b-").unwrap();
        assert_eq!(nb, 1);
        assert_eq!(b.peak_tokens, 180_000);

        assert!(group_by_prefix(&stats, "zzz").is_none());
        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    #[test]
    fn band_dwell_counts_and_folds() {
        let cfg = temp_cfg("dwell");
        emit(
            &cfg,
            "on-1",
            "budget",
            json!({"est_tokens": 40_000, "band": 0}),
        );
        emit(
            &cfg,
            "on-1",
            "budget",
            json!({"est_tokens": 110_000, "band": 1}),
        );
        emit(
            &cfg,
            "on-1",
            "budget",
            json!({"est_tokens": 160_000, "band": 2}),
        );
        emit(
            &cfg,
            "on-2",
            "budget",
            json!({"est_tokens": 185_000, "band": 3}),
        );
        emit(&cfg, "on-1", "inject", json!({"chars": 300, "blocks": 2}));
        emit(&cfg, "on-2", "inject", json!({"chars": 500, "blocks": 1}));

        let stats = summarize(&cfg);
        let s1 = stats.iter().find(|s| s.session == "on-1").unwrap();
        assert_eq!(s1.band_prompts, vec![1, 1, 1]); // one prompt each in b0/b1/b2
        assert_eq!(s1.inject_chars, 300);

        // Folding a group sums band dwell element-wise (b3 from on-2 included).
        let (g, n) = group_by_prefix(&stats, "on-").unwrap();
        assert_eq!(n, 2);
        assert_eq!(g.band_prompts, vec![1, 1, 1, 1]);
        assert_eq!(fmt_dwell(&g.band_prompts), "b0=1 b1=1 b2=1 b3=1");
        assert_eq!(g.inject_chars, 800); // summed across the group

        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    #[test]
    fn nudge_state_missing_log_is_empty() {
        // No metrics.jsonl on disk → empty seen-set and zero total, no panic.
        let cfg = temp_cfg("nudge-missing");
        let (seen, total) = nudge_state(&cfg, "S1");
        assert!(seen.is_empty());
        assert_eq!(total, 0);
        assert!(!path(&cfg).exists());
    }

    #[test]
    fn nudge_state_counts_keys_and_total_per_session() {
        let cfg = temp_cfg("nudge-count");
        emit(&cfg, "S1", "nudge", json!({"tool": "Read"}));
        emit(&cfg, "S1", "nudge", json!({"tool": "Read"})); // repeat key
        emit(&cfg, "S1", "nudge", json!({"tool": "Bash"}));
        // Other sessions and other events must not leak into S1's state.
        emit(&cfg, "S2", "nudge", json!({"tool": "Grep"}));
        emit(&cfg, "S1", "tooldump", json!({"tool": "Glob", "bytes": 99}));

        let (seen, total) = nudge_state(&cfg, "S1");
        assert_eq!(total, 3); // three nudge rows for S1 (incl. the repeat)
        assert_eq!(seen.len(), 2); // distinct keys: Read, Bash
        assert!(seen.contains("Read"));
        assert!(seen.contains("Bash"));
        assert!(!seen.contains("Grep")); // belongs to S2
        assert!(!seen.contains("Glob")); // a tooldump, not a nudge

        let (s2_seen, s2_total) = nudge_state(&cfg, "S2");
        assert_eq!(s2_total, 1);
        assert!(s2_seen.contains("Grep"));

        let _ = std::fs::remove_dir_all(&cfg.state_dir);
    }

    #[test]
    fn nudge_state_skips_empty_and_corrupt_lines() {
        // A log with blank lines, malformed JSON, and a nudge row missing its
        // `tool` field must not panic: bad rows are skipped, the valid one counts,
        // and a tool-less nudge bumps `total` without adding a key.
        let cfg = temp_cfg("nudge-corrupt");
        emit(&cfg, "S1", "nudge", json!({"tool": "Read"}));
        let p = path(&cfg);
        let mut existing = std::fs::read_to_string(&p).unwrap_or_default();
        existing.push('\n'); // blank line
        existing.push_str("{ this is not json }\n"); // corrupt
        existing.push_str("{\"session\":\"S1\",\"event\":\"nudge\"}\n"); // no `tool`
        std::fs::write(&p, existing).unwrap();

        let (seen, total) = nudge_state(&cfg, "S1");
        assert_eq!(total, 2); // the Read row + the tool-less nudge row
        assert_eq!(seen.len(), 1); // only Read contributed a key
        assert!(seen.contains("Read"));

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
