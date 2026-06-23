use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tunables for retrieval + policy. Loaded from ~/.fugu-router/config.toml.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// Override the episode store path. Default: ~/.fugu-router/episodes.jsonl
    pub store_file: Option<String>,
    /// Neighbours to retrieve per task.
    pub k: usize,
    /// Minimum similarity for a neighbour to count.
    pub sim_threshold: f64,
    /// Minimum neighbour pass-rate for a model to be eligible.
    pub pass_threshold: f64,
    /// Minimum neighbour samples (for one model) before trusting learned data.
    pub min_samples: usize,
    /// Use Thompson-sampling exploration (true) or the hard threshold rule (false).
    pub explore: bool,
    /// Max bytes of routing summary injected at UserPromptSubmit (0 = no limit).
    pub inject_limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            store_file: None,
            k: 6,
            sim_threshold: 0.15,
            pass_threshold: 0.7,
            min_samples: 2,
            explore: true,
            inject_limit: 1500,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let home_cfg = home_dir().join(".fugu-router").join("config.toml");
        if home_cfg.exists() {
            if let Ok(s) = std::fs::read_to_string(&home_cfg) {
                if let Ok(c) = toml::from_str::<Config>(&s) {
                    return c;
                }
            }
        }
        Config::default()
    }

    pub fn store_path(&self) -> PathBuf {
        match &self.store_file {
            Some(p) => harness_core::config::expand_tilde(p).into(),
            None => home_dir().join(".fugu-router").join("episodes.jsonl"),
        }
    }
}

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn disabled_env() -> bool {
    std::env::var("FUGU_ROUTER_DISABLED")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

pub fn init_config(target: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(target);
    if path.exists() {
        anyhow::bail!("{} already exists", path.display());
    }
    std::fs::write(&path, include_str!("../fugu-router.example.toml"))?;
    println!("wrote {}", path.display());
    Ok(())
}
