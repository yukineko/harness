use std::path::PathBuf;

use harness_core::config::{base_dir, expand_tilde};
use serde::Deserialize;

pub struct Config {
    pub enabled: bool,
    /// Minimum completed turns before triggering the record prompt.
    pub min_turns: u64,
    /// Minimum total tool events before triggering the record prompt.
    pub min_tool_events: u64,
    pub state_dir: PathBuf,
}

#[derive(Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    min_turns: Option<u64>,
    min_tool_events: Option<u64>,
    state_dir: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let base = base_dir("autoflow");
        let mut cfg = Config {
            enabled: true,
            min_turns: 2,
            min_tool_events: 3,
            state_dir: base.join("state"),
        };
        if let Ok(txt) = std::fs::read_to_string(base.join("config.toml")) {
            if let Ok(fc) = toml::from_str::<FileConfig>(&txt) {
                if let Some(v) = fc.enabled {
                    cfg.enabled = v;
                }
                if let Some(v) = fc.min_turns {
                    cfg.min_turns = v;
                }
                if let Some(v) = fc.min_tool_events {
                    cfg.min_tool_events = v;
                }
                if let Some(v) = fc.state_dir {
                    cfg.state_dir = expand_tilde(&v);
                }
            }
        }
        cfg
    }

    pub fn disabled_env() -> bool {
        std::env::var("AUTOFLOW_DISABLE")
            .map(|v| v == "1")
            .unwrap_or(false)
    }
}
