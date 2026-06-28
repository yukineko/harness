//! Shared span model + defensive JSONL loader.
//!
//! A condukt run is recorded as append-only JSONL — one [`Span`] per line in
//! `~/.tracekit/<run_id>/spans.jsonl`. tracekit *writes* these spans and
//! replaykit *reads* them, so the serde shape is the on-disk contract between
//! the two: it must stay byte-stable. The loader is defensive — a single
//! corrupt line must never sink a trace or a replay, so malformed lines are
//! skipped and counted (blank lines are skipped without counting).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One recorded phase of a run. `ms`/`cost_usd`/`status` are the post-hoc
/// measurements a caller (condukt's state-set transition) supplies when a phase
/// finishes; `end_unix_ms` is stamped at record time so an absolute timeline can
/// be reconstructed for OTLP export (start = end − ms).
///
/// Field order is the canonical serialized order (serde follows struct order);
/// do not reorder without accepting a change to the on-disk JSON.
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

    /// True if this phase ended in a non-ok terminal state. "ok" and "verified"
    /// (case-insensitively) are the success states a condukt run emits.
    pub fn is_error(&self) -> bool {
        !self.status.eq_ignore_ascii_case("ok") && !self.status.eq_ignore_ascii_case("verified")
    }
}

/// Load spans from an explicit JSONL path, in record order. A missing file is an
/// I/O error; a corrupt *line* is tolerated — malformed lines are skipped and
/// counted in the returned `skipped` so the caller can warn. Blank lines are
/// skipped without counting as malformed.
pub fn load_from(path: &Path) -> std::io::Result<(Vec<Span>, usize)> {
    let text = std::fs::read_to_string(path)?;
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

    fn sample(run: &str, id: &str, parent: Option<&str>) -> Span {
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
        s.status = "OK".into();
        assert!(!s.is_error());
        s.status = "verified".into();
        assert!(!s.is_error());
        s.status = "Verified".into();
        assert!(!s.is_error());
        s.status = "failed".into();
        assert!(s.is_error());
        s.status = "error".into();
        assert!(s.is_error());
        s.status = "".into();
        assert!(s.is_error());
        s.status = "timeout".into();
        assert!(s.is_error());
    }

    #[test]
    fn load_skips_malformed_lines() {
        let dir = std::env::temp_dir().join(format!("harness-spans-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("spans.jsonl");
        let good = serde_json::to_string(&sample("r1", "a", None)).unwrap();
        std::fs::write(&p, format!("{good}\n{{bad json}}\n\n{good}\n")).unwrap();
        let (spans, skipped) = load_from(&p).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(skipped, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_defaults_status_and_reads_explicit_fields() {
        let dir = std::env::temp_dir().join(format!("harness-spans-def-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("spans.jsonl");
        std::fs::write(
            &p,
            concat!(
                r#"{"run_id":"r","span_id":"a","name":"interp","phase":"interpreter","end_unix_ms":1}"#,
                "\n",
                "not json at all\n",
                r#"{"run_id":"r","span_id":"b","name":"w","phase":"worker","status":"failed","ms":5,"cost_usd":0.5,"end_unix_ms":2}"#,
                "\n",
            ),
        )
        .unwrap();
        let (spans, skipped) = load_from(&p).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(skipped, 1);
        assert_eq!(spans[0].status, "ok"); // defaulted
        assert!(!spans[0].is_error());
        assert!(spans[1].is_error());
        assert_eq!(spans[1].cost_usd, Some(0.5));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_io_error() {
        let p =
            std::env::temp_dir().join(format!("harness-spans-nope-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&p);
        assert!(load_from(&p).is_err());
    }
}
