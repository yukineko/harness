//! Terminal PR-exit step — the phase-2 "external-loop" close: open a PR via the
//! `gh` CLI, but keep push/PR BEHIND the existing GATED human approval.
//!
//! Design invariants preserved here:
//!
//! 1. **Subscription-native, no external API key**: PR creation uses the `gh`
//!    CLI's own auth (`gh auth status`). We never take an API token.
//!
//! 2. **GATED even in autonomy mode**: opening a PR is a deploy/push-class action.
//!    The actual `gh pr create` runs ONLY when the caller passes `--execute`, which
//!    the /condukt skill supplies ONLY after the human GATED approval. An
//!    unattended/autonomous run therefore stops at a dry-run ([`PrOutcome::Prepared`])
//!    and never opens a PR on its own.
//!
//! 3. **Never break a turn / fail-soft**: when `gh` is absent or unauthenticated
//!    we degrade to local-commit-only ([`PrOutcome::DegradedLocalOnly`]) and exit 0.
//!    None of the functions here panic, unwrap on external input, or call
//!    `Command` directly — the command runner is INJECTED so they stay pure and
//!    unit-testable without `gh` installed.

/// Whether the `gh` CLI is present on PATH and authenticated.
///
/// `present == false` means the binary could not be spawned at all (not on PATH);
/// in that case `authed` is always `false`.
#[derive(Debug, Clone, PartialEq)]
pub struct GhStatus {
    /// The `gh` binary is on PATH (a `gh --version` spawn succeeded).
    pub present: bool,
    /// `gh auth status` reported success (an authenticated login exists).
    pub authed: bool,
}

/// Detect `gh` presence and auth via an injected command runner.
///
/// `run(argv)` returns:
/// - `Some((success, output))` — the process spawned; `success` is its exit-0 status.
/// - `None` — the binary could not be spawned (gh absent).
///
/// Presence is probed with `gh --version` (`None` ⇒ `present = false`); auth with
/// `gh auth status` (`success == true` ⇒ `authed = true`). When gh is absent,
/// auth is not probed and `authed` is `false`. Pure: no IO, no clock, no panic.
pub fn detect_gh<R: Fn(&[&str]) -> Option<(bool, String)>>(run: R) -> GhStatus {
    let present = run(&["--version"]).is_some();
    if !present {
        return GhStatus {
            present: false,
            authed: false,
        };
    }
    let authed = matches!(run(&["auth", "status"]), Some((true, _)));
    GhStatus {
        present: true,
        authed,
    }
}

/// The deterministic inputs to `gh pr create`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PrPlan {
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
}

/// Deterministically format the argv for `gh pr create`. Pure, no side effects.
///
/// Shape: `["pr","create","--title",<t>,"--body",<b>,"--head",<h>,"--base",<base>]`.
pub fn build_pr_args(plan: &PrPlan) -> Vec<String> {
    vec![
        "pr".to_string(),
        "create".to_string(),
        "--title".to_string(),
        plan.title.clone(),
        "--body".to_string(),
        plan.body.clone(),
        "--head".to_string(),
        plan.head.clone(),
        "--base".to_string(),
        plan.base.clone(),
    ]
}

/// The fail-soft outcome of the PR-exit step. Serialized with an `outcome` tag so
/// the skill can branch on it deterministically.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "outcome")]
pub enum PrOutcome {
    /// gh executed `pr create` and returned the new PR URL.
    Created { url: String },
    /// gh is usable but `--execute` was not passed: the GATED dry-run shows the
    /// exact argv that WOULD run, without running it.
    Prepared { args: Vec<String> },
    /// Fail-soft: gh absent or unauthenticated (or its execution failed) — the
    /// work is left as local commits and the turn is not broken.
    DegradedLocalOnly { reason: String },
}

/// The pure, fail-soft PR decision. Never returns an error, never panics.
///
/// - gh absent            ⇒ [`PrOutcome::DegradedLocalOnly`] (local commits only).
/// - gh present, unauthed ⇒ [`PrOutcome::DegradedLocalOnly`] (login required).
/// - gh usable, `!execute`⇒ [`PrOutcome::Prepared`] (the GATED dry-run — shows the
///   argv but does NOT run gh).
/// - gh usable, `execute` ⇒ [`PrOutcome::Prepared`] as well: `decide_pr` never
///   runs gh itself. The caller (the subcommand) runs gh with these args and
///   builds [`PrOutcome::Created`] from gh's stdout, degrading soft on failure.
pub fn decide_pr(status: &GhStatus, plan: &PrPlan, execute: bool) -> PrOutcome {
    if !status.present {
        return PrOutcome::DegradedLocalOnly {
            reason: "gh CLI not found; left work as local commits".to_string(),
        };
    }
    if !status.authed {
        return PrOutcome::DegradedLocalOnly {
            reason: "gh not authenticated (gh auth login); left work as local commits".to_string(),
        };
    }
    // Usable. Whether or not `execute` is set, the pure decision yields the
    // prepared argv; the executed path (Created) is built by the caller from
    // gh's real output. When !execute this IS the terminal result (GATED dry-run).
    let _ = execute;
    PrOutcome::Prepared {
        args: build_pr_args(plan),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> PrPlan {
        PrPlan {
            title: "Add PR-exit step".to_string(),
            body: "closes the external loop".to_string(),
            head: "feature/pr-exit".to_string(),
            base: "main".to_string(),
        }
    }

    /// Fail-soft reproduction proof: gh absent (runner always returns None) must
    /// detect present=false,authed=false AND `decide_pr` must degrade to
    /// local-only. A neutered `decide_pr` that panicked or returned
    /// Created/Prepared on the gh-absent path would FAIL here.
    #[test]
    fn gh_absent_degrades_to_local_only() {
        let status = detect_gh(|_| None);
        assert_eq!(
            status,
            GhStatus {
                present: false,
                authed: false
            },
            "gh-absent runner must yield present=false, authed=false"
        );

        // Even with execute=true, an absent gh must never open a PR — it degrades.
        let outcome = decide_pr(&status, &plan(), true);
        match outcome {
            PrOutcome::DegradedLocalOnly { reason } => {
                assert!(
                    reason.contains("not found"),
                    "reason must explain gh is absent: {reason:?}"
                );
            }
            other => panic!("gh-absent must degrade to local-only, got {other:?}"),
        }
    }

    /// gh present but `gh auth status` fails ⇒ authed=false ⇒ DegradedLocalOnly.
    #[test]
    fn gh_present_but_unauthed_degrades() {
        // --version succeeds (present), auth status fails (not logged in).
        let status = detect_gh(|argv| match argv {
            ["--version"] => Some((true, "gh version 2.0.0".to_string())),
            ["auth", "status"] => Some((false, "not logged in".to_string())),
            _ => None,
        });
        assert_eq!(
            status,
            GhStatus {
                present: true,
                authed: false
            }
        );

        let outcome = decide_pr(&status, &plan(), false);
        match outcome {
            PrOutcome::DegradedLocalOnly { reason } => {
                assert!(
                    reason.contains("authenticated"),
                    "reason must explain gh is unauthenticated: {reason:?}"
                );
            }
            other => panic!("unauthed gh must degrade to local-only, got {other:?}"),
        }
    }

    /// GATED dry-run: gh usable but execute=false ⇒ Prepared{args}, and the args
    /// carry the title/head/base. This proves the dry-run does NOT run gh.
    #[test]
    fn gh_usable_without_execute_prepares_not_runs() {
        let status = GhStatus {
            present: true,
            authed: true,
        };
        let p = plan();
        let outcome = decide_pr(&status, &p, false);
        match outcome {
            PrOutcome::Prepared { args } => {
                assert!(
                    args.contains(&p.title),
                    "args must carry the title: {args:?}"
                );
                assert!(args.contains(&p.head), "args must carry the head: {args:?}");
                assert!(args.contains(&p.base), "args must carry the base: {args:?}");
            }
            other => panic!("usable gh without --execute must Prepare, got {other:?}"),
        }
    }

    /// The argv shape is exact and deterministic.
    #[test]
    fn build_pr_args_is_deterministic() {
        let args = build_pr_args(&plan());
        assert_eq!(
            args,
            vec![
                "pr".to_string(),
                "create".to_string(),
                "--title".to_string(),
                "Add PR-exit step".to_string(),
                "--body".to_string(),
                "closes the external loop".to_string(),
                "--head".to_string(),
                "feature/pr-exit".to_string(),
                "--base".to_string(),
                "main".to_string(),
            ]
        );
    }
}
