use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tunables for retrieval + policy. Loaded from ~/.fugu-router/config.toml.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// Override the episode store path. Default: ~/.fugu-router/episodes.jsonl
    /// When sync_repo is set and this is unset, defaults to <sync_dir>/episodes.jsonl.
    pub store_file: Option<String>,
    /// Override the playbook store path. Default: ~/.fugu-router/playbooks.jsonl
    /// When sync_repo is set and this is unset, defaults to <sync_dir>/playbooks.jsonl.
    pub playbook_file: Option<String>,
    /// GitHub repo URL for syncing records across machines (e.g. https://github.com/you/fugu-router-record).
    pub sync_repo: Option<String>,
    /// Local clone path for sync_repo. Default: ~/.fugu-router/record-repo
    pub sync_dir: Option<String>,
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
            playbook_file: None,
            sync_repo: None,
            sync_dir: None,
            k: 6,
            sim_threshold: 0.15,
            // Bias cheap: a 60% neighbour pass-rate is enough to trust a tier, and
            // a single similar sample is enough to leave the cold-start prior. The
            // verifier's cascade escalation is the safety net that buys back up the
            // few tasks a cheap tier gets wrong, so the bar to *try* cheap is low.
            pass_threshold: 0.6,
            min_samples: 1,
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
            Some(p) => harness_core::config::expand_tilde(p),
            None => match self.sync_repo {
                Some(_) => self.sync_dir_path().join("episodes.jsonl"),
                None => home_dir().join(".fugu-router").join("episodes.jsonl"),
            },
        }
    }

    pub fn playbook_path(&self) -> PathBuf {
        match &self.playbook_file {
            Some(p) => harness_core::config::expand_tilde(p),
            None => match self.sync_repo {
                Some(_) => self.sync_dir_path().join("playbooks.jsonl"),
                None => home_dir().join(".fugu-router").join("playbooks.jsonl"),
            },
        }
    }

    pub fn sync_dir_path(&self) -> PathBuf {
        match &self.sync_dir {
            Some(p) => harness_core::config::expand_tilde(p),
            None => home_dir().join(".fugu-router").join("record-repo"),
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
