//! `condukt state check-oracle` — deterministically ask `tdd oracle` whether a
//! fix/feature task's RED→GREEN proofs form a valid Fail→Pass reproduction
//! oracle. This is a thin, fail-soft wrapper: any external failure (no `tdd`
//! on PATH, spawn failure, missing/corrupt stdout, a gone worktree) degrades
//! to `fallback:true` so the caller can fall back to the legacy gate — it
//! never panics and never exits nonzero.

use std::path::Path;
use std::process::Command;

/// Parse `tdd oracle`'s stdout JSON, returning `(valid_fp_oracle, transition)`.
/// Corrupt/non-object stdout is reported as `(false, None)` rather than
/// panicking — this is a pure helper so it is fully unit-testable without
/// spawning a real `tdd` process.
pub fn interpret_oracle_stdout(stdout: &str) -> (bool, Option<String>) {
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(v) => {
            let valid = v
                .get("valid_fp_oracle")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let transition = v
                .get("transition")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            (valid, transition)
        }
        Err(_) => (false, None),
    }
}

/// Decide whether `task_id` (whose worktree is `run_dir`) carries a valid
/// Fail→Pass reproduction oracle, deferring to `tdd oracle --task <id>` run in
/// `run_dir`. Pure aside from the one external process spawn; never panics.
///
/// Always returns a JSON object with a `fallback` bool: `true` means "the
/// oracle check itself could not be trusted (or does not apply) — degrade to
/// the legacy gate", `false` means the `valid_fp_oracle`/`transition` fields
/// reflect a real `tdd oracle` verdict.
pub fn check_oracle(
    requires_oracle: bool,
    reproduction_tests: Option<&str>,
    task_id: &str,
    run_dir: &Path,
) -> serde_json::Value {
    if !requires_oracle || reproduction_tests.is_none() {
        return serde_json::json!({
            "required": false,
            "valid_fp_oracle": false,
            "fallback": true,
            "reason": "not a fix/feature task or no reproduction_tests",
        });
    }

    match Command::new("tdd")
        .args(["oracle", "--task", task_id])
        .current_dir(run_dir)
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.trim().is_empty() {
                return serde_json::json!({
                    "required": true,
                    "valid_fp_oracle": false,
                    "fallback": true,
                    "reason": "tdd oracle produced no stdout",
                });
            }
            // Confirm the stdout is well-formed JSON before trusting the
            // verdict; `interpret_oracle_stdout` already defaults missing
            // fields to false/None, but corrupt/non-JSON stdout must degrade
            // to fallback rather than silently reporting `valid_fp_oracle:false`.
            if serde_json::from_str::<serde_json::Value>(&stdout).is_err() {
                return serde_json::json!({
                    "required": true,
                    "valid_fp_oracle": false,
                    "fallback": true,
                    "reason": "could not parse tdd oracle stdout as JSON",
                });
            }
            let (valid, transition) = interpret_oracle_stdout(&stdout);
            serde_json::json!({
                "required": true,
                "valid_fp_oracle": valid,
                "fallback": false,
                "transition": transition,
                "reason": if valid {
                    "fail-to-pass oracle confirmed"
                } else {
                    "tdd oracle did not report a valid fail-to-pass transition"
                },
            })
        }
        Err(e) => serde_json::json!({
            "required": true,
            "valid_fp_oracle": false,
            "fallback": true,
            "reason": format!("failed to spawn tdd: {e}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_required_falls_back_immediately() {
        let out = check_oracle(false, Some("cargo test -p x"), "t1", Path::new("."));
        assert_eq!(out["required"], false);
        assert_eq!(out["fallback"], true);
        assert_eq!(out["valid_fp_oracle"], false);
    }

    #[test]
    fn no_reproduction_tests_falls_back_even_when_required() {
        let out = check_oracle(true, None, "t1", Path::new("."));
        assert_eq!(out["required"], false);
        assert_eq!(out["fallback"], true);
        assert_eq!(out["valid_fp_oracle"], false);
    }

    /// Spawning `tdd` with a nonexistent `current_dir` reliably fails the
    /// spawn regardless of whether a `tdd` binary happens to be on PATH in
    /// the test environment — this exercises the "tdd unreachable" fallback
    /// path deterministically.
    #[test]
    fn spawn_failure_falls_back() {
        let bogus_dir = std::env::temp_dir().join("condukt-oracle-test-nonexistent-dir-zzz-987654");
        let _ = std::fs::remove_dir_all(&bogus_dir);
        assert!(!bogus_dir.exists());

        let out = check_oracle(true, Some("cargo test -p x"), "t1", &bogus_dir);
        assert_eq!(out["required"], true);
        assert_eq!(out["fallback"], true);
        assert_eq!(out["valid_fp_oracle"], false);
    }

    #[test]
    fn interpret_valid_fp_oracle_stdout() {
        let (valid, transition) =
            interpret_oracle_stdout(r#"{"valid_fp_oracle":true,"transition":"FailToPass"}"#);
        assert!(valid);
        assert_eq!(transition.as_deref(), Some("FailToPass"));
    }

    #[test]
    fn interpret_invalid_fp_oracle_stdout() {
        let (valid, transition) =
            interpret_oracle_stdout(r#"{"valid_fp_oracle":false,"transition":"fail_to_fail"}"#);
        assert!(!valid);
        assert_eq!(transition.as_deref(), Some("fail_to_fail"));
    }

    #[test]
    fn interpret_corrupt_stdout_defaults_to_invalid() {
        let (valid, transition) = interpret_oracle_stdout("not json");
        assert!(!valid);
        assert_eq!(transition, None);
    }
}
