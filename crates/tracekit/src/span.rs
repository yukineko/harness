//! Span schema + append-only per-run store.
//!
//! One condukt run is a tree of spans: the interpreter span is the root, each
//! worker/verifier a child (linked by `parent_id`). Spans are appended as JSONL
//! to `~/.tracekit/<run_id>/spans.jsonl` — append-only so concurrent phase
//! completions never clobber each other, and a partial run is still readable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use harness_core::config::base_dir;
use serde::{Deserialize, Serialize};

/// One recorded phase of a run. `ms`/`cost_usd`/`status` are the post-hoc
/// measurements a caller (condukt's state-set transition) supplies when a phase
/// finishes; `end_unix_ms` is stamped at record time so an absolute timeline can
/// be reconstructed for OTLP export (start = end − ms).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub run_id: String,
    /// Caller-chosen stable id (e.g. the condukt task id). Hashed to OTLP hex.
    pub span_id: String,
    /// Parent span's `span_id`; `None`/absent for the run root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    /// Phase bucket: interpreter | worker | verifier | tool | … (free-form).
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Wall-clock duration of the phase, milliseconds.
    #[serde(default)]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Terminal status of the phase: ok | error | … (free-form).
    #[serde(default = "default_status")]
    pub status: String,
    /// Epoch millis at which the phase ended (record time).
    #[serde(default)]
    pub end_unix_ms: u64,
}

fn default_status() -> String {
    "ok".to_string()
}

impl Span {
    /// Epoch millis at which the phase began (end − duration, saturating).
    pub fn start_unix_ms(&self) -> u64 {
        self.end_unix_ms.saturating_sub(self.ms)
    }

    /// True if this phase ended in a non-ok terminal state.
    pub fn is_error(&self) -> bool {
        !self.status.eq_ignore_ascii_case("ok") && !self.status.eq_ignore_ascii_case("verified")
    }
}

/// Directory holding one run's spans: `~/.tracekit/<run_id>/`.
pub fn run_dir(run_id: &str) -> PathBuf {
    base_dir("tracekit").join(sanitize(run_id))
}

/// `~/.tracekit/<run_id>/spans.jsonl`.
pub fn spans_path(run_id: &str) -> PathBuf {
    run_dir(run_id).join("spans.jsonl")
}

/// Keep a run id filesystem-safe (it flows in from CLI / condukt RIDs).
fn sanitize(run_id: &str) -> String {
    run_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Append one span to its run's JSONL store (creating the run dir on first use).
pub fn append(span: &Span) -> Result<PathBuf> {
    use std::io::Write;
    let dir = run_dir(&span.run_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = spans_path(&span.run_id);
    let line = serde_json::to_string(span).context("serializing span")?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    writeln!(f, "{line}").with_context(|| format!("appending to {}", path.display()))?;
    Ok(path)
}

/// Load all spans recorded for a run, in record order. Malformed lines are
/// skipped (a corrupt line shouldn't sink the whole trace) but counted in the
/// returned `skipped` so the caller can warn.
pub fn load(run_id: &str) -> Result<(Vec<Span>, usize)> {
    let path = spans_path(run_id);
    load_from(&path)
}

/// Load spans from an explicit path (split out for tests).
pub fn load_from(path: &Path) -> Result<(Vec<Span>, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut spans = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Span>(line) {
            Ok(s) => spans.push(s),
            Err(_) => skipped += 1,
        }
    }
    Ok((spans, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_is_end_minus_duration_saturating() {
        let mut s = sample("r1", "a", None);
        s.ms = 300;
        s.end_unix_ms = 1000;
        assert_eq!(s.start_unix_ms(), 700);
        s.end_unix_ms = 100; // duration exceeds end → saturate at 0
        assert_eq!(s.start_unix_ms(), 0);
    }

    #[test]
    fn error_status_detection() {
        let mut s = sample("r1", "a", None);
        s.status = "ok".into();
        assert!(!s.is_error());
        s.status = "verified".into();
        assert!(!s.is_error());
        s.status = "failed".into();
        assert!(s.is_error());
    }

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize("a/b c:d"), "a_b_c_d");
        assert_eq!(sanitize("run-123_x"), "run-123_x");
    }

    #[test]
    fn load_skips_malformed_lines() {
        let dir = std::env::temp_dir().join(format!("tracekit-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("spans.jsonl");
        let good = serde_json::to_string(&sample("r1", "a", None)).unwrap();
        std::fs::write(&p, format!("{good}\n{{bad json}}\n\n{good}\n")).unwrap();
        let (spans, skipped) = load_from(&p).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(skipped, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    pub(super) fn sample(run: &str, id: &str, parent: Option<&str>) -> Span {
        Span {
            run_id: run.into(),
            span_id: id.into(),
            parent_id: parent.map(|s| s.to_string()),
            name: id.into(),
            phase: "worker".into(),
            model: Some("sonnet".into()),
            task_id: None,
            ms: 100,
            cost_usd: Some(0.01),
            status: "ok".into(),
            end_unix_ms: 1000,
        }
    }
}
