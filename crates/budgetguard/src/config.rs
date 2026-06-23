//! Config: project `budgetguard.toml` layered over `~/.budgetguard/config.toml`.
//!
//! All limits default to 0 (disabled). A limit of 0 means "no limit" — the gate
//! always allows the stop. This keeps the plugin safe-by-default: installing it
//! without configuring any limits is a no-op.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use harness_core::config::expand_tilde;
pub use harness_core::pricing::PriceOverride;

/// The project or home config file.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    state_dir: Option<String>,
    #[serde(default)]
    session: BudgetLevel,
    #[serde(default)]
    daily: BudgetLevel,
    /// Override the built-in price table for specific models.
    #[serde(default)]
    price: Vec<PriceOverrideCfg>,
}

#[derive(Debug, Default, Deserialize)]
struct BudgetLevel {
    /// Emit a warning (additionalContext) when spend crosses this. 0 = disabled.
    warn_usd: Option<f64>,
    /// Block the stop when spend crosses this. 0 = disabled.
    block_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PriceOverrideCfg {
    pattern: String,
    input: f64,
    output: f64,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// Per-session warn/block (USD). 0.0 = disabled.
    pub session_warn_usd: f64,
    pub session_block_usd: f64,
    /// Per-calendar-day warn/block (USD, all sessions). 0.0 = disabled.
    pub daily_warn_usd: f64,
    pub daily_block_usd: f64,
    pub state_dir: PathBuf,
    pub price_overrides: Vec<PriceOverride>,
}

pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("budgetguard")
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            session_warn_usd: 0.0,
            session_block_usd: 0.0,
            daily_warn_usd: 0.0,
            daily_block_usd: 0.0,
            state_dir: base_dir().join("state"),
            price_overrides: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(root: &Path) -> Self {
        let mut cfg = Config::default();

        let path = {
            let p = root.join("budgetguard.toml");
            if p.exists() {
                Some(p)
            } else {
                let h = base_dir().join("config.toml");
                if h.exists() { Some(h) } else { None }
            }
        };

        if let Some(path) = path {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled { cfg.enabled = v; }
                    if let Some(v) = fc.state_dir { cfg.state_dir = expand_tilde(&v); }
                    if let Some(v) = fc.session.warn_usd { cfg.session_warn_usd = v; }
                    if let Some(v) = fc.session.block_usd { cfg.session_block_usd = v; }
                    if let Some(v) = fc.daily.warn_usd { cfg.daily_warn_usd = v; }
                    if let Some(v) = fc.daily.block_usd { cfg.daily_block_usd = v; }
                    cfg.price_overrides = fc.price.into_iter().map(|p| PriceOverride {
                        pattern: p.pattern,
                        input: p.input,
                        output: p.output,
                    }).collect();
                }
            }
        }
        cfg
    }

    pub fn disabled_env() -> bool {
        harness_core::config::env_bool("BUDGETGUARD_DISABLE").unwrap_or(false)
    }
}
