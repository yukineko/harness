//! Configuration: project `session-insights.toml` over
//! `~/.session-insights/config.toml` over defaults (first file that exists
//! wins). Env kill switch `SESSION_INSIGHTS_DISABLE`. Recording only ever writes
//! to its own state dir (and, opt-in, an Obsidian vault) — it never blocks a turn.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use harness_core::config::home;
// Re-exported so existing `crate::config::expand_tilde` call sites keep working.
pub use harness_core::config::expand_tilde;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// Tools excluded from metrics (bookkeeping noise).
    pub ignore_tools: Vec<String>,
    /// Size-class thresholds by total recorded tool events: [S, M, L, XL].
    pub size_thresholds: [usize; 4],
    /// Write a dated session note to an Obsidian vault on Stop.
    pub obsidian_log: bool,
    /// Vault root for the session note (subdir `sessions/` is used).
    pub obsidian_vault: PathBuf,
    pub state_dir: PathBuf,
    /// Write/update an AEGIS-style record note on SessionEnd (opt-in).
    pub record: bool,
    /// Vault subdir for record notes (kept separate from terse `sessions/`).
    pub record_dir: String,
    /// Per-model price overrides for the cost block; empty → built-in rates.
    pub price_overrides: Vec<harness_core::pricing::PriceOverride>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    ignore_tools: Option<Vec<String>>,
    size_thresholds: Option<Vec<usize>>,
    obsidian_log: Option<bool>,
    obsidian_vault: Option<String>,
    state_dir: Option<String>,
    record: Option<bool>,
    record_dir: Option<String>,
    price_overrides: Option<Vec<PriceOverrideEntry>>,
}

#[derive(Debug, Default, Deserialize)]
struct PriceOverrideEntry {
    pattern: String,
    input: f64,
    output: f64,
}

/// The `~/.session-insights` base directory. Thin wrapper over the shared helper;
/// the on-disk dir name is preserved exactly.
pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("session-insights")
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            ignore_tools: vec!["TodoWrite".to_string()],
            size_thresholds: [5, 15, 40, 100],
            obsidian_log: false,
            obsidian_vault: home().join("Documents/vault/yukineko"),
            state_dir: base_dir().join("state"),
            record: false,
            record_dir: "records".to_string(),
            price_overrides: Vec::new(),
        }
    }
}

impl Config {
    pub fn project_path(root: &Path) -> PathBuf {
        root.join("session-insights.toml")
    }
    pub fn home_path() -> PathBuf {
        base_dir().join("config.toml")
    }

    pub fn load(root: &Path) -> Self {
        let mut cfg = Config::default();
        let chosen = {
            let p = Config::project_path(root);
            if p.exists() {
                Some(p)
            } else {
                let h = Config::home_path();
                h.exists().then_some(h)
            }
        };
        if let Some(path) = chosen {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled {
                        cfg.enabled = v;
                    }
                    if let Some(v) = fc.ignore_tools {
                        cfg.ignore_tools = v;
                    }
                    if let Some(v) = fc.size_thresholds {
                        if v.len() == 4 {
                            cfg.size_thresholds = [v[0], v[1], v[2], v[3]];
                        }
                    }
                    if let Some(v) = fc.obsidian_log {
                        cfg.obsidian_log = v;
                    }
                    if let Some(v) = fc.obsidian_vault {
                        cfg.obsidian_vault = expand_tilde(&v);
                    }
                    if let Some(v) = fc.state_dir {
                        cfg.state_dir = expand_tilde(&v);
                    }
                    if let Some(v) = fc.record {
                        cfg.record = v;
                    }
                    if let Some(v) = fc.record_dir {
                        cfg.record_dir = v;
                    }
                    if let Some(v) = fc.price_overrides {
                        cfg.price_overrides = v
                            .into_iter()
                            .map(|e| harness_core::pricing::PriceOverride {
                                pattern: e.pattern.to_lowercase(),
                                input: e.input,
                                output: e.output,
                            })
                            .collect();
                    }
                }
            }
        }
        cfg
    }

    pub fn disabled_env() -> bool {
        std::env::var("SESSION_INSIGHTS_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }

    pub fn is_ignored(&self, tool: &str) -> bool {
        self.ignore_tools.iter().any(|t| t == tool)
    }
}
