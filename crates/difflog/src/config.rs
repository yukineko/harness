//! Config: project `difflog.toml` layered over `~/.difflog/config.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use harness_core::config::expand_tilde;

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    /// Directory where session diff-log files are written.
    log_dir: Option<String>,
    /// Max bytes of `git diff` output to include verbatim. 0 = no diff body.
    diff_body_limit: Option<usize>,
    /// Paths to exclude from the diff (glob patterns, applied to file paths).
    #[serde(default)]
    exclude_globs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub log_dir: PathBuf,
    /// Include a bounded verbatim diff body (0 = stat-only).
    pub diff_body_limit: usize,
    pub exclude_globs: Vec<String>,
}

pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("difflog")
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            log_dir: base_dir().join("logs"),
            diff_body_limit: 4096,
            exclude_globs: vec![
                "Cargo.lock".into(),
                "package-lock.json".into(),
                "*.min.js".into(),
                "*.min.css".into(),
            ],
        }
    }
}

impl Config {
    pub fn load(root: &Path) -> Self {
        let mut cfg = Config::default();
        let path = {
            let p = root.join("difflog.toml");
            if p.exists() {
                Some(p)
            } else {
                let h = base_dir().join("config.toml");
                if h.exists() {
                    Some(h)
                } else {
                    None
                }
            }
        };
        if let Some(path) = path {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled {
                        cfg.enabled = v;
                    }
                    if let Some(v) = fc.log_dir {
                        cfg.log_dir = expand_tilde(&v);
                    }
                    if let Some(v) = fc.diff_body_limit {
                        cfg.diff_body_limit = v;
                    }
                    if !fc.exclude_globs.is_empty() {
                        cfg.exclude_globs = fc.exclude_globs;
                    }
                }
            }
        }
        cfg
    }

    pub fn disabled_env() -> bool {
        harness_core::config::env_bool("DIFFLOG_DISABLE").unwrap_or(false)
    }
}
