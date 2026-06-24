//! Runtime configuration: defaults <- ~/.condukt/config.toml <- environment.
//!
//! Everything is generic and project-agnostic. The AEGIS-specific notion of
//! "never parallelize these shared files" lives entirely in `shared_globs`, so
//! a user configures it per project rather than us hardcoding model.py/glossary.

use serde::Deserialize;
use std::path::PathBuf;

// Re-exported so existing `crate::config::expand_tilde` call sites keep working.
pub use harness_core::config::expand_tilde;

pub struct Config {
    /// Where worktrees are created (must be outside the repo).
    pub worktree_base: PathBuf,
    /// Branch to merge completed work back into.
    pub default_branch: String,
    /// Globs that force a touching task to run serially (never in parallel).
    pub shared_globs: Vec<String>,
    /// Soft cap on concurrent workers (advisory; the skill honors it).
    pub max_parallel: usize,
    /// Where run-state files are stored.
    pub state_dir: PathBuf,
    /// Override command for `condukt state test` (None = auto-detect).
    pub test_command: Option<String>,
}

#[derive(Default, Deserialize)]
struct FileTestConfig {
    command: Option<String>,
}

#[derive(Default, Deserialize)]
struct FileConfig {
    worktree_base: Option<String>,
    default_branch: Option<String>,
    shared_globs: Option<Vec<String>>,
    max_parallel: Option<usize>,
    state_dir: Option<String>,
    test: Option<FileTestConfig>,
}

/// `~/.condukt` (falls back to `./.condukt` if there is no home dir). Thin
/// wrapper over the shared base-dir resolution.
pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("condukt")
}

impl Config {
    pub fn load() -> Self {
        let base = base_dir();
        let mut cfg = Config {
            worktree_base: base.join("worktrees"),
            default_branch: "main".to_string(),
            shared_globs: Vec::new(),
            max_parallel: 4,
            state_dir: base.join("state"),
            test_command: None,
        };

        if let Ok(txt) = std::fs::read_to_string(base.join("config.toml")) {
            if let Ok(fc) = toml::from_str::<FileConfig>(&txt) {
                if let Some(v) = fc.worktree_base {
                    cfg.worktree_base = expand_tilde(&v);
                }
                if let Some(v) = fc.default_branch {
                    cfg.default_branch = v;
                }
                if let Some(v) = fc.shared_globs {
                    cfg.shared_globs = v;
                }
                if let Some(v) = fc.max_parallel {
                    cfg.max_parallel = v;
                }
                if let Some(v) = fc.state_dir {
                    cfg.state_dir = expand_tilde(&v);
                }
                if let Some(t) = fc.test {
                    cfg.test_command = t.command;
                }
            }
        }

        if let Ok(v) = std::env::var("CONDUKT_WORKTREE_BASE") {
            cfg.worktree_base = expand_tilde(&v);
        }
        if let Ok(v) = std::env::var("CONDUKT_DEFAULT_BRANCH") {
            cfg.default_branch = v;
        }
        if let Ok(v) = std::env::var("CONDUKT_MAX_PARALLEL") {
            if let Ok(n) = v.parse() {
                cfg.max_parallel = n;
            }
        }
        cfg
    }

    /// Global kill switch for the hooks (`CONDUKT_DISABLE=1`).
    pub fn disabled() -> bool {
        std::env::var("CONDUKT_DISABLE").map(|v| v == "1").unwrap_or(false)
    }
}
