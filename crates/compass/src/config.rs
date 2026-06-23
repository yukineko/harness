//! Configuration: project `.compass/config.toml` (DESIGN §12) over built-in
//! defaults.
//!
//! Mirrors the DESIGN §12 schema:
//! ```toml
//! [freshness]
//! stale_commits  = 20    # commits since charter last touched (primary signal)
//! stale_days     = 14    # wall-clock since last touch (secondary signal)
//! check_dod_refs = true  # check DoD-referenced paths/symbols still exist
//! [carve]
//! max_rounds     = 4     # interrogate sync-round cap. 0 = all sentinel
//! [routing]
//! right_size     = ["s", "m"]  # B-plan: these go to condukt, rest is parked (§6)
//! ```
//!
//! Every section and field is `#[serde(default)]` so a missing file, missing
//! section, or missing key all fall back to the §12 defaults. A parse error
//! silently yields defaults (a re-grounding tool must never crash a turn).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Resolved compass configuration. See DESIGN §12.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub freshness: Freshness,
    pub carve: Carve,
    pub routing: Routing,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Freshness {
    /// Commits since the charter was last touched before it's "drift suspect".
    pub stale_commits: u32,
    /// Wall-clock days since last touch before it's "drift suspect".
    pub stale_days: u32,
    /// Whether to check that DoD-referenced paths/symbols still exist.
    pub check_dod_refs: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Carve {
    /// interrogate sync-round cap. 0 = emit everything as a sentinel (no sync).
    pub max_rounds: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Routing {
    /// B-plan (§6): sizes considered "right-sized" and routed to condukt; the
    /// rest is parked.
    pub right_size: Vec<String>,
}

impl Default for Freshness {
    fn default() -> Self {
        Freshness {
            stale_commits: 20,
            stale_days: 14,
            check_dod_refs: true,
        }
    }
}

impl Default for Carve {
    fn default() -> Self {
        Carve { max_rounds: 4 }
    }
}

impl Default for Routing {
    fn default() -> Self {
        Routing {
            right_size: vec!["s".to_string(), "m".to_string()],
        }
    }
}

impl Config {
    /// `.compass/config.toml` under the project root.
    pub fn project_path(root: &Path) -> PathBuf {
        root.join(".compass").join("config.toml")
    }

    /// Load config for a project root. A missing file or any parse error
    /// silently falls back to the §12 defaults.
    pub fn load(root: &Path) -> Self {
        let path = Config::project_path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<Config>(&text).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_design_section_12() {
        let cfg = Config::default();
        assert_eq!(cfg.freshness.stale_commits, 20);
        assert_eq!(cfg.freshness.stale_days, 14);
        assert!(cfg.freshness.check_dod_refs);
        assert_eq!(cfg.carve.max_rounds, 4);
        assert_eq!(cfg.routing.right_size, vec!["s", "m"]);
    }

    #[test]
    fn partial_toml_keeps_defaults_for_omitted_fields() {
        let text = "[freshness]\nstale_commits = 5\n";
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.freshness.stale_commits, 5);
        // omitted in the same section -> default
        assert_eq!(cfg.freshness.stale_days, 14);
        assert!(cfg.freshness.check_dod_refs);
        // omitted sections -> default
        assert_eq!(cfg.carve.max_rounds, 4);
        assert_eq!(cfg.routing.right_size, vec!["s", "m"]);
    }
}
