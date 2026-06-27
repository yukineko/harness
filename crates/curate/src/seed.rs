//! Read fugu-router's playbook store as promotion *seeds*.
//!
//! fugu-router writes one playbook per verified task — `title`, `done_criteria`,
//! `touched_files`, `class` — to `~/.fugu-router/playbooks.jsonl` (append-only,
//! policy-search only; never holdout-curated). curate reads it read-only and a
//! seed becomes a golden case. We deserialize only the fields we need; unknown
//! fields (notes, ts extras) are ignored, so we stay decoupled from fugu's
//! struct.

use std::path::{Path, PathBuf};

use harness_core::config::home;
use serde::Deserialize;

/// A promotion candidate distilled from one fugu playbook entry. Only the fields
/// curation needs are deserialized; the rest of the playbook (class,
/// touched_files, notes) is ignored, keeping curate decoupled from fugu's struct.
#[derive(Debug, Clone, Deserialize)]
pub struct Seed {
    #[serde(default)]
    pub ts: u64,
    pub title: String,
    #[serde(default)]
    pub done_criteria: String,
}

/// Default fugu playbook store: `~/.fugu-router/playbooks.jsonl`.
pub fn default_store() -> PathBuf {
    home().join(".fugu-router").join("playbooks.jsonl")
}

/// Load seeds from a playbook JSONL store, skipping malformed/blank lines
/// (a corrupt line must not sink curation). Missing file → empty.
pub fn load(path: &Path) -> Vec<Seed> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Seed>(l).ok())
        .collect()
}

/// Pick the seed to promote: the most recent (`ts`, tie → last) among those
/// whose title contains `selector` (case-insensitive), or the most recent
/// overall when `latest`. Pure for unit-testing.
pub fn select(seeds: &[Seed], selector: Option<&str>, latest: bool) -> Option<usize> {
    let needle = if latest {
        None
    } else {
        selector.map(|s| s.to_lowercase())
    };
    seeds
        .iter()
        .enumerate()
        .filter(|(_, s)| match &needle {
            Some(n) => s.title.to_lowercase().contains(n),
            None => true,
        })
        .max_by_key(|(i, s)| (s.ts, *i))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(title: &str, ts: u64) -> Seed {
        Seed {
            ts,
            title: title.into(),
            done_criteria: String::new(),
        }
    }

    #[test]
    fn latest_ignores_selector_and_takes_newest() {
        let s = vec![seed("a", 10), seed("b", 30), seed("c", 20)];
        assert_eq!(select(&s, Some("a"), true), Some(1));
    }

    #[test]
    fn selector_takes_most_recent_match() {
        let s = vec![
            seed("add login", 10),
            seed("add LOGIN v2", 20),
            seed("x", 30),
        ];
        assert_eq!(select(&s, Some("login"), false), Some(1));
    }

    #[test]
    fn no_match_is_none() {
        let s = vec![seed("a", 1)];
        assert_eq!(select(&s, Some("zzz"), false), None);
    }
}
