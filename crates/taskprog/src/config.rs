use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// Path to the progress file. Defaults to <cwd>/.claude/progress.md
    pub progress_file: Option<String>,
    /// Max bytes of progress file to inject at SessionStart (0 = no limit).
    pub inject_limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            progress_file: None,
            inject_limit: 4096,
        }
    }
}

impl Config {
    pub fn load(cwd: &str) -> Self {
        // Project-local config wins over home config.
        let project = PathBuf::from(cwd).join("taskprog.toml");
        if project.exists() {
            if let Ok(s) = std::fs::read_to_string(&project) {
                if let Ok(c) = toml::from_str::<Config>(&s) {
                    return c;
                }
            }
        }
        let home_cfg = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".taskprog")
            .join("config.toml");
        if home_cfg.exists() {
            if let Ok(s) = std::fs::read_to_string(&home_cfg) {
                if let Ok(c) = toml::from_str::<Config>(&s) {
                    return c;
                }
            }
        }
        Config::default()
    }

    pub fn resolve_progress_path(&self, cwd: &str) -> PathBuf {
        if let Some(p) = &self.progress_file {
            harness_core::config::expand_tilde(p).into()
        } else {
            PathBuf::from(cwd).join(".claude").join("progress.md")
        }
    }
}

pub fn disabled_env() -> bool {
    std::env::var("TASKPROG_DISABLED").map(|v| v == "1" || v == "true").unwrap_or(false)
}

pub fn init_config(target: &str) -> Result<()> {
    let path = PathBuf::from(target);
    if path.exists() {
        anyhow::bail!("{} already exists", path.display());
    }
    std::fs::write(&path, include_str!("../taskprog.example.toml"))?;
    println!("wrote {}", path.display());
    Ok(())
}
