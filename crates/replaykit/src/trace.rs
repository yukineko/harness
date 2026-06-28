//! tracekit span model + defensive loader — re-exported from the shared crate.
//!
//! The [`Span`] schema and its skip-malformed JSONL loader are owned by
//! [`harness_core::spans`], the single source of truth shared with tracekit (it
//! *writes* the spans this crate *reads*), so a recorded span round-trips
//! byte-for-byte. We re-export under the names replaykit already uses
//! (`load_spans`) to keep the call sites unchanged.

pub use harness_core::spans::{load_from as load_spans, Span};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn is_error_for_ok_and_verified_is_false() {
        let mut s = sample("ok");
        assert!(!s.is_error());
        s.status = "OK".into();
        assert!(!s.is_error());
        s.status = "verified".into();
        assert!(!s.is_error());
        s.status = "Verified".into();
        assert!(!s.is_error());
    }

    #[test]
    fn is_error_for_failed_error_and_empty_is_true() {
        for status in ["failed", "error", "", "timeout"] {
            assert!(sample(status).is_error(), "{status:?} should be an error");
        }
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

    fn sample(status: &str) -> Span {
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
}
