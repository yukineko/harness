//! Verifier-stage invariants — enforced by the binary, not by SKILL.md prose.
//!
//! Two failure modes of the LLM verifier stage are made mechanical here so they
//! cannot drift out of the skill:
//!
//! 1. **Shared blind spot** (`resolve_verifier_model`): the verifier model must
//!    never equal the worker model. When fugu-router is absent both sides used to
//!    fall back to the same tier (sonnet), so generation and verification shared
//!    the same blind spots. The resolver guarantees a distinct, independent tier.
//!
//! 2. **Behavioral criteria never skip the verifier** (`classify_criteria`):
//!    only *purely mechanical* done_criteria (a runnable check with no judgement
//!    words) may bypass the LLM verifier. For behavioral criteria a passing test
//!    is only *evidence handed to* the verifier, never a substitute for it. When
//!    classification is ambiguous we fail toward RUNNING the verifier (safe side).

/// How many trailing lines of raw output to retain in [`FailureDigest::output_tail`].
const OUTPUT_TAIL_LINES: usize = 20;

/// A deterministic, structured distillation of a failing command's raw output.
///
/// The verifier→worker reflux used to carry only a boolean plus an undistilled
/// output blob. `FailureDigest` extracts the *why-it-failed* signal — failing
/// test names, assertion evidence, and a bounded output tail — so a worker (or
/// the /condukt skill's retry prompt) can self-correct in the same run. The
/// FORMATTING here is deterministic Rust; only the fix DECISION is the LLM's job.
///
/// The `condukt verify digest` subcommand exposes [`distill_failure`] so the
/// skill can distill ANY worker/verifier raw output into the retry reflux prompt.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FailureDigest {
    /// Names of failing tests (deduplicated, first-seen order).
    pub failing_tests: Vec<String>,
    /// Assertion / panic evidence lines (trimmed short strings).
    pub assertion_diffs: Vec<String>,
    /// The last [`OUTPUT_TAIL_LINES`] lines of the raw output, joined by `\n`.
    pub output_tail: String,
}

/// Distill a failing command's raw output into a [`FailureDigest`].
///
/// Pure and deterministic: no LLM, no network, no filesystem, no clock. Handles
/// empty / garbage / non-cargo input gracefully (empty vecs, whatever tail
/// exists) and never panics.
///
/// - `failing_tests`: names from cargo-test `test <name> ... FAILED` lines and
///   from the indented `failures:` summary block. Deduplicated, first-seen order.
/// - `assertion_diffs`: `assertion \`...\` failed` lines, following `left:` /
///   `right:` lines, and `panicked at ...` / `thread '...' panicked` lines.
/// - `output_tail`: the last [`OUTPUT_TAIL_LINES`] lines (or all, if shorter).
pub fn distill_failure(raw_output: &str) -> FailureDigest {
    let mut failing_tests: Vec<String> = Vec::new();
    let mut assertion_diffs: Vec<String> = Vec::new();

    let push_unique = |v: &mut Vec<String>, s: String| {
        if !s.is_empty() && !v.contains(&s) {
            v.push(s);
        }
    };

    // First pass: `test <name> ... FAILED` result lines.
    for line in raw_output.lines() {
        let t = line.trim();
        if let Some(name) = parse_test_result_failed(t) {
            push_unique(&mut failing_tests, name);
        }
    }

    // Second pass: the `failures:` summary block lists each failing test name on
    // its own indented line, terminated by a blank line or a `test result:` line.
    let mut in_failures_block = false;
    for line in raw_output.lines() {
        let t = line.trim();
        if t == "failures:" {
            in_failures_block = true;
            continue;
        }
        if in_failures_block {
            // The block ends at a blank line, a `test result:` summary, or the
            // start of the error-detail sub-listing that cargo repeats.
            if t.is_empty() || t.starts_with("test result:") {
                in_failures_block = false;
                continue;
            }
            // Names in the summary are bare identifiers (e.g. `foo::bar`); ignore
            // any lines that look like prose/evidence rather than a test path.
            if is_test_name_line(t) {
                push_unique(&mut failing_tests, t.to_string());
            }
        }
    }

    // Assertion / panic evidence: the "why" beyond the boolean.
    for line in raw_output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let is_assertion = t.contains("assertion") && t.contains("failed");
        let is_left = t.starts_with("left:");
        let is_right = t.starts_with("right:");
        let is_panic =
            t.starts_with("panicked at") || (t.starts_with("thread '") && t.contains("panicked"));
        if is_assertion || is_left || is_right || is_panic {
            push_unique(&mut assertion_diffs, t.to_string());
        }
    }

    let output_tail = tail_lines(raw_output, OUTPUT_TAIL_LINES);

    FailureDigest {
        failing_tests,
        assertion_diffs,
        output_tail,
    }
}

/// Parse a cargo-test result line `test <name> ... FAILED`, returning `<name>`.
/// Tolerates a leading log prefix by matching on the `test ` token boundary and
/// the trailing ` ... FAILED`. Returns `None` for non-matching lines.
fn parse_test_result_failed(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("test ")?;
    // Must end in the FAILED result marker (cargo emits `... FAILED`).
    if !rest.ends_with("FAILED") {
        return None;
    }
    let name_part = rest.split(" ... ").next()?.trim();
    if name_part.is_empty() || name_part == "result:" {
        return None;
    }
    Some(name_part.to_string())
}

/// Heuristic: does a `failures:`-block line look like a bare test-name path
/// (e.g. `foo::bar`) rather than prose or evidence?
fn is_test_name_line(t: &str) -> bool {
    !t.is_empty()
        && !t.contains(' ')
        && t.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '-')
}

/// Return the last `n` lines of `s` joined by `\n`. If `s` has `n` or fewer
/// lines, all of them are returned. Empty input yields an empty string.
fn tail_lines(s: &str, n: usize) -> String {
    if s.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Known model tiers, cheapest → strongest.
const TIERS: [&str; 3] = ["haiku", "sonnet", "opus"];

/// Collapse a model string to its canonical tier keyword when recognised
/// (e.g. `"claude-sonnet-4"` → `"sonnet"`), else the trimmed lowercase string.
/// Two models are "the same model" iff their canonical forms are equal.
fn canonical(model: &str) -> String {
    let m = model.trim().to_lowercase();
    for t in TIERS {
        if m.contains(t) {
            return t.to_string();
        }
    }
    m
}

/// Position of a model within [`TIERS`] (by canonical tier), if recognised.
fn tier_index(model: &str) -> Option<usize> {
    let c = canonical(model);
    TIERS.iter().position(|t| *t == c)
}

/// True iff `a` and `b` denote the same model (so using both for worker and
/// verifier would share a blind spot).
pub fn same_model(a: &str, b: &str) -> bool {
    canonical(a) == canonical(b)
}

/// Resolve the verifier model, guaranteeing it differs from the worker model.
///
/// `suggested` is the `verifier_model` from `route.json` (may be absent/empty).
/// Invariant: the returned model is never the same model as `worker`.
///
/// - A distinct, non-empty `suggested` is honoured as-is.
/// - Otherwise a distinct tier is chosen: prefer a *stronger* verifier than the
///   worker (independent, higher-signal check); if the worker is already at the
///   top tier, step down one tier so the verifier is still independent.
/// - An unrecognised worker model defaults the verifier to the strongest tier.
pub fn resolve_verifier_model(worker: &str, suggested: Option<&str>) -> String {
    if let Some(s) = suggested {
        let s = s.trim();
        if !s.is_empty() && !same_model(s, worker) {
            return s.to_string();
        }
    }
    match tier_index(worker) {
        Some(i) if i + 1 < TIERS.len() => TIERS[i + 1].to_string(),
        Some(i) if i > 0 => TIERS[i - 1].to_string(),
        // haiku with no stronger-but-that-branch-taken (i==0 handled above) or
        // an unrecognised model → strongest independent tier.
        _ => "opus".to_string(),
    }
}

/// Markers that mean the criteria demands judgement about implementation /
/// logic / design / behaviour / correctness. Their presence forces the LLM
/// verifier to run even if an accompanying command exits 0. Bilingual because
/// the skill and its decompositions mix Japanese and English.
const BEHAVIORAL_MARKERS: &[&str] = &[
    // English
    "implement",
    "logic",
    "design",
    "behavior",
    "behaviour",
    "correct",
    "refactor",
    "handle",
    "ensure",
    "semantic",
    "invariant",
    "properly",
    "prove",
    "prevent",
    "enforce",
    // Japanese (SKILL.md wording: 実装/ロジック/設計/コード/振る舞い …)
    "実装",
    "ロジック",
    "設計",
    "コード",
    "振る舞い",
    "挙動",
    "正しく",
    "妥当",
    "検証",
];

/// True iff `done_criteria` carries any behavioral marker — i.e. it asks about
/// *what the code does*, not merely an observable mechanical fact.
pub fn criteria_is_behavioral(done_criteria: &str) -> bool {
    let lower = done_criteria.to_lowercase();
    BEHAVIORAL_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()))
}

/// Classification of a done_criteria for the verifier-skip decision.
#[derive(Debug, Clone)]
pub struct Classification {
    /// The criteria carries behavioral markers (judgement required).
    pub behavioral: bool,
    /// A runnable mechanical check derived from the criteria, if any.
    pub mechanical_cmd: Option<Vec<String>>,
    /// The LLM verifier may be skipped ONLY when this is true: a mechanical
    /// command exists AND the criteria carries no behavioral markers. Any
    /// ambiguity resolves to `false` (run the verifier — the safe side).
    pub skip_eligible: bool,
}

/// Classify a done_criteria: behavioral vs purely mechanical, and whether the
/// verifier may be skipped. Behavioral criteria are never skip-eligible even
/// when they embed a runnable command.
pub fn classify_criteria(done_criteria: &str) -> Classification {
    let behavioral = criteria_is_behavioral(done_criteria);
    let mechanical_cmd = mechanical_cmd(done_criteria);
    let skip_eligible = !behavioral && mechanical_cmd.is_some();
    Classification {
        behavioral,
        mechanical_cmd,
        skip_eligible,
    }
}

/// Build the verify-gate verdict for the purely-mechanical branch.
///
/// [`classify_criteria`] sets `skip_eligible` only when `mechanical_cmd.is_some()`,
/// so a `skip_eligible` classification with no command is supposed to be impossible.
/// If that invariant is ever violated (schema drift, external JSON, a future
/// refactor) we must NOT panic in an unattended run — a panic there breaks the turn.
/// Instead we fail soft: emit a verdict that refuses to skip the verifier, since
/// running the verifier is the safe side.
///
/// `run` runs the mechanical command, returning `(passed, output)`; it is only
/// invoked when a command actually exists.
///
/// Returns `(verdict_json, gate_failed)`. `gate_failed` is true only when a real
/// mechanical command was run and failed (the caller then fails this gate). A
/// missing command never fails the gate — the verifier still runs.
pub fn mechanical_skip_verdict(
    cls: &Classification,
    run: impl FnOnce(&[String]) -> (bool, String),
) -> (serde_json::Value, bool) {
    let Some(cmd) = cls.mechanical_cmd.as_ref() else {
        // Invariant-violating input: skip_eligible with no command. Fail soft —
        // refuse to skip the verifier rather than panicking an unattended run.
        let out = serde_json::json!({
            "mechanical": true,
            "behavioral": cls.behavioral,
            "passed": false,
            "skip_verifier": false,
            "reason": "skip_eligible classification carried no mechanical command; \
                       refusing to skip the verifier (safe side)",
        });
        return (out, false);
    };
    let (passed, output) = run(cmd);
    let mut out = serde_json::json!({
        "mechanical": true,
        "behavioral": false,
        "passed": passed,
        "skip_verifier": passed,
        "cmd": cmd,
        "output": output,
    });
    // On failure, attach the deterministic structured digest alongside the raw
    // output so the verifier→worker reflux carries the *why*, not just a boolean.
    // The passing-case shape is left unchanged.
    if !passed {
        if let Some(obj) = out.as_object_mut() {
            obj.insert(
                "failure_digest".to_string(),
                serde_json::to_value(distill_failure(&output)).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    (out, !passed)
}

/// Extract a runnable command from a done_criteria string for mechanical gate
/// checking. Returns `None` when no mechanical check can be derived (the LLM
/// verifier is then required). This is intentionally about *runnability* only;
/// [`classify_criteria`] layers the behavioral veto on top.
pub fn mechanical_cmd(done_criteria: &str) -> Option<Vec<String>> {
    // Prefer an explicit backtick command: `cargo test -p condukt`
    if let Ok(re) = regex::Regex::new(r"`([^`]+)`") {
        for caps in re.captures_iter(done_criteria) {
            if let Some(inner) = caps.get(1) {
                let argv: Vec<String> = inner
                    .as_str()
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                if argv.first().is_some_and(|p| is_criteria_runner(p)) {
                    return Some(argv);
                }
            }
        }
    }
    // Fall back to recognised test-runner prose.
    let lower = done_criteria.to_lowercase();
    if lower.contains("cargo test") {
        let mut cmd = vec!["cargo".to_string(), "test".to_string()];
        if let Ok(re2) = regex::Regex::new(r"-p\s+([A-Za-z0-9_-]+)") {
            if let Some(c) = re2.captures(done_criteria).and_then(|c| c.get(1)) {
                cmd.push("-p".to_string());
                cmd.push(c.as_str().to_string());
            }
        }
        return Some(cmd);
    }
    if lower.contains("npm test") {
        return Some(vec!["npm".to_string(), "test".to_string()]);
    }
    if lower.contains("pytest") {
        return Some(vec!["pytest".to_string()]);
    }
    if lower.contains("go test") {
        return Some(vec!["go".to_string(), "test".to_string()]);
    }
    None
}

fn is_criteria_runner(tok: &str) -> bool {
    matches!(
        tok,
        "cargo"
            | "npm"
            | "npx"
            | "pytest"
            | "go"
            | "make"
            | "bash"
            | "sh"
            | "python"
            | "python3"
            | "node"
            | "yarn"
            | "pnpm"
            | "just"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Invariant 1: verifier model never equals worker model ──────────────

    /// Across every worker tier, and whether the suggested verifier is absent,
    /// empty, or identical to the worker, the resolved verifier must differ.
    #[test]
    fn verifier_model_never_equals_worker() {
        let workers = [
            "haiku",
            "sonnet",
            "opus",
            "claude-sonnet-4",
            "mystery-model",
        ];
        for w in workers {
            let canon = canonical(w);
            // No suggestion, empty/blank suggestion, or a suggestion that is the
            // same model (exact string or canonical tier) must all still yield a
            // verifier that differs from the worker.
            let suggestions: [Option<&str>; 5] =
                [None, Some(""), Some("  "), Some(w), Some(canon.as_str())];
            for s in suggestions {
                let v = resolve_verifier_model(w, s);
                assert!(
                    !same_model(&v, w),
                    "verifier {v:?} must differ from worker {w:?} (suggested={s:?})"
                );
            }
        }
    }

    /// A distinct, explicit suggestion is honoured verbatim.
    #[test]
    fn distinct_suggestion_is_honoured() {
        assert_eq!(resolve_verifier_model("sonnet", Some("opus")), "opus");
        assert_eq!(resolve_verifier_model("opus", Some("haiku")), "haiku");
    }

    /// The fallback prefers a stronger tier, or steps down from the top tier.
    #[test]
    fn fallback_picks_distinct_tier() {
        assert_eq!(resolve_verifier_model("haiku", None), "sonnet");
        assert_eq!(resolve_verifier_model("sonnet", None), "opus");
        // Worker already at the top → step down to stay independent.
        assert_eq!(resolve_verifier_model("opus", None), "sonnet");
        // Unknown worker → strongest independent tier.
        assert_eq!(resolve_verifier_model("weird", None), "opus");
    }

    // ── Invariant 2: behavioral criteria never skip the verifier ───────────

    /// A behavioral criteria that ALSO embeds a passing test command must NOT
    /// be skip-eligible: the passing test is evidence, not a substitute.
    #[test]
    fn behavioral_criteria_never_skips_verifier() {
        let dc = "Implement the retry logic correctly; `cargo test -p condukt` passes";
        let c = classify_criteria(dc);
        assert!(c.behavioral, "criteria must be classified behavioral");
        assert!(
            c.mechanical_cmd.is_some(),
            "the embedded command is still extracted (as evidence)"
        );
        assert!(
            !c.skip_eligible,
            "behavioral criteria must NEVER be skip-eligible even with a passing test"
        );
    }

    /// A purely mechanical criteria (observable fact, no judgement words) may
    /// skip the verifier.
    #[test]
    fn purely_mechanical_criteria_is_skip_eligible() {
        let c = classify_criteria("`cargo test -p condukt` exits 0");
        assert!(!c.behavioral);
        assert_eq!(
            c.mechanical_cmd.as_deref(),
            Some(&["cargo", "test", "-p", "condukt"].map(String::from)[..])
        );
        assert!(c.skip_eligible, "a plain passing-test criteria may skip");
    }

    /// No runnable command → not skip-eligible (verifier must run).
    #[test]
    fn non_runnable_criteria_is_not_skip_eligible() {
        let c = classify_criteria("the README documents the new flag");
        assert!(c.mechanical_cmd.is_none());
        assert!(!c.skip_eligible);
    }

    // ── Fail-soft: invariant-violating skip_eligible must not panic ────────

    /// A `skip_eligible` classification whose `mechanical_cmd` is `None` violates
    /// the classifier invariant (e.g. from schema drift / external JSON). The
    /// verdict builder must NOT panic; it must refuse to skip the verifier
    /// (`skip_verifier == false`), carry a `reason`, and not fail the gate.
    #[test]
    fn skip_eligible_without_command_fails_soft() {
        let cls = Classification {
            behavioral: false,
            mechanical_cmd: None,
            skip_eligible: true,
        };
        // The runner must never be called when there is no command.
        let (verdict, gate_failed) = mechanical_skip_verdict(&cls, |_cmd| {
            panic!("runner must not be invoked when there is no mechanical command");
        });
        assert_eq!(
            verdict["skip_verifier"],
            serde_json::json!(false),
            "invariant-violating input must NOT skip the verifier (safe side)"
        );
        assert!(
            verdict.get("reason").and_then(|r| r.as_str()).is_some(),
            "the verdict must carry a machine-readable reason: {verdict}"
        );
        assert!(
            !gate_failed,
            "a missing command must not fail the gate — the verifier still runs"
        );
    }

    /// Valid case: `skip_eligible` with a real command runs it; `skip_verifier`
    /// tracks the command result and a failing command fails the gate.
    #[test]
    fn skip_eligible_with_command_runs_and_tracks_result() {
        let cls = Classification {
            behavioral: false,
            mechanical_cmd: Some(vec!["cargo".to_string(), "test".to_string()]),
            skip_eligible: true,
        };
        // Passing command → skip the verifier, gate not failed.
        let (v_pass, failed_pass) =
            mechanical_skip_verdict(&cls, |cmd| (true, format!("ran {cmd:?}")));
        assert_eq!(v_pass["skip_verifier"], serde_json::json!(true));
        assert_eq!(v_pass["passed"], serde_json::json!(true));
        assert_eq!(v_pass["cmd"], serde_json::json!(["cargo", "test"]));
        assert!(!failed_pass);

        // Failing command → do not skip, gate fails.
        let (v_fail, failed_fail) = mechanical_skip_verdict(&cls, |_cmd| (false, "boom".into()));
        assert_eq!(v_fail["skip_verifier"], serde_json::json!(false));
        assert_eq!(v_fail["passed"], serde_json::json!(false));
        assert!(failed_fail, "a failing mechanical check must fail the gate");
    }

    // ── Structured failure digest (verifier→worker reflux) ────────────────

    /// A representative failing cargo-test output must yield detail BEYOND the
    /// pass/fail boolean: the failing test name and the assertion evidence. A
    /// neutered `distill_failure` returning an empty digest would fail this.
    #[test]
    fn distill_surfaces_test_name_and_assertion_diff() {
        let raw = "\
running 2 tests
test foo::bar ... FAILED
test foo::baz ... ok

failures:

---- foo::bar stdout ----
thread 'foo::bar' panicked at src/lib.rs:42:5:
assertion `left == right` failed
  left: 3
 right: 4

failures:
    foo::bar

test result: FAILED. 1 passed; 1 failed; 0 ignored";
        let d = distill_failure(raw);
        // (a) the failing test name is surfaced.
        assert!(
            d.failing_tests.iter().any(|t| t == "foo::bar"),
            "expected failing test 'foo::bar' in {:?}",
            d.failing_tests
        );
        // (b) assertion evidence is surfaced (the "why" beyond the boolean).
        assert!(
            d.assertion_diffs
                .iter()
                .any(|a| a.contains("assertion `left == right` failed")),
            "expected the assertion line in {:?}",
            d.assertion_diffs
        );
        // left/right evidence is captured too.
        assert!(
            d.assertion_diffs.iter().any(|a| a.starts_with("left:")),
            "expected the left value line in {:?}",
            d.assertion_diffs
        );
        assert!(
            d.assertion_diffs.iter().any(|a| a.starts_with("right:")),
            "expected the right value line in {:?}",
            d.assertion_diffs
        );
        // The panic location is captured as evidence.
        assert!(
            d.assertion_diffs.iter().any(|a| a.contains("panicked at")),
            "expected the panic line in {:?}",
            d.assertion_diffs
        );
        // The tail retains the last lines of output.
        assert!(
            d.output_tail.contains("test result: FAILED"),
            "output_tail must retain the trailing summary: {:?}",
            d.output_tail
        );
        // Names are deduplicated: 'foo::bar' appears in both the result line and
        // the summary block, but must be listed once.
        assert_eq!(
            d.failing_tests.iter().filter(|t| *t == "foo::bar").count(),
            1,
            "failing test names must be deduplicated: {:?}",
            d.failing_tests
        );
    }

    /// Empty input must not panic and must yield empty vecs + empty tail.
    #[test]
    fn distill_empty_input_is_graceful() {
        let d = distill_failure("");
        assert!(d.failing_tests.is_empty());
        assert!(d.assertion_diffs.is_empty());
        assert_eq!(d.output_tail, "");
    }

    /// Garbage / non-cargo input must not panic and yields no false positives,
    /// but still keeps the tail.
    #[test]
    fn distill_garbage_input_is_graceful() {
        let raw = "some unrelated log line\nanother line without markers";
        let d = distill_failure(raw);
        assert!(d.failing_tests.is_empty());
        assert!(d.assertion_diffs.is_empty());
        assert_eq!(d.output_tail, raw);
    }

    /// The mechanical verdict embeds the digest on failure and omits it on pass.
    #[test]
    fn mechanical_verdict_embeds_digest_only_on_failure() {
        let cls = Classification {
            behavioral: false,
            mechanical_cmd: Some(vec!["cargo".to_string(), "test".to_string()]),
            skip_eligible: true,
        };
        // Passing case: no failure_digest field (shape unchanged).
        let (v_pass, _) = mechanical_skip_verdict(&cls, |_c| (true, "all good".into()));
        assert!(
            v_pass.get("failure_digest").is_none(),
            "passing verdict must not carry a failure_digest: {v_pass}"
        );
        // Failing case: failure_digest present and populated.
        let (v_fail, failed) =
            mechanical_skip_verdict(&cls, |_c| (false, "test foo::bar ... FAILED".into()));
        assert!(failed);
        let digest = v_fail
            .get("failure_digest")
            .expect("failing verdict must carry a failure_digest");
        assert_eq!(
            digest["failing_tests"],
            serde_json::json!(["foo::bar"]),
            "digest must surface the failing test: {v_fail}"
        );
        // The raw output is still present alongside the digest.
        assert!(v_fail.get("output").is_some());
    }

    /// Japanese behavioral markers are recognised too.
    #[test]
    fn japanese_behavioral_marker_blocks_skip() {
        let dc = "リトライの振る舞いを実装する。`cargo test -p condukt` が通ること";
        let c = classify_criteria(dc);
        assert!(c.behavioral);
        assert!(!c.skip_eligible);
    }
}
