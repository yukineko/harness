//! Verification gates over the audit's findings (see DESIGN-VERIFY.md).
//!
//! The audit produces findings; this stage refines them before they reach the
//! human, as a pure transform `Vec<(label, Parsed)> -> Vec<(label, Parsed)>` so
//! the downstream merge/sentinel logic stays unchanged:
//!
//! - **V1 refute** (`[verify].enabled`): for each shard the audit flagged
//!   `needs_user=yes`, an independent skeptic re-derives the findings and drops
//!   only those refuted by a verbatim quote. Uncertain findings are KEPT — the
//!   refutation can only lower false positives, never silently delete real drift
//!   (DESIGN-VERIFY.md §3.3). The skeptic's verdict is appended to the shard's
//!   report body for transparency (§3.4); the post-verify `needs_user` drives the
//!   sentinel.
//! - **V2 completeness** (`[verify].completeness`): a separate agent per area /
//!   invariant shard surfaces verifiable canon rules the sampling audit never
//!   matched (false negatives). Its findings are appended as new `completeness:*`
//!   shards.
//!
//! Both reuse [`agent::run_shards`] (fresh context, bounded parallelism, the same
//! read-only allowlist) and [`parse::parse`] (same marker contract). A verify
//! agent that fails or omits its marker is treated as INCONCLUSIVE: the original
//! findings are kept (fail-safe), never dropped — a broken verifier must not lose
//! an audit's findings.

use crate::agent::{self, ShardPrompt};
use crate::config::Config;
use crate::parse::{self, Parsed};
use crate::prompt::{self, Shard};
use crate::scope::Scope;
use std::collections::HashMap;
use std::path::Path;

/// Apply the enabled verification gates to the parsed shard results. `shards` is
/// index-aligned with `parsed` (both built in the same order from the run).
pub fn apply(
    cfg: &Config,
    repo_root: &Path,
    scope: &Scope,
    shards: &[Shard],
    date: &str,
    parsed: Vec<(String, Parsed)>,
) -> Vec<(String, Parsed)> {
    let mut parsed = if cfg.verify.enabled {
        refute(cfg, repo_root, scope, shards, date, parsed)
    } else {
        parsed
    };
    if cfg.verify.completeness {
        parsed.extend(completeness(cfg, repo_root, scope, shards, date));
    }
    parsed
}

/// V1: re-derive every `needs_user=yes` finding with an independent skeptic and
/// drop only those refuted by a verbatim quote.
fn refute(
    cfg: &Config,
    repo_root: &Path,
    scope: &Scope,
    shards: &[Shard],
    date: &str,
    parsed: Vec<(String, Parsed)>,
) -> Vec<(String, Parsed)> {
    // Build a refute prompt for each flagged shard, remembering its position so
    // the result can be folded back in order.
    let mut prompts: Vec<ShardPrompt> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    for (i, (sh, (label, p))) in shards.iter().zip(parsed.iter()).enumerate() {
        if p.needs_user && p.marker_found {
            prompts.push(ShardPrompt {
                label: format!("refute:{label}"),
                prompt: prompt::render_refute(cfg, scope, *sh, date, &p.report),
            });
            positions.push(i);
        }
    }
    if prompts.is_empty() {
        return parsed;
    }

    let outs = agent::run_shards(&cfg.agent, repo_root, prompts);
    // position -> Some(refute result) when conclusive, None when inconclusive.
    let mut by_pos: HashMap<usize, Option<Parsed>> = HashMap::new();
    for (k, o) in outs.iter().enumerate() {
        let r = parse::parse(&o.out.stdout);
        let conclusive = o.out.code == 0 && r.marker_found;
        by_pos.insert(positions[k], conclusive.then_some(r));
    }

    parsed
        .into_iter()
        .enumerate()
        .map(|(i, (label, audit))| match by_pos.remove(&i) {
            None => (label, audit), // shard was not flagged — untouched
            Some(Some(r)) => (label, fold_refute(&audit, &r)),
            Some(None) => {
                eprintln!(
                    "specguard: WARN 反証 inconclusive for shard '{label}' (agent 失敗/marker 欠落); findings を存置"
                );
                (label, fold_inconclusive(&audit))
            }
        })
        .collect()
}

/// Combine an audit result with its skeptic's verdict: the skeptic's report is
/// appended under a `反証 (verify)` heading, and the post-verify `needs_user` /
/// summary take over (a finding survives only if the skeptic kept it).
fn fold_refute(audit: &Parsed, r: &Parsed) -> Parsed {
    let report = format!(
        "{}\n\n### 反証 (verify)\n{}",
        audit.report.trim_end(),
        r.report.trim_end()
    );
    let summary = if r.summary.trim().is_empty() {
        audit.summary.clone()
    } else {
        r.summary.clone()
    };
    Parsed {
        report,
        needs_user: r.needs_user,
        summary,
        marker_found: true,
    }
}

/// Inconclusive refutation (skeptic failed / no marker): keep the original
/// findings verbatim and annotate why — never drop on a broken verifier.
fn fold_inconclusive(audit: &Parsed) -> Parsed {
    let report = format!(
        "{}\n\n### 反証 (verify)\n反証不能 (skeptic agent 失敗/marker 欠落) — findings を存置した (本物を黙って消さない)。",
        audit.report.trim_end()
    );
    Parsed {
        report,
        needs_user: audit.needs_user,
        summary: audit.summary.clone(),
        marker_found: true,
    }
}

/// V2: surface verifiable canon rules the sampling audit never matched. Runs for
/// area and invariant shards (the decisions/D3 shard has no "unmatched rule"
/// notion). Appends one `completeness:<label>` entry per shard.
fn completeness(
    cfg: &Config,
    repo_root: &Path,
    scope: &Scope,
    shards: &[Shard],
    date: &str,
) -> Vec<(String, Parsed)> {
    let mut prompts: Vec<ShardPrompt> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for sh in shards {
        if matches!(sh, Shard::Decisions) {
            continue;
        }
        let label = format!("completeness:{}", prompt::shard_label(cfg, scope, *sh));
        prompts.push(ShardPrompt {
            label: label.clone(),
            prompt: prompt::render_completeness(cfg, scope, *sh, date),
        });
        labels.push(label);
    }
    if prompts.is_empty() {
        return Vec::new();
    }

    let outs = agent::run_shards(&cfg.agent, repo_root, prompts);
    outs.into_iter()
        .zip(labels)
        .map(|(o, label)| {
            let p = parse::parse(&o.out.stdout);
            if o.out.code != 0 || !p.marker_found {
                eprintln!(
                    "specguard: WARN 網羅性批評 inconclusive for '{label}' (agent 失敗/marker 欠落); fail-safe: needs_user=true で存置"
                );
                // A broken critic cannot confirm "nothing missed" — treat as
                // needs_user=true so the shard is not silently dropped from review.
                return (
                    label,
                    Parsed {
                        report: "網羅性批評 inconclusive (agent 失敗/marker 欠落)".to_string(),
                        needs_user: true,
                        summary: String::new(),
                        marker_found: true,
                    },
                );
            }
            (label, p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(report: &str, needs_user: bool, summary: &str) -> Parsed {
        Parsed {
            report: report.to_string(),
            needs_user,
            summary: summary.to_string(),
            marker_found: true,
        }
    }

    #[test]
    fn fold_refute_drops_when_skeptic_clears() {
        let audit = p("# audit\nfinding X", true, "drift X");
        let refuted = p("## 反証結果\nDROP: 引用が支持せず", false, "なし");
        let out = fold_refute(&audit, &refuted);
        // Post-verify the finding is cleared.
        assert!(!out.needs_user);
        assert_eq!(out.summary, "なし");
        // Both the original finding and the refutation are visible (transparency).
        assert!(out.report.contains("finding X"));
        assert!(out.report.contains("反証 (verify)"));
        assert!(out.report.contains("引用が支持せず"));
    }

    #[test]
    fn fold_refute_keeps_when_skeptic_upholds() {
        let audit = p("# audit\nfinding X", true, "drift X");
        let refuted = p("## 反証結果\nKEEP: 覆せない", true, "drift X 確定");
        let out = fold_refute(&audit, &refuted);
        assert!(out.needs_user);
        assert_eq!(out.summary, "drift X 確定");
    }

    #[test]
    fn fold_inconclusive_keeps_findings() {
        let audit = p("# audit\nfinding X", true, "drift X");
        let out = fold_inconclusive(&audit);
        // Fail-safe: a broken verifier never drops a real finding.
        assert!(out.needs_user);
        assert_eq!(out.summary, "drift X");
        assert!(out.report.contains("反証不能"));
        assert!(out.report.contains("finding X"));
    }
}
