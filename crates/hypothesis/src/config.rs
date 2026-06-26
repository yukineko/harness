use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

pub struct Config {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub inject_limit: usize,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    store_dir: Option<String>,
    inject_limit: Option<usize>,
}

fn base_dir() -> PathBuf {
    harness_core::config::base_dir("hypothesis")
}

impl Config {
    pub fn load() -> Result<Self> {
        let store_dir = base_dir();
        let mut cfg = Config {
            enabled: true,
            store_dir: store_dir.clone(),
            inject_limit: 2000,
        };

        let config_path = store_dir.join("config.toml");
        if config_path.exists() {
            if let Ok(text) = std::fs::read_to_string(&config_path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled {
                        cfg.enabled = v;
                    }
                    if let Some(v) = fc.store_dir {
                        cfg.store_dir = harness_core::config::expand_tilde(&v);
                    }
                    if let Some(v) = fc.inject_limit {
                        cfg.inject_limit = v;
                    }
                }
            }
        }

        Ok(cfg)
    }

    pub fn hypotheses_path(&self) -> PathBuf {
        self.store_dir.join("hypotheses.toml")
    }

    pub fn disabled_env() -> bool {
        std::env::var("HYPOTHESIS_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }
}
