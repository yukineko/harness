//! tracekit span model + defensive loader.
//!
//! tracekit records a condukt run as append-only JSONL — one [`Span`] per line
//! in `~/.tracekit/<run_id>/spans.jsonl`. We mirror tracekit's serde shape
//! field-for-field so a recorded span round-trips, and load defensively: a
//! single corrupt line must never sink a replay, so malformed lines are skipped
//! and counted.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One recorded span of a condukt run. Mirrors tracekit's on-disk serde shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub run_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    /// Free-form phase tag: interpreter | worker | verifier | tool | …
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default)]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub end_unix_ms: u64,
}

fn default_status() -> String {
    "ok".to_string()
}

impl Span {
    /// A span is an error when its status (case-insensitively) is neither "ok"
    /// nor "verified" — those two are the success states a condukt run emits.
    pub fn is_error(&self) -> bool {
        let s = self.status.to_lowercase();
        s != "ok" && s != "verified"
    }
}

/// Stream a spans.jsonl file, returning the parsed spans and the count of
/// malformed lines that were skipped. A missing file is an I/O error; a corrupt
/// *line* is tolerated. Blank lines are skipped without counting as malformed.
pub fn load_spans(path: &Path) -> std::io::Result<(Vec<Span>, usize)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut spans = Vec::new();
    let mut skipped = 0usize;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Span>(&line) {
            Ok(s) => spans.push(s),
            Err(_) => skipped += 1,
        }
    }
    Ok((spans, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn span(status: &str) -> Span {
        Span {
            run_id: "r".into(),
            span_id: "s".into(),
            parent_id: None,
            name: "n".into(),
            phase: "worker".into(),
            model: None,
            task_id: None,
            ms: 0,
            cost_usd: None,
            status: status.into(),
            end_unix_ms: 0,
        }
    }

    #[test]
    fn is_error_for_ok_and_verified_is_false() {
        assert!(!span("ok").is_error());
        assert!(!span("OK").is_error());
        assert!(!span("verified").is_error());
        assert!(!span("Verified").is_error());
    }

    #[test]
    fn is_error_for_failed_error_and_empty_is_true() {
        assert!(span("failed").is_error());
        assert!(span("error").is_error());
        assert!(span("").is_error());
        assert!(span("timeout").is_error());
    }

    #[test]
    fn load_skips_malformed_and_counts_them() {
        let mut path = std::env::temp_dir();
        path.push(format!("replaykit-trace-{}.jsonl", std::process::id()));

        let mut f = File::create(&path).unwrap();
        // good span (status defaults to "ok" when absent)
        writeln!(
            f,
            r#"{{"run_id":"r","span_id":"a","name":"interp","phase":"interpreter","end_unix_ms":1}}"#
        )
        .unwrap();
        // blank line — skipped, NOT counted as malformed
        writeln!(f).unwrap();
        // garbage — counted as malformed
        writeln!(f, "not json at all").unwrap();
        // another good span with explicit fields
        writeln!(
            f,
            r#"{{"run_id":"r","span_id":"b","name":"w","phase":"worker","status":"failed","ms":5,"cost_usd":0.5,"end_unix_ms":2}}"#
        )
        .unwrap();
        f.flush().unwrap();
        drop(f);

        let (spans, skipped) = load_spans(&path).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(skipped, 1);
        assert_eq!(spans[0].status, "ok"); // defaulted
        assert!(!spans[0].is_error());
        assert!(spans[1].is_error());
        assert_eq!(spans[1].cost_usd, Some(0.5));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_io_error() {
        let mut path = std::env::temp_dir();
        path.push(format!("replaykit-nope-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(load_spans(&path).is_err());
    }
}
