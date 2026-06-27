//! The routing policy: turn a task + its neighbours into a worker/verifier model
//! choice. Core rule = **cheapest tier that historically clears the bar**; fall
//! back to a keyword prior on cold start. This is the deterministic stand-in for
//! fugu's trained coordinator — retrieval over outcomes instead of weight updates.

use std::collections::BTreeMap;

use crate::rag::Neighbor;
use crate::rng::Rng;

/// Cost order of the model tiers condukt understands. Index = cheapest-first.
const TIERS: &[&str] = &["haiku", "sonnet", "opus"];

#[derive(Debug, Clone)]
pub struct Decision {
    pub worker_model: String,
    pub verifier_model: String,
    /// "learned" | "prior" | "gated"
    pub basis: &'static str,
    /// "high" | "low"
    pub confidence: &'static str,
    pub neighbors: usize,
    pub rationale: String,
}

const DESIGN_KW: &[&str] = &[
    "design",
    "architecture",
    "refactor",
    "migrate",
    "migration",
    "schema",
    "security",
    "auth",
    "concurren",
    "protocol",
    "algorithm",
    "crypto",
    "race",
    "performance",
];
const TRIVIAL_KW: &[&str] = &[
    "rename",
    "typo",
    "format",
    "comment",
    "doc",
    "docs",
    "readme",
    "whitespace",
    "lint",
    "bump",
    "copy",
    "label",
    "wording",
];

/// Cold-start model guess from task text + file count (mirrors the interpreter's
/// own rule: design→opus, trivial→cheap, else sonnet).
///
/// Order matters: triviality is judged **before** the raw `file_count > 5`
/// rule. Otherwise a many-file mechanical chore (a repo-wide `rename`, a
/// formatting sweep) would short-circuit to opus on volume alone — the cold
/// start over-charges exactly the cheap work, before any history exists to
/// correct it. Design/high-stakes keywords still win outright.
pub fn prior_model(title: &str, file_count: usize) -> &'static str {
    let t = title.to_lowercase();

    // Design / high-stakes work warrants the strong tier even at one file.
    if DESIGN_KW.iter().any(|k| t.contains(k)) {
        return "opus";
    }
    // A mechanical chore stays cheap across many files — the *kind* of work is
    // trivial, so volume alone shouldn't force opus. A large sweep still gets
    // sonnet (more surface = more chance of a slip); a small one gets haiku.
    if TRIVIAL_KW.iter().any(|k| t.contains(k)) {
        return if file_count > 5 { "sonnet" } else { "haiku" };
    }
    // Non-trivial, non-design: a wide blast radius is itself a complexity signal.
    if file_count > 5 {
        return "opus";
    }
    "sonnet"
}

/// Independent verifier: generally a not-cheaper, different model so it doesn't
/// share the worker's blind spots. Escalate for serial / high-stakes work.
pub fn verifier_model(worker: &str, class: &str, title: &str) -> &'static str {
    let t = title.to_lowercase();
    let high_stakes = class == "serial" || DESIGN_KW.iter().any(|k| t.contains(k));
    match worker {
        "opus" => "sonnet", // strong worker → independent (cheaper) second pair of eyes
        _ => {
            if high_stakes {
                "opus"
            } else {
                "sonnet"
            }
        }
    }
}

/// Drop a model one tier toward the cheapest. haiku is the floor.
fn one_tier_down(model: &str) -> &'static str {
    match model {
        "opus" => "sonnet",
        "sonnet" => "haiku",
        _ => "haiku",
    }
}

/// Under daily budget pressure (budgetguard reports the warn threshold reached),
/// bias the decision cheaper: shave the worker one tier and cap the verifier at
/// sonnet (suppress opus escalation). Deterministic and explicitly recorded in
/// the rationale. A no-op for `gated` tasks (human-approved, never auto-routed)
/// and when nothing can be lowered (worker already haiku, verifier not opus).
pub fn downgrade_for_budget(d: Decision) -> Decision {
    if d.basis == "gated" {
        return d;
    }
    let new_worker = one_tier_down(&d.worker_model);
    // Never let the independent verifier sit at opus while we're saving money.
    let new_verifier = if d.verifier_model == "opus" {
        "sonnet"
    } else {
        d.verifier_model.as_str()
    };
    if d.worker_model == new_worker && d.verifier_model == new_verifier {
        return d; // already at the floor — nothing to downgrade
    }
    // Record only the tiers that actually moved (the worker may already be at
    // the haiku floor while the verifier still gets capped).
    let mut changes = Vec::new();
    if d.worker_model != new_worker {
        changes.push(format!("worker {}→{new_worker}", d.worker_model));
    }
    if d.verifier_model != new_verifier {
        changes.push(format!("verifier {}→{new_verifier}", d.verifier_model));
    }
    let rationale = format!("{} | budget pressure: {}", d.rationale, changes.join(", "));
    Decision {
        worker_model: new_worker.to_string(),
        verifier_model: new_verifier.to_string(),
        basis: d.basis,
        confidence: d.confidence,
        neighbors: d.neighbors,
        rationale,
    }
}

struct ModelStat {
    count: usize,
    passes: usize,
}

fn aggregate(neighbors: &[Neighbor]) -> BTreeMap<String, ModelStat> {
    let mut m: BTreeMap<String, ModelStat> = BTreeMap::new();
    for n in neighbors {
        let s = m.entry(n.ep.model.clone()).or_insert(ModelStat {
            count: 0,
            passes: 0,
        });
        s.count += 1;
        // Learn from the human label when present, else the verifier's self-pass.
        if n.ep.effective_pass() {
            s.passes += 1;
        }
    }
    m
}

pub fn decide(
    title: &str,
    files: &[String],
    class: &str,
    neighbors: &[Neighbor],
    pass_threshold: f64,
    min_samples: usize,
) -> Decision {
    if class == "gated" {
        return Decision {
            worker_model: "opus".into(),
            verifier_model: "opus".into(),
            basis: "gated",
            confidence: "high",
            neighbors: 0,
            rationale: "gated task — human approval required; not auto-routed".into(),
        };
    }

    let stats = aggregate(neighbors);
    // Cheapest tier that historically clears the pass threshold with enough samples.
    let mut chosen: Option<(String, f64)> = None;
    for tier in TIERS {
        if let Some(s) = stats.get(*tier) {
            if s.count >= min_samples {
                let rate = s.passes as f64 / s.count as f64;
                if rate >= pass_threshold {
                    chosen = Some((tier.to_string(), rate));
                    break; // cheapest-first: stop at the first that qualifies
                }
            }
        }
    }

    let (worker, basis, confidence, rationale) = match chosen {
        Some((m, rate)) => (
            m.clone(),
            "learned",
            "high",
            format!(
                "{} similar task(s): {} passed {:.0}% → cheapest tier clearing {:.0}%",
                neighbors.len(),
                m,
                rate * 100.0,
                pass_threshold * 100.0
            ),
        ),
        None => {
            let p = prior_model(title, files.len());
            let why = if neighbors.is_empty() {
                "no similar history".to_string()
            } else {
                format!(
                    "{} neighbour(s) but none cleared {:.0}% with >={} samples",
                    neighbors.len(),
                    pass_threshold * 100.0,
                    min_samples
                )
            };
            (
                p.to_string(),
                "prior",
                "low",
                format!("{why} -> heuristic prior: {p}"),
            )
        }
    };

    let verifier = verifier_model(&worker, class, title).to_string();
    Decision {
        worker_model: worker,
        verifier_model: verifier,
        basis,
        confidence,
        neighbors: neighbors.len(),
        rationale,
    }
}

/// Thompson-sampling variant of [`decide`]. Same "cheapest tier that clears the
/// bar" goal, but instead of a hard pass-rate test it draws each tier's
/// pass-probability from a Beta(1+passes, 1+fails) posterior (Gaussian-
/// approximated) and takes the cheapest tier whose *sample* clears the bar. Wide
/// posteriors on rarely-tried cheap tiers get explored; as evidence accrues they
/// converge to exploitation. This is the online-learning the threshold rule lacks.
pub fn decide_bandit(
    title: &str,
    files: &[String],
    class: &str,
    neighbors: &[Neighbor],
    pass_threshold: f64,
    min_samples: usize,
    rng: &mut Rng,
) -> Decision {
    if class == "gated" {
        return Decision {
            worker_model: "opus".into(),
            verifier_model: "opus".into(),
            basis: "gated",
            confidence: "high",
            neighbors: 0,
            rationale: "gated task — human approval required; not auto-routed".into(),
        };
    }

    let stats = aggregate(neighbors);
    let posterior = |tier: &str| -> (f64, f64, usize) {
        let (count, passes) = stats
            .get(tier)
            .map(|s| (s.count, s.passes))
            .unwrap_or((0, 0));
        let a = 1.0 + passes as f64;
        let b = 1.0 + (count - passes) as f64;
        let mean = a / (a + b);
        let var = (a * b) / ((a + b) * (a + b) * (a + b + 1.0));
        (mean, var.sqrt(), count)
    };

    // Cheapest-first: take the first tier whose sampled rate clears the bar.
    let mut chosen: Option<(String, f64, usize)> = None;
    for tier in TIERS {
        let (mean, sd, count) = posterior(tier);
        let sample = rng.normal(mean, sd).clamp(0.0, 1.0);
        if sample >= pass_threshold {
            chosen = Some((tier.to_string(), mean, count));
            break;
        }
    }

    let (worker, basis, confidence, rationale) = match chosen {
        Some((m, mean, count)) => (
            m.clone(),
            "bandit",
            if count >= min_samples { "high" } else { "low" },
            format!(
                "Thompson: {m} cleared {:.0}% (posterior mean {:.0}%, {count} sample(s))",
                pass_threshold * 100.0,
                mean * 100.0
            ),
        ),
        None => {
            // No tier sampled the bar — exploit the best posterior mean we have,
            // else fall back to the cold-start prior.
            let best = TIERS
                .iter()
                .filter_map(|t| stats.get(*t).map(|_| (*t, posterior(t).0)))
                .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
            match best {
                Some((t, mean)) => (
                    t.to_string(),
                    "bandit",
                    "low",
                    format!(
                        "no tier sampled the bar → exploit best mean {:.0}% ({t})",
                        mean * 100.0
                    ),
                ),
                None => {
                    let p = prior_model(title, files.len());
                    (
                        p.to_string(),
                        "prior",
                        "low",
                        format!("no history → prior {p}"),
                    )
                }
            }
        }
    };

    let verifier = verifier_model(&worker, class, title).to_string();
    Decision {
        worker_model: worker,
        verifier_model: verifier,
        basis,
        confidence,
        neighbors: neighbors.len(),
        rationale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Episode;

    fn nb(model: &str, pass: bool) -> Neighbor {
        Neighbor {
            ep: Episode {
                ts: 0,
                title: "x".into(),
                touched_files: vec![],
                class: "parallel".into(),
                model: model.into(),
                role: "worker".into(),
                pass,
                cost_usd: 0.0,
                human_label: None,
                labeled_by: None,
            },
            sim: 0.5,
        }
    }

    #[test]
    fn human_label_overrides_pass_in_aggregate() {
        // Two sonnet neighbours that the verifier passed; a human labels one bad.
        let mut bad = nb("sonnet", true);
        bad.ep.human_label = Some(false);
        let good = nb("sonnet", true);
        let stats = aggregate(&[bad, good]);
        let s = stats.get("sonnet").unwrap();
        assert_eq!(s.count, 2);
        // only the human-good (unlabeled→self-pass) one counts as a pass.
        assert_eq!(s.passes, 1);
    }

    #[test]
    fn human_good_label_rescues_a_failed_episode() {
        // verifier failed it, but a human says good → counts as a pass.
        let mut rescued = nb("haiku", false);
        rescued.ep.human_label = Some(true);
        let stats = aggregate(&[rescued]);
        assert_eq!(stats.get("haiku").unwrap().passes, 1);
    }

    #[test]
    fn gated_is_not_auto_routed() {
        let d = decide("deploy to prod", &[], "gated", &[], 0.7, 2);
        assert_eq!(d.basis, "gated");
    }

    #[test]
    fn cold_start_uses_keyword_prior() {
        let d = decide("redesign auth architecture", &[], "serial", &[], 0.7, 2);
        assert_eq!(d.worker_model, "opus");
        assert_eq!(d.basis, "prior");
        // worker is already opus → verify independently with a cheaper sonnet.
        assert_eq!(d.verifier_model, "sonnet");

        let d2 = decide("rename a variable", &[], "parallel", &[], 0.7, 2);
        assert_eq!(d2.worker_model, "haiku");
    }

    #[test]
    fn multi_file_trivial_drops_below_opus() {
        // A many-file mechanical chore must NOT be billed at opus on volume
        // alone (the bug): a wide trivial sweep gets sonnet, a small one haiku.
        assert_eq!(prior_model("rename a symbol across the repo", 20), "sonnet");
        assert_eq!(prior_model("reformat the whole tree", 30), "sonnet");
        assert_eq!(prior_model("fix a typo", 1), "haiku");
    }

    #[test]
    fn budget_downgrade_shaves_worker_and_caps_verifier() {
        // opus worker / sonnet verifier under pressure → sonnet / sonnet.
        let d = decide("redesign auth architecture", &[], "serial", &[], 0.7, 2);
        assert_eq!(d.worker_model, "opus");
        assert_eq!(d.verifier_model, "sonnet");
        let dg = downgrade_for_budget(d);
        assert_eq!(dg.worker_model, "sonnet");
        assert_eq!(dg.verifier_model, "sonnet");
        assert!(dg.rationale.contains("budget pressure"));
    }

    #[test]
    fn budget_downgrade_suppresses_opus_verifier() {
        // sonnet worker + high-stakes (serial) → opus verifier; pressure caps it
        // at sonnet and shaves the worker to haiku. ("endpoint" hits no DESIGN_KW.)
        let d = decide("implement the endpoint", &[], "serial", &[], 0.7, 2);
        assert_eq!(d.worker_model, "sonnet");
        assert_eq!(d.verifier_model, "opus");
        let dg = downgrade_for_budget(d);
        assert_eq!(dg.worker_model, "haiku");
        assert_eq!(dg.verifier_model, "sonnet");
        assert!(dg.rationale.contains("verifier opus→sonnet"));
    }

    #[test]
    fn budget_downgrade_is_noop_at_floor_and_for_gated() {
        // haiku worker + sonnet verifier: already the floor → unchanged.
        let floor = decide("rename a variable", &[], "parallel", &[], 0.7, 2);
        assert_eq!(floor.worker_model, "haiku");
        assert_eq!(floor.verifier_model, "sonnet");
        let dg = downgrade_for_budget(floor);
        assert_eq!(dg.worker_model, "haiku");
        assert_eq!(dg.verifier_model, "sonnet");
        assert!(!dg.rationale.contains("budget pressure"));

        // gated stays human-gated, never downgraded.
        let gated = downgrade_for_budget(decide("deploy to prod", &[], "gated", &[], 0.7, 2));
        assert_eq!(gated.basis, "gated");
        assert_eq!(gated.worker_model, "opus");
    }

    #[test]
    fn multi_file_design_and_nontrivial_stay_opus() {
        // Design keywords win outright, even across many files…
        assert_eq!(prior_model("refactor the auth module", 20), "opus");
        // …and a wide non-trivial, non-design change keeps its volume signal.
        assert_eq!(prior_model("implement the new endpoints", 8), "opus");
        // An ordinary small change is still sonnet.
        assert_eq!(prior_model("add a field to the response", 1), "sonnet");
    }

    #[test]
    fn learned_picks_cheapest_that_clears() {
        // haiku passes 3/3 on similar tasks → choose haiku even if sonnet also passes.
        let neighbors = vec![
            nb("haiku", true),
            nb("haiku", true),
            nb("haiku", true),
            nb("sonnet", true),
        ];
        let d = decide("implement endpoint", &[], "parallel", &neighbors, 0.7, 2);
        assert_eq!(d.worker_model, "haiku");
        assert_eq!(d.basis, "learned");
    }

    #[test]
    fn learned_escalates_when_cheap_fails() {
        // haiku fails 0/2, sonnet passes 2/2 → choose sonnet.
        let neighbors = vec![
            nb("haiku", false),
            nb("haiku", false),
            nb("sonnet", true),
            nb("sonnet", true),
        ];
        let d = decide("implement endpoint", &[], "parallel", &neighbors, 0.7, 2);
        assert_eq!(d.worker_model, "sonnet");
        assert_eq!(d.basis, "learned");
    }

    #[test]
    fn too_few_samples_falls_back_to_prior() {
        // only one haiku pass — below min_samples=2 → prior, not learned.
        let neighbors = vec![nb("haiku", true)];
        let d = decide(
            "tweak something ordinary",
            &[],
            "parallel",
            &neighbors,
            0.7,
            2,
        );
        assert_eq!(d.basis, "prior");
    }

    // --- bandit (Thompson sampling) -------------------------------------------

    fn tally_picks(neighbors: &[Neighbor], iters: u64) -> BTreeMap<String, usize> {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for i in 0..iters {
            let mut rng = Rng::new(i + 1);
            let d = decide_bandit(
                "implement an endpoint",
                &[],
                "parallel",
                neighbors,
                0.7,
                2,
                &mut rng,
            );
            *m.entry(d.worker_model).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn bandit_exploits_strong_history() {
        // haiku passes 5/5 → it should dominate.
        let neighbors = vec![
            nb("haiku", true),
            nb("haiku", true),
            nb("haiku", true),
            nb("haiku", true),
            nb("haiku", true),
        ];
        let t = tally_picks(&neighbors, 300);
        assert!(
            t.get("haiku").copied().unwrap_or(0) > 230,
            "haiku picks: {t:?}"
        );
    }

    #[test]
    fn bandit_explores_untested_cheaper_tier() {
        // sonnet has a track record; haiku is untested. The bandit must still try
        // the cheaper haiku sometimes (exploration), while favouring sonnet.
        let neighbors = vec![
            nb("sonnet", true),
            nb("sonnet", true),
            nb("sonnet", true),
            nb("sonnet", true),
        ];
        let t = tally_picks(&neighbors, 300);
        assert!(
            t.get("haiku").copied().unwrap_or(0) > 20,
            "expected some haiku exploration: {t:?}"
        );
        assert!(
            t.get("sonnet").copied().unwrap_or(0) > t.get("opus").copied().unwrap_or(0),
            "{t:?}"
        );
    }

    #[test]
    fn bandit_avoids_failing_cheap_tier() {
        // haiku fails 0/4, sonnet passes 4/4 → haiku should rarely be chosen.
        let neighbors = vec![
            nb("haiku", false),
            nb("haiku", false),
            nb("haiku", false),
            nb("haiku", false),
            nb("sonnet", true),
            nb("sonnet", true),
            nb("sonnet", true),
            nb("sonnet", true),
        ];
        let t = tally_picks(&neighbors, 300);
        assert!(
            t.get("haiku").copied().unwrap_or(0) < 50,
            "haiku over-chosen: {t:?}"
        );
        assert!(t.get("sonnet").copied().unwrap_or(0) > 150, "{t:?}");
    }

    #[test]
    fn bandit_keeps_gated_human_gated() {
        let mut rng = Rng::new(1);
        let d = decide_bandit("deploy to prod", &[], "gated", &[], 0.7, 2, &mut rng);
        assert_eq!(d.basis, "gated");
    }
}
