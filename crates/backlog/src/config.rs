use std::path::PathBuf;

use harness_core::config::{base_dir, expand_tilde};
use serde::Deserialize;

pub struct Config {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub inject_limit: usize,
}

#[derive(Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    store_dir: Option<String>,
    inject_limit: Option<usize>,
}

impl Config {
    pub fn load() -> Self {
        let base = base_dir("backlog");
        let mut cfg = Config {
            enabled: true,
            store_dir: base.clone(),
            inject_limit: 4000,
        };
        if let Ok(txt) = std::fs::read_to_string(base.join("config.toml")) {
            if let Ok(fc) = toml::from_str::<FileConfig>(&txt) {
                if let Some(v) = fc.enabled {
                    cfg.enabled = v;
                }
                if let Some(v) = fc.store_dir {
                    cfg.store_dir = expand_tilde(&v);
                }
                if let Some(v) = fc.inject_limit {
                    cfg.inject_limit = v;
                }
            }
        }
        cfg
    }

    pub fn tasks_path(&self) -> PathBuf {
        self.store_dir.join("tasks.toml")
    }

    pub fn disabled_env() -> bool {
        std::env::var("BACKLOG_DISABLE")
            .map(|v| v == "1")
            .unwrap_or(false)
    }
}
