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

    /// Japanese behavioral markers are recognised too.
    #[test]
    fn japanese_behavioral_marker_blocks_skip() {
        let dc = "リトライの振る舞いを実装する。`cargo test -p condukt` が通ること";
        let c = classify_criteria(dc);
        assert!(c.behavioral);
        assert!(!c.skip_eligible);
    }
}
