use anyhow::Result;
use std::path::PathBuf;

pub struct Config {
    pub enabled: bool,
    pub store_dir: PathBuf,
    pub inject_limit: usize,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self {
            enabled: true,
            store_dir: PathBuf::new(),
            inject_limit: 2000,
        })
    }

    pub fn hypotheses_path(&self) -> PathBuf {
        self.store_dir.join("hypotheses.toml")
    }

    pub fn disabled_env() -> bool {
        std::env::var("HYPOTHESIS_DISABLE").is_ok()
    }
}
