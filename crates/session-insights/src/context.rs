//! Context-health join: read context-governor's append-only ledger and summarize
//! window health beside session-insights' own tool-throughput metrics.
//!
//! LOOSE coupling by design — session-insights does NOT depend on the
//! context-governor crate. It reads the same `<state_dir>/ledger.jsonl` file that
//! context-governor writes (one JSON object per line, via
//! `harness_core::metrics::emit`) and replicates the minimal aggregation:
//! count rows, sum `saved_tokens`, group by `event`, and track `resident_tokens`
//! (peak + last).
//!
//! The `<state_dir>` derivation is kept BYTE-FOR-BYTE consistent with
//! context-governor's `backing::TranscriptBackingStore::open` / `ledger::Ledger`:
//!   base    = $CONTEXT_GOVERNOR_STATE_DIR (if set & non-empty)
//!             else $HOME/.context-governor (HOME empty → ".")
//!   project = harness_core::store::project_key(cwd)
//!   session = harness_core::store::safe_session($CLAUDE_CODE_SESSION_ID)
//!             (unset/empty → safe_session("default"))
//!   ledger  = <base>/<project>/<session>/ledger.jsonl
//!
//! Both `project_key` and `safe_session` are the shared harness-core helpers the
//! writer uses, so the join reads exactly the file the writer produced.
//!
//! Best-effort throughout: a missing/empty/corrupt ledger yields a zeroed summary,
//! never a panic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness_core::store::{project_key, safe_session};

/// Joined view of context-governor window health for one project+session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextHealthSummary {
    /// Sum of `saved_tokens` across every recorded row (tokens reclaimed by grooming).
    pub total_saved_tokens: u64,
    /// Total number of ledger rows parsed.
    pub rows: u64,
    /// Count of rows per `event` name, in deterministic key order.
    pub per_action: BTreeMap<String, u64>,
    /// Largest `resident_tokens` seen across rows (window-occupancy high-water mark).
    pub peak_resident_tokens: u64,
    /// `resident_tokens` of the last row (most recent window occupancy estimate).
    pub last_resident_tokens: u64,
    /// The ledger path consulted (for diagnostics / "no context ledger" messaging).
    pub ledger_path: PathBuf,
}

impl ContextHealthSummary {
    /// True when no ledger rows were found (absent/empty file). Drives the
    /// graceful "no context ledger" path in the report.
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }
}

/// Resolve `<state_dir>/ledger.jsonl` for `cwd` the same way context-governor does,
/// then summarize it. Fail-soft: a missing/empty/corrupt ledger yields a zeroed
/// summary (with `ledger_path` populated for messaging), never a panic.
pub fn summarize(cwd: &Path) -> ContextHealthSummary {
    let path = ledger_path(cwd);
    summarize_jsonl(&path)
}

/// Derive the ledger path for `cwd`, mirroring context-governor's writer exactly.
fn ledger_path(cwd: &Path) -> PathBuf {
    let base = std::env::var("CONTEXT_GOVERNOR_STATE_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ".".to_string());
            PathBuf::from(home).join(".context-governor")
        });

    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| safe_session(&s))
        .unwrap_or_else(|| safe_session("default"));

    base.join(project_key(cwd))
        .join(session)
        .join("ledger.jsonl")
}

/// Aggregate a ledger JSONL file at `path`. Pure function of the path (no env), so
/// callers and tests get a deterministic result. Replicates context-governor's
/// `ledger::summarize_jsonl`, additionally tracking `resident_tokens`. Fail-soft:
/// a missing file yields a zeroed summary; corrupt/partial lines are skipped.
fn summarize_jsonl(path: &Path) -> ContextHealthSummary {
    let mut summary = ContextHealthSummary {
        ledger_path: path.to_path_buf(),
        ..Default::default()
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return summary;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        summary.rows += 1;
        summary.total_saved_tokens += value
            .get("saved_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(event) = value.get("event").and_then(|v| v.as_str()) {
            *summary.per_action.entry(event.to_string()).or_insert(0) += 1;
        }
        let resident = value
            .get("resident_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        summary.peak_resident_tokens = summary.peak_resident_tokens.max(resident);
        // Last row wins for `last_resident_tokens` — the ledger is append-only.
        summary.last_resident_tokens = resident;
    }

    summary
}

/// Render the context-health block for the report. Mirrors the indented,
/// `label: value` style of `Session::render_report`. Degrades gracefully:
/// an empty/absent ledger prints a single "no context ledger" line.
pub fn render_block(summary: &ContextHealthSummary) -> String {
    let mut s = String::new();
    s.push_str("  context health:\n");
    if summary.is_empty() {
        s.push_str("    (no context ledger — context-governor has not recorded any rows)\n");
        return s;
    }
    s.push_str(&format!(
        "    rows: {}   saved tokens: {}   resident tokens: {} peak / {} last\n",
        summary.rows,
        summary.total_saved_tokens,
        summary.peak_resident_tokens,
        summary.last_resident_tokens,
    ));
    if !summary.per_action.is_empty() {
        let parts: Vec<String> = summary
            .per_action
            .iter()
            .map(|(action, count)| format!("{action} {count}"))
            .collect();
        s.push_str(&format!("    by action: {}\n", parts.join(", ")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path-pinned core: a synthetic ledger.jsonl yields the expected summary
    /// (total_saved_tokens, rows, per-action counts, peak/last resident).
    #[test]
    fn summarizes_synthetic_ledger() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ledger.jsonl");
        // Mirror the writer's line shape (harness_core::metrics::emit + ledger extra).
        let jsonl = concat!(
            r#"{"session":"S1","event":"groomed","saved_tokens":50,"resident_tokens":1200,"hook":"PostToolUse"}"#,
            "\n",
            r#"{"session":"S1","event":"injected","saved_tokens":0,"resident_tokens":1180,"hook":"UserPromptSubmit"}"#,
            "\n",
            r#"{"session":"S1","event":"groomed","saved_tokens":30,"resident_tokens":900,"hook":"PostToolUse"}"#,
            "\n",
            "   \n",        // blank line is skipped
            "{ not json\n", // corrupt line is skipped, not a panic
        );
        std::fs::write(&path, jsonl).expect("write ledger");

        let summary = summarize_jsonl(&path);
        assert_eq!(summary.rows, 3);
        assert_eq!(summary.total_saved_tokens, 80);
        assert_eq!(summary.per_action.get("groomed"), Some(&2));
        assert_eq!(summary.per_action.get("injected"), Some(&1));
        assert_eq!(summary.peak_resident_tokens, 1200);
        assert_eq!(summary.last_resident_tokens, 900);
        assert_eq!(summary.ledger_path, path);
        assert!(!summary.is_empty());
    }

    /// Missing ledger → zeroed summary, never a panic; `is_empty()` true.
    #[test]
    fn missing_ledger_is_empty_not_panic() {
        let path = Path::new("/tmp/si-context-absent-xyz/ledger.jsonl");
        let summary = summarize_jsonl(path);
        assert_eq!(
            summary,
            ContextHealthSummary {
                ledger_path: path.to_path_buf(),
                ..Default::default()
            }
        );
        assert!(summary.is_empty());
        assert_eq!(summary.total_saved_tokens, 0);
        assert_eq!(summary.rows, 0);
    }

    /// The empty-ledger render path shows the graceful "no context ledger" line.
    #[test]
    fn render_block_degrades_gracefully() {
        let summary = ContextHealthSummary::default();
        let out = render_block(&summary);
        assert!(out.contains("context health"));
        assert!(out.contains("no context ledger"));
    }

    /// A populated summary renders rows / saved / resident / per-action.
    #[test]
    fn render_block_shows_metrics() {
        let mut summary = ContextHealthSummary {
            total_saved_tokens: 80,
            rows: 3,
            peak_resident_tokens: 1200,
            last_resident_tokens: 900,
            ..Default::default()
        };
        summary.per_action.insert("groomed".into(), 2);
        summary.per_action.insert("injected".into(), 1);
        let out = render_block(&summary);
        assert!(out.contains("rows: 3"));
        assert!(out.contains("saved tokens: 80"));
        assert!(out.contains("1200 peak"));
        assert!(out.contains("900 last"));
        assert!(out.contains("groomed 2"));
        assert!(out.contains("injected 1"));
    }

    /// Smoke: the env-derived public entry point reads the env (never mutates it)
    /// and must never panic, even on a cwd that was never written.
    #[test]
    fn summarize_smoke_never_panics() {
        let _ = summarize(Path::new("/tmp/si-context-smoke-cwd"));
    }
}
