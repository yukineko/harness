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
    /// How many seconds a Running task may be silent before being considered STUCK.
    /// Defaults to 1800 (30 minutes).
    pub stuck_ttl_secs: u64,
    /// Loop feature: command to build artifacts before testing (client/e2e cycles).
    pub build_command: Option<String>,
    /// Loop feature: command to deploy before testing (server/e2e cycles).
    pub deploy_command: Option<String>,
    /// Loop feature: max iterations before the loop gives up. Defaults to 10.
    pub loop_max_iters: usize,
    /// Autonomy mode: when true, the /condukt skill downgrades its human gates
    /// (e.g. the Phase 3 decomposition agreement) to deterministic defaults so the
    /// loop can run with no user approval beyond genuinely-needed information.
    /// Defaults to false (every existing AskUserQuestion still fires — fully
    /// backward compatible). Read by `condukt state autonomy-check`.
    pub autonomous: bool,
}

/// Which test-fix cycle sequence to use for a module type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleCycle {
    /// deploy → test
    Server,
    /// build → test
    Client,
    /// build → deploy → test
    E2e,
}

impl ModuleCycle {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "server" => Some(Self::Server),
            "client" => Some(Self::Client),
            "e2e" => Some(Self::E2e),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
            Self::E2e => "e2e",
        }
    }
}

#[derive(Default, Deserialize)]
struct FileTestConfig {
    command: Option<String>,
}

#[derive(Default, Deserialize)]
struct FileLoopConfig {
    build_command: Option<String>,
    deploy_command: Option<String>,
    max_iters: Option<usize>,
}

#[derive(Default, Deserialize)]
struct FileConfig {
    worktree_base: Option<String>,
    default_branch: Option<String>,
    shared_globs: Option<Vec<String>>,
    max_parallel: Option<usize>,
    state_dir: Option<String>,
    test: Option<FileTestConfig>,
    stuck_ttl_secs: Option<u64>,
    autonomous: Option<bool>,
    #[serde(rename = "loop")]
    loop_cfg: Option<FileLoopConfig>,
}

/// Parse the `CONDUKT_AUTONOMOUS` env override. Accepts common truthy/falsy
/// spellings so the switch can be forced either way from the environment
/// (overriding config.toml). Unrecognized values leave config untouched.
fn parse_autonomous_env(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
            stuck_ttl_secs: 1800,
            build_command: None,
            deploy_command: None,
            loop_max_iters: 10,
            autonomous: false,
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
                if let Some(v) = fc.stuck_ttl_secs {
                    cfg.stuck_ttl_secs = v;
                }
                if let Some(v) = fc.autonomous {
                    cfg.autonomous = v;
                }
                if let Some(lc) = fc.loop_cfg {
                    if let Some(v) = lc.build_command {
                        cfg.build_command = Some(v);
                    }
                    if let Some(v) = lc.deploy_command {
                        cfg.deploy_command = Some(v);
                    }
                    if let Some(v) = lc.max_iters {
                        cfg.loop_max_iters = v;
                    }
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
        if let Ok(v) = std::env::var("CONDUKT_STUCK_TTL_SECS") {
            if let Ok(n) = v.parse() {
                cfg.stuck_ttl_secs = n;
            }
        }
        if let Ok(v) = std::env::var("CONDUKT_AUTONOMOUS") {
            if let Some(b) = parse_autonomous_env(&v) {
                cfg.autonomous = b;
            }
        }
        cfg
    }

    /// Global kill switch for the hooks (`CONDUKT_DISABLE=1`).
    pub fn disabled() -> bool {
        std::env::var("CONDUKT_DISABLE")
            .map(|v| v == "1")
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_cycle_roundtrip() {
        for (s, expected) in [
            ("server", ModuleCycle::Server),
            ("client", ModuleCycle::Client),
            ("e2e", ModuleCycle::E2e),
        ] {
            let got = ModuleCycle::from_str(s).expect("should parse");
            assert_eq!(got, expected);
            assert_eq!(got.as_str(), s);
        }
    }

    #[test]
    fn module_cycle_unknown_returns_none() {
        assert!(ModuleCycle::from_str("unknown").is_none());
        assert!(ModuleCycle::from_str("").is_none());
    }

    #[test]
    fn loop_config_parses_from_toml() {
        let toml = r#"
[loop]
build_command = "npm run build"
deploy_command = "kubectl rollout restart deployment/api"
max_iters = 5
"#;
        let fc: FileConfig = toml::from_str(toml).expect("should parse");
        let lc = fc.loop_cfg.expect("loop section present");
        assert_eq!(lc.build_command.as_deref(), Some("npm run build"));
        assert_eq!(
            lc.deploy_command.as_deref(),
            Some("kubectl rollout restart deployment/api")
        );
        assert_eq!(lc.max_iters, Some(5));
    }

    #[test]
    fn loop_config_defaults_when_absent() {
        let toml = "";
        let fc: FileConfig = toml::from_str(toml).expect("should parse");
        assert!(fc.loop_cfg.is_none());
    }

    #[test]
    fn autonomous_parses_from_toml() {
        let fc: FileConfig = toml::from_str("autonomous = true").expect("should parse");
        assert_eq!(fc.autonomous, Some(true));
    }

    #[test]
    fn autonomous_defaults_none_when_absent() {
        let fc: FileConfig = toml::from_str("").expect("should parse");
        assert_eq!(fc.autonomous, None);
    }

    #[test]
    fn autonomous_env_truthy_and_falsy() {
        for v in ["1", "true", "TRUE", "yes", "on", " True "] {
            assert_eq!(parse_autonomous_env(v), Some(true), "{v:?} should be true");
        }
        for v in ["0", "false", "No", "off"] {
            assert_eq!(parse_autonomous_env(v), Some(false), "{v:?} should be false");
        }
        for v in ["", "maybe", "2", "enabled"] {
            assert_eq!(parse_autonomous_env(v), None, "{v:?} should be None");
        }
    }
}
