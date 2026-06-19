//! Configuration: project `playbook.toml` over `~/.playbook/config.toml` over
//! defaults, with an env kill switch. Retrieval only ever *injects* short notes
//! under a hard char budget — it can never block a turn.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub store_dir: PathBuf,
    /// Max notes injected per prompt.
    pub top_k: usize,
    /// Minimum relevance score for a note to be injected (always-notes bypass).
    pub min_score: i64,
    /// Hard cap on total injected characters (keeps context lean).
    pub max_chars: usize,
    /// Also search the shared `_global` store, not just the project's notes.
    pub include_global: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    store_dir: Option<String>,
    top_k: Option<usize>,
    min_score: Option<i64>,
    max_chars: Option<usize>,
    include_global: Option<bool>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn base_dir() -> PathBuf {
    home().join(".playbook")
}

pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else if s == "~" {
        home()
    } else {
        PathBuf::from(s)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            store_dir: base_dir().join("store"),
            top_k: 3,
            min_score: 5,
            max_chars: 1500,
            include_global: true,
        }
    }
}

impl Config {
    pub fn project_path(root: &Path) -> PathBuf {
        root.join("playbook.toml")
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
                    if let Some(v) = fc.store_dir {
                        cfg.store_dir = expand_tilde(&v);
                    }
                    if let Some(v) = fc.top_k {
                        cfg.top_k = v;
                    }
                    if let Some(v) = fc.min_score {
                        cfg.min_score = v;
                    }
                    if let Some(v) = fc.max_chars {
                        cfg.max_chars = v;
                    }
                    if let Some(v) = fc.include_global {
                        cfg.include_global = v;
                    }
                }
            }
        }
        cfg.top_k = cfg.top_k.max(1);
        cfg.max_chars = cfg.max_chars.max(120);
        cfg
    }

    pub fn disabled_env() -> bool {
        std::env::var("PLAYBOOK_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }
}
