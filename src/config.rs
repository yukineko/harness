//! Configuration: project `donegate.toml` (preferred) layered over a home-level
//! `~/.donegate/config.toml`, over built-in defaults. Env overrides last.
//!
//! Safe by default: with no config and no checks, the gate runs nothing and lets
//! every stop through — installing the hook can never *block* a project that
//! hasn't opted in with at least one `[[check]]`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One acceptance command, run as a subprocess on Stop.
#[derive(Debug, Clone, Deserialize)]
pub struct Check {
    /// Short label shown in the block reason (e.g. "test", "clippy").
    pub name: String,
    /// Shell command line; run via `sh -c` (Unix) / `cmd /C` (Windows).
    pub cmd: String,
    /// If set, the check runs only when a changed file (git diff vs HEAD +
    /// untracked) matches one of these globs. Absent ⇒ always run.
    #[serde(default)]
    pub when_changed: Option<Vec<String>>,
    /// Per-check timeout; falls back to `default_timeout_secs`.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// A failing optional check warns but never blocks the stop.
    #[serde(default)]
    pub optional: bool,
    /// Run the command in this subdir of the project root.
    #[serde(default)]
    pub workdir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// After this many consecutive blocks in one session, give up and allow the
    /// stop (so a genuinely stuck agent isn't trapped forever).
    pub max_attempts: u32,
    pub default_timeout_secs: u64,
    /// How many trailing lines of a failing command's output to feed back.
    pub output_tail_lines: usize,
    /// A session's attempt counter resets if this many seconds pass between
    /// stops (a fresh turn after the user did other work).
    pub reset_after_secs: i64,
    pub state_dir: PathBuf,
    pub checks: Vec<Check>,
}

/// On-disk form; every field optional.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    max_attempts: Option<u32>,
    default_timeout_secs: Option<u64>,
    output_tail_lines: Option<usize>,
    reset_after_secs: Option<i64>,
    state_dir: Option<String>,
    #[serde(default)]
    check: Vec<Check>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn base_dir() -> PathBuf {
    home().join(".donegate")
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
            max_attempts: 3,
            default_timeout_secs: 300,
            output_tail_lines: 40,
            reset_after_secs: 600,
            state_dir: base_dir().join("state"),
            checks: Vec::new(),
        }
    }
}

impl Config {
    /// The project config path (`<root>/donegate.toml`).
    pub fn project_path(root: &Path) -> PathBuf {
        root.join("donegate.toml")
    }

    /// The home config path (`~/.donegate/config.toml`).
    pub fn home_path() -> PathBuf {
        base_dir().join("config.toml")
    }

    /// Load config for a project root. A project `donegate.toml` wins outright;
    /// otherwise the home config; otherwise built-in defaults. Any parse error
    /// silently falls back (the gate must never crash a turn).
    pub fn load(root: &Path) -> Self {
        let mut cfg = Config::default();

        let chosen = {
            let p = Config::project_path(root);
            if p.exists() {
                Some(p)
            } else {
                let h = Config::home_path();
                if h.exists() {
                    Some(h)
                } else {
                    None
                }
            }
        };

        if let Some(path) = chosen {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled {
                        cfg.enabled = v;
                    }
                    if let Some(v) = fc.max_attempts {
                        cfg.max_attempts = v;
                    }
                    if let Some(v) = fc.default_timeout_secs {
                        cfg.default_timeout_secs = v;
                    }
                    if let Some(v) = fc.output_tail_lines {
                        cfg.output_tail_lines = v;
                    }
                    if let Some(v) = fc.reset_after_secs {
                        cfg.reset_after_secs = v;
                    }
                    if let Some(v) = fc.state_dir {
                        cfg.state_dir = expand_tilde(&v);
                    }
                    cfg.checks = fc.check;
                }
            }
        }

        // sanitize
        if cfg.max_attempts == 0 {
            cfg.max_attempts = 1;
        }
        if cfg.default_timeout_secs == 0 {
            cfg.default_timeout_secs = 300;
        }
        if cfg.output_tail_lines == 0 {
            cfg.output_tail_lines = 40;
        }
        cfg.checks
            .retain(|c| !c.name.trim().is_empty() && !c.cmd.trim().is_empty());
        cfg
    }

    /// Globally disabled via env.
    pub fn disabled_env() -> bool {
        std::env::var("DONEGATE_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }
}
