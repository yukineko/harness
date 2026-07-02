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

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

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

/// A deterministic, structured distillation of a target's *runtime* signals.
///
/// The phase-2 companion [`FailureDigest`] distills a failing command's *test*
/// output (failing test names, assertion evidence). `RuntimeDigest` is the
/// phase-3 counterpart: it distills the signals from actually *running* the
/// target — its exit code, any panic/exception evidence, and bounded tails of
/// both output streams — so the verifier→worker reflux carries the runtime
/// *why-it-broke*, not just a boolean. The FORMATTING here is deterministic
/// Rust; only the fix DECISION is the LLM's job.
///
/// Exposed by the `condukt verify runtime` subcommand (symmetric to `verify
/// digest`) and embedded into the verifier→worker reflux verdict on a runtime
/// failure by [`runtime_reflux_verdict`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeDigest {
    /// The process exit code, or `None` when unknown (e.g. signal termination).
    pub exit_code: Option<i32>,
    /// Panic / exception evidence lines (deduplicated, first-seen order, stderr
    /// preferred). Matches `panicked at`, `thread '...' panicked`, `Exception`,
    /// `Traceback`, and `Error:` markers.
    pub panics: Vec<String>,
    /// The last [`OUTPUT_TAIL_LINES`] lines of stderr, joined by `\n`.
    pub stderr_tail: String,
    /// The last [`OUTPUT_TAIL_LINES`] lines of stdout, joined by `\n`.
    pub stdout_tail: String,
}

/// Distill a target's runtime output into a [`RuntimeDigest`].
///
/// Pure and deterministic: no LLM, no network, no filesystem, no clock. Handles
/// empty / garbage / non-UTF8-ish input gracefully (empty vecs, whatever tail
/// exists) and never panics.
///
/// - `exit_code`: threaded through verbatim (`None` when the caller could not
///   determine one, e.g. signal termination).
/// - `panics`: panic/exception evidence lines gathered from BOTH streams, with
///   stderr scanned first so its lines win first-seen order. Deduplicated via
///   the same policy as [`distill_failure`]. Markers: `panicked at`,
///   `thread '...' panicked`, `Exception`, `Traceback`, `Error:`.
/// - `stderr_tail` / `stdout_tail`: the last [`OUTPUT_TAIL_LINES`] lines of each
///   stream (or all, if shorter).
pub fn distill_runtime(stdout: &str, stderr: &str, exit_code: Option<i32>) -> RuntimeDigest {
    let mut panics: Vec<String> = Vec::new();

    let push_unique = |v: &mut Vec<String>, s: String| {
        if !s.is_empty() && !v.contains(&s) {
            v.push(s);
        }
    };

    // Scan stderr first (preferred first-seen), then stdout, for panic/exception
    // evidence. A line qualifies if it carries any recognised runtime marker.
    for stream in [stderr, stdout] {
        for line in stream.lines() {
            let t = line.trim();
            if is_panic_evidence(t) {
                push_unique(&mut panics, t.to_string());
            }
        }
    }

    RuntimeDigest {
        exit_code,
        panics,
        stderr_tail: tail_lines(stderr, OUTPUT_TAIL_LINES),
        stdout_tail: tail_lines(stdout, OUTPUT_TAIL_LINES),
    }
}

/// Build the verifier→worker reflux verdict for a target's *runtime* signals.
///
/// The phase-2 companion [`mechanical_skip_verdict`] embeds a [`FailureDigest`]
/// under `"failure_digest"` when a mechanical *test* check fails; this is the
/// phase-3 counterpart for a *runtime* failure. It is pure and deterministic:
///
/// - it decides pass/fail from the mechanical facts alone — a runtime failure is
///   a non-zero exit code (`Some(c)` with `c != 0`) OR any panic/exception
///   evidence in [`RuntimeDigest::panics`];
/// - on failure it embeds the structured [`RuntimeDigest`] under `"runtime_digest"`
///   so the reflux carries the runtime *why* (exit code, panic lines, and the
///   stderr/stdout tails), not merely the boolean; the passing shape omits it.
///
/// The FORMATTING here is deterministic Rust; the verdict states only observable
/// facts and carries NO fix decision — how to fix stays with the LLM worker.
pub fn runtime_reflux_verdict(
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> serde_json::Value {
    let digest = distill_runtime(stdout, stderr, exit_code);
    // Mechanical failure predicate: a non-zero exit OR any panic/exception line.
    let nonzero_exit = digest.exit_code.is_some_and(|c| c != 0);
    let runtime_failed = nonzero_exit || !digest.panics.is_empty();
    let mut out = serde_json::json!({
        "kind": "runtime",
        "passed": !runtime_failed,
    });
    // Attach the deterministic structured digest ONLY on failure, mirroring the
    // `failure_digest` embedding: the passing-case shape stays a bare boolean.
    if runtime_failed {
        if let Some(obj) = out.as_object_mut() {
            obj.insert(
                "runtime_digest".to_string(),
                serde_json::to_value(&digest).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    out
}

/// True iff a trimmed line looks like panic / exception evidence from a running
/// process. Language-agnostic: covers Rust panics plus common Python/JVM/other
/// exception markers. Empty lines never qualify.
fn is_panic_evidence(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    t.starts_with("panicked at")
        || (t.starts_with("thread '") && t.contains("panicked"))
        || t.contains("Exception")
        || t.contains("Traceback")
        || t.contains("Error:")
}

/// Launch `cmd` as a real subprocess inside the blastguard-validated envelope,
/// capture its runtime signals, and reflux them through the existing
/// deterministic verdict path. This is the IO-bearing companion to the pure
/// [`runtime_reflux_verdict`]: formatting stays with that one function, so this
/// launcher never re-implements digest shaping.
///
/// The command is run through `sh -c` (no Docker/VM/sandbox — the existing
/// `sh -c` + `wait-timeout` envelope is the whole isolation story) with a
/// bounded timeout.
///
/// **never-break-a-turn**: this function never `panic!`/`unwrap`/`expect`s on an
/// external-input or absent-tool path. Every branch returns a verdict (JSON):
///
/// - **blastguard `Deny`**: the command is refused *fail-closed* and is NEVER
///   spawned — a fail-soft runtime-failure verdict carries the refusal reason in
///   its stderr tail. No shell runs, so a destructive payload cannot execute.
/// - **spawn failure** (missing target / not executable): a fail-soft failure
///   verdict (`exit_code` null, stderr carries the error, `note = "spawn-error"`).
/// - **timeout**: the child is killed; a fail-soft failure verdict (`exit_code`
///   null, `note = "timeout"`).
/// - **normal exit**: stdout/stderr/exit code are refluxed through
///   [`runtime_reflux_verdict`], whose pass/fail predicate decides the verdict.
///
/// The verdict carries only observable facts (pass/fail, the runtime digest, and
/// a mechanical `note` for the fail-soft branches) — never a fix decision. How
/// to fix stays with the LLM worker.
pub fn launch_and_reflux(cmd: &str, timeout_secs: u64) -> serde_json::Value {
    // (a) blastguard gate — validate BEFORE spawning, reusing the same pure
    // detector the PreToolUse hook uses (no reimplementation). A flagged command
    // is refused fail-closed and never reaches the shell.
    let input = serde_json::json!({ "command": cmd });
    if let blastguard::model::Decision::Deny(reason) =
        blastguard::detect::detect("Bash", Some(&input))
    {
        let stderr = format!(
            "[blastguard] launch command `{cmd}` refused before sh -c (fail-closed) — {reason}"
        );
        return fail_soft_launch_verdict("", &stderr, None, "blastguard-denied");
    }

    // (b) spawn via `sh -c`, piping both streams so we can capture them.
    let timeout = timeout_secs.max(1);
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            // Fail-soft: the target could not even be started. No panic.
            let stderr = format!("failed to spawn `{cmd}`: {e}");
            return fail_soft_launch_verdict("", &stderr, None, "spawn-error");
        }
    };

    // (c) wait with a timeout; a timed-out child is killed and reaped.
    match child.wait_timeout(Duration::from_secs(timeout)) {
        Ok(Some(status)) => {
            // (d) normal exit — read both streams and reflux through the pure fn.
            let (stdout, stderr) = read_child_streams(&mut child);
            runtime_reflux_verdict(&stdout, &stderr, status.code())
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = format!("launch of `{cmd}` timed out after {timeout}s and was killed");
            fail_soft_launch_verdict("", &stderr, None, "timeout")
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = format!("failed to wait on `{cmd}`: {e}");
            fail_soft_launch_verdict("", &stderr, None, "wait-error")
        }
    }
}

/// Build a fail-soft launch verdict that is ALWAYS a failure, regardless of the
/// [`runtime_reflux_verdict`] predicate (which keys off exit code / panic
/// markers that the fail-soft branches — deny / spawn-error / timeout — may not
/// carry). It mirrors the runtime verdict shape (`kind` + `passed` + an embedded
/// `runtime_digest`) and adds a mechanical `note` naming the fail-soft cause.
fn fail_soft_launch_verdict(
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    note: &str,
) -> serde_json::Value {
    let digest = distill_runtime(stdout, stderr, exit_code);
    let mut out = serde_json::json!({
        "kind": "runtime",
        "passed": false,
        "note": note,
    });
    if let Some(obj) = out.as_object_mut() {
        obj.insert(
            "runtime_digest".to_string(),
            serde_json::to_value(&digest).unwrap_or(serde_json::Value::Null),
        );
    }
    out
}

/// Read a finished child's piped stdout/stderr into lossy-UTF8 strings. The
/// child has already exited when this is called (so the bounded pipe buffers
/// hold everything), and read errors degrade to whatever was captured — never a
/// panic.
fn read_child_streams(child: &mut std::process::Child) -> (String, String) {
    let mut stdout_buf = Vec::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_end(&mut stdout_buf);
    }
    let mut stderr_buf = Vec::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_end(&mut stderr_buf);
    }
    (
        String::from_utf8_lossy(&stdout_buf).into_owned(),
        String::from_utf8_lossy(&stderr_buf).into_owned(),
    )
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

    // ── Structured runtime digest (phase-3 runtime FB reflux) ─────────────

    /// A representative failing run — non-zero exit, a panic line, and stderr
    /// content — must surface ALL of (a) the exit code, (b) the panic evidence
    /// line, and (c) the stderr tail. A neutered `distill_runtime` that dropped
    /// exit_code / panics / stderr_tail would genuinely FAIL this.
    #[test]
    fn distill_runtime_surfaces_exit_panic_and_stderr() {
        let stdout = "starting up\ndoing work\n";
        let stderr = "\
thread 'main' panicked at src/main.rs:10:5:
index out of bounds: the len is 0 but the index is 3
note: run with `RUST_BACKTRACE=1` for a backtrace";
        let d = distill_runtime(stdout, stderr, Some(101));
        // (a) the exit code is surfaced verbatim.
        assert_eq!(d.exit_code, Some(101), "exit_code must be surfaced");
        // (b) the panic evidence line is surfaced.
        assert!(
            d.panics.iter().any(|p| p.contains("panicked at")),
            "expected the panic line in {:?}",
            d.panics
        );
        // (c) the stderr tail retains the trailing stderr content.
        assert!(
            d.stderr_tail.contains("index out of bounds"),
            "stderr_tail must retain trailing stderr: {:?}",
            d.stderr_tail
        );
        // stdout tail is captured independently of stderr.
        assert!(
            d.stdout_tail.contains("doing work"),
            "stdout_tail must retain trailing stdout: {:?}",
            d.stdout_tail
        );
    }

    /// Panic evidence is gathered from BOTH streams, deduplicated, with stderr
    /// preferred first-seen. A line present in both streams appears once.
    #[test]
    fn distill_runtime_collects_from_both_streams_deduped() {
        let shared = "Traceback (most recent call last):";
        let stdout = format!("stdout noise\n{shared}\nError: boom from stdout");
        let stderr = format!("{shared}\nException: kaboom");
        let d = distill_runtime(&stdout, &stderr, None);
        // The shared line is deduplicated to a single entry.
        assert_eq!(
            d.panics.iter().filter(|p| *p == shared).count(),
            1,
            "shared evidence must be deduplicated: {:?}",
            d.panics
        );
        // stderr is scanned first, so its shared line wins first-seen order.
        assert_eq!(
            d.panics.first().map(String::as_str),
            Some(shared),
            "stderr evidence must be first-seen: {:?}",
            d.panics
        );
        // Evidence from both streams is present.
        assert!(d.panics.iter().any(|p| p.contains("Exception")));
        assert!(d.panics.iter().any(|p| p.contains("Error:")));
        // No exit code was provided.
        assert_eq!(d.exit_code, None);
    }

    /// Empty input must not panic and must yield an empty digest.
    #[test]
    fn distill_runtime_empty_input_is_graceful() {
        let d = distill_runtime("", "", None);
        assert_eq!(d.exit_code, None);
        assert!(d.panics.is_empty());
        assert_eq!(d.stderr_tail, "");
        assert_eq!(d.stdout_tail, "");
    }

    /// Garbage input — a long newline-free string and a dense symbol run — must
    /// not panic and must yield no false-positive panics, but keep the tails.
    #[test]
    fn distill_runtime_garbage_input_is_graceful() {
        let huge = "x".repeat(50_000);
        let symbols = "�\u{0}\t!@#$%^&*()_+{}|:<>?~`-=[]\\;',./\u{1b}[31m";
        let d = distill_runtime(&huge, symbols, Some(-1));
        assert!(
            d.panics.is_empty(),
            "no marker present → no panics: {:?}",
            d.panics
        );
        // Single-line (no `\n`) input is retained whole as the tail.
        assert_eq!(d.stdout_tail, huge);
        assert_eq!(d.stderr_tail, symbols);
        assert_eq!(d.exit_code, Some(-1));
    }

    // ── Runtime reflux verdict (phase-3 verifier→worker reflux) ───────────

    /// RED existence: a runtime failure — non-zero exit + a panic line + stderr
    /// content — must produce a reflux verdict that carries, BEYOND the pass/fail
    /// boolean, the runtime diagnostics: the exit code, the panic evidence line,
    /// and the stderr tail. Neutering the `runtime_digest` embedding (dropping the
    /// `obj.insert`) makes `.expect("runtime_digest")` panic → genuine FAIL; a
    /// `distill_runtime` that dropped exit_code / panics / stderr_tail also FAILs.
    #[test]
    fn runtime_reflux_verdict_embeds_diagnostics_on_failure() {
        let stdout = "booting\n";
        let stderr = "\
thread 'main' panicked at src/main.rs:10:5:
index out of bounds: the len is 0 but the index is 3
note: run with `RUST_BACKTRACE=1` for a backtrace";
        let v = runtime_reflux_verdict(stdout, stderr, Some(101));
        // The verdict states pass/fail, and this run is a failure.
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "non-zero exit + panic must be a runtime failure: {v}"
        );
        // BEYOND the boolean: the structured runtime digest is embedded.
        let d = v
            .get("runtime_digest")
            .expect("a failing runtime verdict must carry a runtime_digest");
        // (a) the exit code is surfaced.
        assert_eq!(
            d["exit_code"],
            serde_json::json!(101),
            "runtime_digest must surface the exit code: {v}"
        );
        // (b) the panic evidence line is surfaced.
        assert!(
            d["panics"].as_array().is_some_and(|a| a
                .iter()
                .any(|p| p.as_str().is_some_and(|s| s.contains("panicked at")))),
            "runtime_digest must surface the panic line: {v}"
        );
        // (c) the stderr tail retains the trailing stderr content.
        assert!(
            d["stderr_tail"]
                .as_str()
                .is_some_and(|s| s.contains("index out of bounds")),
            "runtime_digest must surface the stderr tail: {v}"
        );
    }

    /// A clean run — exit 0, no panics — passes and omits the digest (the passing
    /// shape stays a bare boolean, mirroring the failure_digest omission on pass).
    #[test]
    fn runtime_reflux_verdict_pass_omits_digest() {
        let v = runtime_reflux_verdict("all good\n", "", Some(0));
        assert_eq!(v["passed"], serde_json::json!(true));
        assert!(
            v.get("runtime_digest").is_none(),
            "a passing runtime verdict must not carry a runtime_digest: {v}"
        );
    }

    /// Panic evidence alone marks a failure even when the exit code is 0 (a
    /// process can panic-catch and still exit 0): the reflux must still fail and
    /// embed the digest so the panic reaches the worker.
    #[test]
    fn runtime_reflux_verdict_fails_on_panic_even_with_zero_exit() {
        let v = runtime_reflux_verdict("", "thread 'worker' panicked at lib.rs:1:1", Some(0));
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "a panic must fail the runtime verdict regardless of exit code: {v}"
        );
        assert!(
            v.get("runtime_digest").is_some(),
            "the panic evidence must be embedded for the worker: {v}"
        );
    }

    /// The reflux carries only observable facts — never a fix instruction. This
    /// pins the LLM/Rust separation: no "how to fix" field leaks into the verdict.
    #[test]
    fn runtime_reflux_verdict_carries_no_fix_decision() {
        let v = runtime_reflux_verdict("", "Error: boom", Some(2));
        let obj = v.as_object().expect("verdict is a JSON object");
        // Only the mechanical keys are present; nothing prescribing a fix.
        for k in obj.keys() {
            assert!(
                matches!(k.as_str(), "kind" | "passed" | "runtime_digest"),
                "unexpected key {k:?} — the verdict must stay fact-only (no fix decision): {v}"
            );
        }
    }

    // ── Real process launch + fail-soft (phase-3 DoD#3) ───────────────────

    /// RED existence: a blastguard-flagged command (recursive rm) must be
    /// refused BEFORE `sh -c` runs. The benign leading segment (`touch sentinel`)
    /// must never execute — the surviving-absent sentinel proves the shell was
    /// not invoked. Neuter oracle: removing the blastguard gate lets the shell
    /// run, so the sentinel is created (this test's `!exists` FAILs) and the
    /// benign rm-on-missing exits 0 (`passed` becomes true → FAILs too).
    #[test]
    fn launch_refuses_destructive_command_without_spawning() {
        let tmp = tempfile::tempdir().unwrap();
        let sentinel = tmp.path().join("ran.txt");
        let victim = tmp.path().join("victim");
        let payload = format!("touch {} ; rm -rf {}", sentinel.display(), victim.display());
        let v = launch_and_reflux(&payload, 5);
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "a refused command must not count as passed: {v}"
        );
        assert_eq!(v["note"], serde_json::json!("blastguard-denied"));
        let d = v
            .get("runtime_digest")
            .expect("a refusal must carry a runtime_digest");
        assert!(
            d["stderr_tail"]
                .as_str()
                .is_some_and(|s| s.contains("blastguard")),
            "the refusal reason must name the guard: {v}"
        );
        assert!(
            !sentinel.exists(),
            "sh -c must NOT have run — a created sentinel would prove the payload executed"
        );
    }

    /// RED existence: a benign (blastguard-allowed) command that exits non-zero
    /// and writes stderr must reflux a runtime FAILURE whose digest carries the
    /// diagnostics BEYOND the boolean — the exit code and the stderr tail.
    /// Neuter oracle: dropping the `runtime_digest` embed makes `.expect` panic;
    /// dropping the exit-code reflux makes the `exit_code == 3` assert FAIL.
    #[test]
    fn launch_refluxes_runtime_failure_with_diagnostics() {
        let v = launch_and_reflux("echo boom >&2; exit 3", 5);
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "a non-zero exit is a runtime failure: {v}"
        );
        let d = v
            .get("runtime_digest")
            .expect("a runtime failure must carry a runtime_digest");
        assert_eq!(
            d["exit_code"],
            serde_json::json!(3),
            "the exit code must be refluxed: {v}"
        );
        assert!(
            d["stderr_tail"]
                .as_str()
                .is_some_and(|s| s.contains("boom")),
            "the stderr tail must carry the diagnostic beyond the boolean: {v}"
        );
    }

    /// Fail-soft: an absent / unstartable target must NOT panic — it must return
    /// a runtime-failure verdict. Neuter oracle: an `unwrap`/`?` on the child's
    /// exit path would panic here instead of yielding a verdict.
    #[test]
    fn launch_absent_target_fails_soft_without_panic() {
        let v = launch_and_reflux("this_binary_does_not_exist_zzq --nope", 5);
        assert_eq!(v["kind"], serde_json::json!("runtime"));
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "an unstartable target must fail soft to a failure: {v}"
        );
        assert!(
            v.get("runtime_digest").is_some(),
            "a fail-soft verdict still carries a digest: {v}"
        );
    }

    /// Fail-soft: a long-running command hit with a short timeout must be killed
    /// and reported as a timeout WITHOUT panicking (and the test finishes in ~1s,
    /// not ~5s). Neuter oracle: a plain `child.wait()` (no timeout/kill) would
    /// block for the full sleep and return exit 0, so `passed:false` / the
    /// `note == "timeout"` assert FAILs (and the test no longer finishes fast).
    #[test]
    fn launch_timeout_fails_soft_with_note() {
        let v = launch_and_reflux("sleep 5", 1);
        assert_eq!(
            v["passed"],
            serde_json::json!(false),
            "a timed-out launch must fail soft: {v}"
        );
        assert_eq!(
            v["note"],
            serde_json::json!("timeout"),
            "the timeout note must be set: {v}"
        );
        let d = v
            .get("runtime_digest")
            .expect("a timeout must carry a digest");
        assert_eq!(
            d["exit_code"],
            serde_json::Value::Null,
            "no exit code is known on a timeout: {v}"
        );
    }

    /// A benign command that exits 0 cleanly must pass, and the passing shape
    /// omits the digest (mirroring the pure `runtime_reflux_verdict` pass shape).
    #[test]
    fn launch_benign_command_passes() {
        let v = launch_and_reflux("echo ok", 5);
        assert_eq!(
            v["passed"],
            serde_json::json!(true),
            "a clean exit-0 command must pass: {v}"
        );
        assert!(
            v.get("runtime_digest").is_none(),
            "the passing shape must omit the runtime_digest: {v}"
        );
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
