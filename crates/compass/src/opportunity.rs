//! opportunity (DESIGN §-OST) — the discovery-side layer between the charter's
//! single `north_star` (the active outcome) and the one solution-action `route`
//! hands to condukt. PDO's Opportunity Solution Tree puts named customer
//! needs/opportunities under an outcome; compass previously went
//! north_star→gap→solution with no opportunity layer at all.
//!
//! This is the persisted store (charter DoD#1): an append-only JSON array of
//! [`Opportunity`] under `.compass/opportunities.json`, each scoped to an active
//! outcome (snapshotted as the charter `north_star`, mirroring how
//! [`crate::outcome`] snapshots its goal). Wiring the opportunity ref into the
//! `route` handoff (DoD#2) and per-opportunity `gap` (DoD#3) build on top and are
//! parked follow-ups.
//!
//! # Persistence
//!
//! Load → append → atomic-write, mirroring the outcome/charter store conventions.
//! Recording REQUIRES a non-empty title (build is not validation: an empty
//! opportunity is not a recorded bet), mirroring the evidence guard in
//! [`crate::outcome::record`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A single named opportunity (customer need / bet) sitting under an active
/// outcome. Self-describing: it snapshots the `outcome_ref` it was filed under so
/// a later reader can group opportunities by outcome with no other context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opportunity {
    /// Stable, human-readable id derived from the title (slug + short hash).
    pub id: String,
    /// The opportunity statement (non-empty, trimmed).
    pub title: String,
    /// The active outcome this opportunity sits under (charter `north_star`
    /// snapshot, or an explicit `--outcome` override).
    pub outcome_ref: String,
    /// Wall-clock record time, unix seconds (0 if the clock is pre-epoch).
    pub created_at: u64,
}

/// On-disk shape: a JSON object with an `opportunities` array (forward-compatible).
#[derive(Debug, Default, Serialize, Deserialize)]
struct OpportunitiesFile {
    #[serde(default)]
    opportunities: Vec<Opportunity>,
}

/// `.compass/opportunities.json` under the project root.
pub fn store_path(root: &Path) -> PathBuf {
    root.join(".compass").join("opportunities.json")
}

/// Load all recorded opportunities (oldest first). A missing file => empty Vec; a
/// corrupt file is a hard error (we don't silently drop recorded bets).
pub fn load(root: &Path) -> Result<Vec<Opportunity>> {
    let path = store_path(root);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let file: OpportunitiesFile = serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?;
            Ok(file.opportunities)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Atomic-write the full opportunities array, creating `.compass/` if absent.
fn save(root: &Path, opportunities: &[Opportunity]) -> Result<()> {
    let path = store_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = OpportunitiesFile {
        opportunities: opportunities.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file).context("serializing opportunities")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

/// Current wall-clock as unix seconds (0 if the system clock is before epoch).
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A stable, human-readable id for an opportunity title: an ASCII slug plus a
/// 6-hex hash suffix for uniqueness. A purely non-ASCII title (e.g. Japanese)
/// slugs to empty, so it falls back to `opp-<hash>`. Deterministic per title
/// (mirrors the slug-id convention used by curate/replaykit).
fn slug_id(title: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut h);
    let suffix = format!("{:06x}", h.finish() & 0xff_ffff);
    let slug = title
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let slug = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("opp-{suffix}")
    } else {
        format!("{slug}-{suffix}")
    }
}

/// Record one named opportunity under `outcome_ref` and append it to the store.
/// REQUIRES a non-empty title (trimmed); bails otherwise (an empty bet is not a
/// recorded opportunity). Returns the persisted [`Opportunity`].
pub fn record(root: &Path, title: &str, outcome_ref: &str) -> Result<Opportunity> {
    let title = title.trim();
    if title.is_empty() {
        anyhow::bail!("opportunity requires a non-empty --title (a named bet, not a blank)");
    }
    let mut opportunities = load(root)?;
    let opportunity = Opportunity {
        id: slug_id(title),
        title: title.to_string(),
        outcome_ref: outcome_ref.trim().to_string(),
        created_at: now_unix(),
    };
    opportunities.push(opportunity.clone());
    save(root, &opportunities)?;
    Ok(opportunity)
}

/// All opportunities filed under `outcome_ref` (oldest first). An empty store or
/// a non-matching outcome yields an empty Vec.
pub fn list_under(root: &Path, outcome_ref: &str) -> Result<Vec<Opportunity>> {
    let want = outcome_ref.trim();
    Ok(load(root)?
        .into_iter()
        .filter(|o| o.outcome_ref == want)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_with_title_persists_and_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let rec = record(
            root,
            "  users can't see why a move was chosen  ",
            "ship OST",
        )
        .expect("record");

        // title trimmed, scoped to the outcome, stable id derived.
        assert_eq!(rec.title, "users can't see why a move was chosen");
        assert_eq!(rec.outcome_ref, "ship OST");
        assert!(rec.id.ends_with(&rec.id[rec.id.len() - 6..]));
        assert!(!rec.id.is_empty());

        // reload from disk: the record round-trips.
        let loaded = load(root).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], rec);
    }

    #[test]
    fn empty_title_is_rejected_and_persists_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let err = record(root, "   ", "ship OST").unwrap_err();
        assert!(err.to_string().contains("non-empty --title"));

        // nothing was written.
        assert!(!store_path(root).exists());
        assert_eq!(load(root).expect("load").len(), 0);
    }

    #[test]
    fn list_under_filters_by_active_outcome() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        record(root, "opportunity A", "outcome-1").expect("a");
        record(root, "opportunity B", "outcome-1").expect("b");
        record(root, "opportunity C", "outcome-2").expect("c");

        let under_1 = list_under(root, "outcome-1").expect("list 1");
        assert_eq!(under_1.len(), 2);
        assert!(under_1.iter().all(|o| o.outcome_ref == "outcome-1"));

        let under_2 = list_under(root, "outcome-2").expect("list 2");
        assert_eq!(under_2.len(), 1);

        // a non-matching outcome yields empty.
        assert!(list_under(root, "outcome-X").expect("list x").is_empty());
    }

    #[test]
    fn list_under_is_empty_when_no_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(list_under(dir.path(), "any").expect("list").is_empty());
    }

    #[test]
    fn slug_id_falls_back_for_non_ascii() {
        // a purely non-ASCII (Japanese) title slugs to empty => opp-<hash>.
        let id = slug_id("機会の層");
        assert!(id.starts_with("opp-"), "got {id}");
        // deterministic per title.
        assert_eq!(id, slug_id("機会の層"));
        // distinct titles get distinct ids (overwhelmingly likely).
        assert_ne!(slug_id("alpha"), slug_id("beta"));
    }
}
