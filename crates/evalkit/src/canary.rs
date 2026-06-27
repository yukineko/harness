//! Canary diff: compare two `evalkit run --json` result sets — a *baseline*
//! (e.g. SKILL.md before an edit) against a *current* run — and surface what
//! moved. It is the pure diff layer behind the promptfoo-style side-by-side:
//! replay the same golden dataset across two prompt versions and classify every
//! case as a regression, fix, added, dropped, or unchanged.
//!
//! The classification core (`diff`) is pure over two case-maps so it is fully
//! unit-testable without touching the filesystem — mirroring how `run.rs` keeps
//! `check_assert` pure. Exit-code policy matches the rest of evalkit: `0` ok,
//! `1` a real regression (only when `--fail-on-regression`), `2` harness error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

/// One case's outcome distilled from an `evalkit run --json` `cases[]` entry.
/// A `skipped` case (a draft) is neither a pass nor a fail; `pass` is then false.
#[derive(Clone, Copy)]
pub struct CaseResult {
    pub pass: bool,
    pub skipped: bool,
}

/// The result of diffing a baseline against a current run. Case labels are kept
/// per bucket (sorted) so the report can name exactly what changed; `unchanged`
/// is only a count since listing every steady case adds noise.
pub struct Diff {
    pub baseline_pass_rate: f64,
    pub current_pass_rate: f64,
    /// baseline.pass && current.fail (neither skipped) — the gating bucket.
    pub regressions: Vec<String>,
    /// baseline.fail && current.pass (neither skipped).
    pub fixes: Vec<String>,
    /// Present only in the current run.
    pub added: Vec<String>,
    /// Present only in the baseline run.
    pub dropped: Vec<String>,
    /// Count of cases present on both sides with no class-changing transition
    /// (includes benign skip transitions like pass→skip or skip→pass).
    pub unchanged: usize,
}

impl Diff {
    pub fn delta(&self) -> f64 {
        self.current_pass_rate - self.baseline_pass_rate
    }
}

/// Pass-rate over a case-map: passed / (total - skipped). A skipped draft is
/// excluded from the denominator; an all-skipped (or empty) map → 0.0 (no
/// divide-by-zero).
fn pass_rate(cases: &BTreeMap<String, CaseResult>) -> f64 {
    let skipped = cases.values().filter(|c| c.skipped).count();
    let denom = cases.len() - skipped;
    if denom == 0 {
        return 0.0;
    }
    let passed = cases.values().filter(|c| c.pass).count();
    passed as f64 / denom as f64
}

/// Pure classification core: diff two case-maps keyed by `case` label.
///
/// A `regression` is *strictly* `baseline.pass == true && current.pass == false`
/// with neither side skipped — that is the only condition that may gate the
/// process. A `fix` is the mirror (baseline fail → current pass). Skipped
/// transitions (pass→skip, skip→pass, skip→skip) are intentionally NOT
/// regressions/fixes; they fall through to `unchanged` since a draft carries no
/// pass/fail signal to regress or fix.
pub fn diff(
    baseline: &BTreeMap<String, CaseResult>,
    current: &BTreeMap<String, CaseResult>,
) -> Diff {
    let mut regressions = Vec::new();
    let mut fixes = Vec::new();
    let mut added = Vec::new();
    let mut dropped = Vec::new();
    let mut unchanged = 0usize;

    // Union of labels; BTreeMap iteration is sorted, so each bucket stays sorted.
    let mut labels: Vec<&String> = baseline.keys().chain(current.keys()).collect();
    labels.sort();
    labels.dedup();

    for label in labels {
        match (baseline.get(label), current.get(label)) {
            (Some(b), Some(c)) => {
                // A real pass/fail flip requires both sides to carry a verdict.
                let b_fail = !b.pass && !b.skipped;
                let c_fail = !c.pass && !c.skipped;
                if b.pass && c_fail {
                    regressions.push(label.clone());
                } else if b_fail && c.pass {
                    fixes.push(label.clone());
                } else {
                    unchanged += 1;
                }
            }
            (None, Some(_)) => added.push(label.clone()),
            (Some(_), None) => dropped.push(label.clone()),
            (None, None) => unreachable!("label came from one of the two maps"),
        }
    }

    Diff {
        baseline_pass_rate: pass_rate(baseline),
        current_pass_rate: pass_rate(current),
        regressions,
        fixes,
        added,
        dropped,
        unchanged,
    }
}

/// Parse one `evalkit run --json` report into a case-map keyed by `case` label.
/// Missing `pass`/`skipped` default to false so a partial report still diffs.
fn parse_report(path: &Path) -> Result<BTreeMap<String, CaseResult>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let cases = value
        .get("cases")
        .and_then(|c| c.as_array())
        .with_context(|| {
            format!(
                "{}: no `cases` array (not an `evalkit run --json` output?)",
                path.display()
            )
        })?;
    let mut map = BTreeMap::new();
    for c in cases {
        let label = c.get("case").and_then(|l| l.as_str()).with_context(|| {
            format!(
                "{}: a case entry is missing its `case` label",
                path.display()
            )
        })?;
        let pass = c.get("pass").and_then(|p| p.as_bool()).unwrap_or(false);
        let skipped = c.get("skipped").and_then(|s| s.as_bool()).unwrap_or(false);
        map.insert(label.to_string(), CaseResult { pass, skipped });
    }
    Ok(map)
}

/// Read+parse the two reports, diff them, print a human report or `--json`
/// summary, and return the exit code. `2` on any harness error (unreadable /
/// unparseable file), else `1` only when `fail_on_regression` is set AND there
/// is at least one regression, else `0`.
pub fn execute(
    baseline: PathBuf,
    current: PathBuf,
    json_out: bool,
    fail_on_regression: bool,
) -> i32 {
    let (base_map, cur_map) = match (parse_report(&baseline), parse_report(&current)) {
        (Ok(b), Ok(c)) => (b, c),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("evalkit: {e:#}");
            return 2;
        }
    };
    let d = diff(&base_map, &cur_map);

    if json_out {
        report_json(&d);
    } else {
        report_human(&d);
    }

    if fail_on_regression && !d.regressions.is_empty() {
        1
    } else {
        0
    }
}

fn report_human(d: &Diff) {
    let before = d.baseline_pass_rate * 100.0;
    let after = d.current_pass_rate * 100.0;
    let delta = d.delta() * 100.0;
    println!("evalkit canary: pass-rate {before:.1}% → {after:.1}% ({delta:+.1} pts)\n");

    if d.regressions.is_empty() {
        println!("  no regressions");
    } else {
        println!("  !! {} REGRESSION(S) (pass → fail):", d.regressions.len());
        for label in &d.regressions {
            println!("     !! {label}");
        }
    }
    print_bucket("fixes (fail → pass)", &d.fixes);
    print_bucket("added (new in current)", &d.added);
    print_bucket("dropped (gone from current)", &d.dropped);
    println!("\n  {} unchanged", d.unchanged);
}

fn print_bucket(title: &str, labels: &[String]) {
    if labels.is_empty() {
        return;
    }
    println!("  {} {}:", labels.len(), title);
    for label in labels {
        println!("     - {label}");
    }
}

fn report_json(d: &Diff) {
    println!(
        "{}",
        json!({
            "baseline_pass_rate": d.baseline_pass_rate,
            "current_pass_rate": d.current_pass_rate,
            "delta": d.delta(),
            "regressions": d.regressions,
            "fixes": d.fixes,
            "added": d.added,
            "dropped": d.dropped,
        })
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, bool, bool)]) -> BTreeMap<String, CaseResult> {
        entries
            .iter()
            .map(|(label, pass, skipped)| {
                (
                    label.to_string(),
                    CaseResult {
                        pass: *pass,
                        skipped: *skipped,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn pass_to_fail_is_a_regression() {
        let b = map(&[("a", true, false)]);
        let c = map(&[("a", false, false)]);
        let d = diff(&b, &c);
        assert_eq!(d.regressions, vec!["a".to_string()]);
        assert!(d.fixes.is_empty());
    }

    #[test]
    fn fail_to_pass_is_a_fix_and_does_not_gate() {
        let b = map(&[("a", false, false)]);
        let c = map(&[("a", true, false)]);
        let d = diff(&b, &c);
        assert_eq!(d.fixes, vec!["a".to_string()]);
        assert!(d.regressions.is_empty());
        // A fix alone never drives exit 1, even under --fail-on-regression.
        assert!(d.regressions.is_empty());
    }

    #[test]
    fn added_and_dropped_detected() {
        let b = map(&[("only_base", true, false)]);
        let c = map(&[("only_cur", true, false)]);
        let d = diff(&b, &c);
        assert_eq!(d.added, vec!["only_cur".to_string()]);
        assert_eq!(d.dropped, vec!["only_base".to_string()]);
        assert!(d.regressions.is_empty());
    }

    #[test]
    fn skipped_transitions_are_not_regressions() {
        // pass → skip and skip → fail are both benign (no pass/fail verdict flip).
        let b = map(&[("a", true, false), ("b", false, true)]);
        let c = map(&[("a", false, true), ("b", false, false)]);
        let d = diff(&b, &c);
        assert!(d.regressions.is_empty(), "{:?}", d.regressions);
        assert!(d.fixes.is_empty(), "{:?}", d.fixes);
        assert_eq!(d.unchanged, 2);
    }

    #[test]
    fn pass_rate_math_and_divide_by_zero() {
        // 2 of 3 non-skipped pass → 2/3; one skipped excluded from denominator.
        let m = map(&[
            ("a", true, false),
            ("b", true, false),
            ("c", false, false),
            ("d", false, true),
        ]);
        assert!((pass_rate(&m) - 2.0 / 3.0).abs() < 1e-9);

        // All skipped → denominator 0 → 0.0, no panic.
        let all_skip = map(&[("a", false, true)]);
        assert_eq!(pass_rate(&all_skip), 0.0);

        // Empty map → 0.0.
        let empty: BTreeMap<String, CaseResult> = BTreeMap::new();
        assert_eq!(pass_rate(&empty), 0.0);
    }

    #[test]
    fn delta_is_current_minus_baseline() {
        let b = map(&[("a", true, false), ("x", false, false)]); // 0.5
        let c = map(&[("a", true, false), ("x", true, false)]); // 1.0
        let d = diff(&b, &c);
        assert!((d.delta() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn fail_on_regression_exit_policy() {
        // Build two tiny reports on disk and exercise the real exit-code policy.
        let dir = std::env::temp_dir().join(format!("evalkit-canary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("base.json");
        let cur = dir.join("cur.json");
        std::fs::write(
            &base,
            r#"{"total":1,"passed":1,"failed":0,"skipped":0,"cases":[{"case":"a","pass":true,"skipped":false,"failures":[]}]}"#,
        )
        .unwrap();
        std::fs::write(
            &cur,
            r#"{"total":1,"passed":0,"failed":1,"skipped":0,"cases":[{"case":"a","pass":false,"skipped":false,"failures":["boom"]}]}"#,
        )
        .unwrap();

        // A regression exists, but without the flag we still exit 0.
        assert_eq!(execute(base.clone(), cur.clone(), true, false), 0);
        // With the flag, the regression gates → exit 1.
        assert_eq!(execute(base.clone(), cur.clone(), false, true), 1);
        // Diffing a report against itself: no regression → exit 0 even with flag.
        assert_eq!(execute(base.clone(), base.clone(), false, true), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_file_is_harness_error_exit_2() {
        let missing = PathBuf::from("/no/such/evalkit/report.json");
        assert_eq!(execute(missing.clone(), missing, false, true), 2);
    }
}
