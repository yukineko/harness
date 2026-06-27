//! gap (DESIGN §3) — deterministic ASSEMBLY of the inputs a skill reasons over
//! to derive "goal − current-state", plus a write-back of the skill-produced
//! gap text into the charter.
//!
//! # Architecture constraint
//!
//! A Rust binary cannot call an LLM, so the SEMANTIC gap derivation ("which DoD
//! items are unmet, what's the biggest gap") is a SKILL job — NOT here. This
//! module only deterministically gathers the raw material the skill will reason
//! over ([`assemble_gap_inputs`]) and persists the gap string the skill writes
//! back ([`persist_gap`]). No judgment, no LLM.

use std::path::Path;

use anyhow::Result;
use harness_core::interrogate::{Authority, Bundle};
use serde::Serialize;

use crate::charter::Charter;
use crate::outcome::Outcome;

/// The deterministic inputs to the (skill-side) gap derivation. Serialized to
/// JSON and printed so the skill can read and reason over it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GapInputs {
    /// Observable done conditions (charter `definition_of_done`).
    pub dod: Vec<String>,
    /// Mid-authority "what the project is actually doing now" fragments
    /// (git log / status / diff), newest/most-relevant first.
    pub recent_activity: Vec<String>,
    /// The taskprog progress excerpt, if present (the self-feeding "残り" loop).
    pub progress_excerpt: Option<String>,
    /// What the next move is measured by (charter `measuring_stick`).
    pub measuring_stick: String,
    /// The most recently recorded outcome (verdict + evidence + the gap it
    /// judged), or `None` if no move has been judged yet. Closes the measurement
    /// loop: the skill sees how the last move actually moved the needle. Set by
    /// `gap_command`; `assemble_gap_inputs` leaves it `None`.
    pub last_outcome: Option<Outcome>,
}

/// Deterministically assemble the gap inputs from the charter (the stated goal)
/// and the gather [`Bundle`] (what the project is actually doing now).
///
/// The split is purely by provenance, no semantic judgment:
///   - `dod` / `measuring_stick` come straight from the charter,
///   - `recent_activity` is the Mid-authority git fragments (log/status/diff),
///   - `progress_excerpt` is the Mid-authority taskprog fragment, if any.
pub fn assemble_gap_inputs(charter: &Charter, bundle: &Bundle) -> GapInputs {
    let mut recent_activity = Vec::new();
    let mut progress_excerpt = None;

    for frag in &bundle.fragments {
        if frag.authority != Authority::Mid {
            continue;
        }
        match frag.source_path.as_str() {
            // git activity → recent_activity (preserve gather()'s order).
            "git:log" | "git:status" | "git:diff" => {
                recent_activity.push(frag.text.clone());
            }
            // taskprog progress → progress_excerpt (first one wins).
            ".claude/progress.md" | "progress.md" if progress_excerpt.is_none() => {
                progress_excerpt = Some(frag.text.clone());
            }
            // deepwiki and anything else is not gap-input material here.
            _ => {}
        }
    }

    GapInputs {
        dod: charter.definition_of_done.clone(),
        recent_activity,
        progress_excerpt,
        measuring_stick: charter.measuring_stick.clone(),
        // The caller (`gap_command`) fills this from the outcomes store; the
        // pure assembly has no repo root to read it from.
        last_outcome: None,
    }
}

/// Persist the skill-produced gap text into `charter.current_gap` and save the
/// charter back to `charter_path`. Deterministic write-back; no interpretation.
pub fn persist_gap(charter_path: &Path, charter: &mut Charter, gap: &str) -> Result<()> {
    charter.current_gap = gap.trim().to_string();
    charter.save(charter_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::interrogate::Fragment;

    fn frag(text: &str, source: &str, authority: Authority) -> Fragment {
        Fragment {
            text: text.to_string(),
            source_path: source.to_string(),
            authority,
            score: 0,
            anchor: None,
        }
    }

    #[test]
    fn assemble_splits_by_provenance() {
        let charter = Charter {
            definition_of_done: vec!["crate builds".to_string(), "tests pass".to_string()],
            measuring_stick: "defensibility x closeness / cost".to_string(),
            ..Charter::default()
        };
        let bundle = Bundle {
            fragments: vec![
                frag("north_star text", ".compass/charter.md", Authority::High),
                frag("abc123 do a thing", "git:log", Authority::Mid),
                frag(" M src/lib.rs", "git:status", Authority::Mid),
                frag("- remaining item", ".claude/progress.md", Authority::Mid),
                frag("some wiki page", ".deepwiki/arch.md", Authority::Mid),
            ],
        };

        let inputs = assemble_gap_inputs(&charter, &bundle);
        assert_eq!(inputs.dod, vec!["crate builds", "tests pass"]);
        assert_eq!(inputs.measuring_stick, "defensibility x closeness / cost");
        // git fragments in gather order; deepwiki excluded; charter excluded.
        assert_eq!(
            inputs.recent_activity,
            vec!["abc123 do a thing".to_string(), " M src/lib.rs".to_string()]
        );
        assert_eq!(inputs.progress_excerpt.as_deref(), Some("- remaining item"));
    }

    #[test]
    fn assemble_with_no_activity_is_empty_not_error() {
        let charter = Charter::default();
        let bundle = Bundle { fragments: vec![] };
        let inputs = assemble_gap_inputs(&charter, &bundle);
        assert!(inputs.recent_activity.is_empty());
        assert!(inputs.progress_excerpt.is_none());
        assert!(inputs.dod.is_empty());
    }

    #[test]
    fn last_outcome_is_null_when_none_recorded() {
        let charter = Charter::default();
        let bundle = Bundle { fragments: vec![] };
        let inputs = assemble_gap_inputs(&charter, &bundle);
        assert!(inputs.last_outcome.is_none());

        let json = serde_json::to_value(&inputs).expect("serialize");
        assert!(json["last_outcome"].is_null());
    }

    #[test]
    fn last_outcome_surfaces_latest_recorded() {
        use crate::outcome::{self, Verdict};

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let charter = Charter {
            north_star: "ship the loop".to_string(),
            current_gap: "moves are unjudged".to_string(),
            ..Charter::default()
        };

        // record one outcome, then mirror what `gap_command` does.
        outcome::record(root, &charter, Verdict::Forward, vec!["p95 fell 20%".to_string()])
            .expect("record");

        let mut inputs = assemble_gap_inputs(&charter, &Bundle { fragments: vec![] });
        inputs.last_outcome = outcome::latest(root).expect("latest");

        let last = inputs.last_outcome.as_ref().expect("some outcome");
        assert_eq!(last.verdict, Verdict::Forward);
        assert_eq!(last.evidence, vec!["p95 fell 20%".to_string()]);
        assert_eq!(last.current_gap, "moves are unjudged");

        let json = serde_json::to_value(&inputs).expect("serialize");
        assert_eq!(json["last_outcome"]["verdict"], "forward");
        assert_eq!(json["last_outcome"]["evidence"][0], "p95 fell 20%");
        assert_eq!(json["last_outcome"]["current_gap"], "moves are unjudged");
    }

    #[test]
    fn persist_gap_writes_and_saves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".compass").join("charter.md");
        let mut charter = Charter {
            north_star: "ship compass".to_string(),
            definition_of_done: vec!["builds".to_string()],
            ..Charter::default()
        };

        persist_gap(&path, &mut charter, "  the biggest gap is X  ").expect("persist");
        assert_eq!(charter.current_gap, "the biggest gap is X");

        let reloaded = Charter::load(&path).expect("load");
        assert_eq!(reloaded.current_gap, "the biggest gap is X");
        // other fields preserved.
        assert_eq!(reloaded.north_star, "ship compass");
    }
}
