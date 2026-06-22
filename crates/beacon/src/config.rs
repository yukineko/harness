//! Configuration: project `beacon.toml` (preferred) over a home-level
//! `~/.beacon/config.toml` over built-in defaults. The first file that exists
//! wins (the layers are not merged), matching the rest of the toolkit.
//!
//! Secrets (webhook URLs) may also come from the environment so they need not
//! be committed: `BEACON_SLACK_WEBHOOK` and `BEACON_WEBHOOK` override the file.

use std::path::{Path, PathBuf};

use serde::Deserialize;

// Re-exported so existing `crate::config::expand_tilde` call sites keep working.
pub use harness_core::config::expand_tilde;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// Notify when a turn finishes (Stop hook).
    pub on_stop: bool,
    /// Notify when Claude Code raises a Notification (waiting for input/permission).
    pub on_notification: bool,
    /// On Stop, append a short tail of Claude's last message to the body.
    pub include_snippet: bool,
    /// Max characters of that snippet.
    pub snippet_chars: usize,

    // --- channels ---
    /// macOS `osascript` / Linux `notify-send` desktop notification.
    pub desktop: bool,
    /// macOS notification sound name (e.g. "Glass", "Ping"); None = silent.
    pub sound: Option<String>,
    /// Slack incoming-webhook URL (env `BEACON_SLACK_WEBHOOK` overrides).
    pub slack_webhook: Option<String>,
    /// Generic webhook URL that receives a JSON payload (env `BEACON_WEBHOOK`).
    pub webhook: Option<String>,
    /// Escape-hatch command run via `sh -c` with BEACON_* env vars set.
    pub command: Option<String>,

    /// Append each notification to `<state_dir>/log.jsonl`.
    pub log: bool,
    pub state_dir: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    on_stop: Option<bool>,
    on_notification: Option<bool>,
    include_snippet: Option<bool>,
    snippet_chars: Option<usize>,
    desktop: Option<bool>,
    sound: Option<String>,
    slack_webhook: Option<String>,
    webhook: Option<String>,
    command: Option<String>,
    log: Option<bool>,
    state_dir: Option<String>,
}

pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("beacon")
}

/// Treat empty strings as "unset" so a blank TOML value disables the channel.
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            on_stop: true,
            on_notification: true,
            include_snippet: true,
            snippet_chars: 160,
            desktop: true,
            sound: None,
            slack_webhook: None,
            webhook: None,
            command: None,
            log: true,
            state_dir: base_dir().join("state"),
        }
    }
}

impl Config {
    pub fn project_path(root: &Path) -> PathBuf {
        root.join("beacon.toml")
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
                    if let Some(v) = fc.on_stop {
                        cfg.on_stop = v;
                    }
                    if let Some(v) = fc.on_notification {
                        cfg.on_notification = v;
                    }
                    if let Some(v) = fc.include_snippet {
                        cfg.include_snippet = v;
                    }
                    if let Some(v) = fc.snippet_chars {
                        cfg.snippet_chars = v;
                    }
                    if let Some(v) = fc.desktop {
                        cfg.desktop = v;
                    }
                    cfg.sound = non_empty(fc.sound);
                    cfg.slack_webhook = non_empty(fc.slack_webhook);
                    cfg.webhook = non_empty(fc.webhook);
                    cfg.command = non_empty(fc.command);
                    if let Some(v) = fc.log {
                        cfg.log = v;
                    }
                    if let Some(v) = fc.state_dir {
                        cfg.state_dir = expand_tilde(&v);
                    }
                }
            }
        }
        // Environment overrides for secrets win over the file.
        if let Some(v) = env_non_empty("BEACON_SLACK_WEBHOOK") {
            cfg.slack_webhook = Some(v);
        }
        if let Some(v) = env_non_empty("BEACON_WEBHOOK") {
            cfg.webhook = Some(v);
        }
        // sanitize
        cfg.snippet_chars = cfg.snippet_chars.clamp(20, 1000);
        cfg
    }

    pub fn disabled_env() -> bool {
        std::env::var("BEACON_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }

    /// True when at least one delivery channel is configured.
    pub fn any_channel(&self) -> bool {
        self.desktop
            || self.slack_webhook.is_some()
            || self.webhook.is_some()
            || self.command.is_some()
    }
}
