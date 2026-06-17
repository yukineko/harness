//! Configuration model for specforge (the generation-side harness).
//!
//! Like specguard, everything project-specific lives in a TOML file so the
//! binary stays generic. The normalize agent is read-only (it reads the
//! requirement + canon and emits a draft), so it defaults to the SAME enforced
//! read-only allowlist as specguard — writes happen in the harness, not the
//! agent (DESIGN.md §1, §7).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub project: Project,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    #[serde(default = "default_dot")]
    pub root: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for AgentConfig {
    /// Read-only by default, enforced by the harness (not just requested): in
    /// `--print` mode any tool outside the allowlist is auto-denied, so the
    /// normalize agent cannot write/exec even if a prompt-injected requirement
    /// tries. The draft is produced on stdout; the harness persists it.
    fn default() -> Self {
        AgentConfig {
            command: "claude".to_string(),
            args: DEFAULT_AGENT_ARGS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Read-only agent argv — mirrors specguard's (single source of truth here).
pub const DEFAULT_AGENT_ARGS: &[&str] = &[
    "--print",
    "--allowedTools",
    "Read",
    "Glob",
    "Grep",
    "Bash(git diff *)",
    "Bash(git log *)",
    "Bash(git show *)",
    "Bash(git status *)",
    "--disallowedTools",
    "Edit",
    "Write",
    "NotebookEdit",
    "WebFetch",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// Directory (relative to repo root) holding `<id>.toml` Spec IR drafts.
    #[serde(default = "default_spec_dir")]
    pub spec_dir: String,
    /// Sentinel raised on a rigor-gate escalation (insufficiency / conflict),
    /// so a SessionStart hook can pull the human in (HOTL).
    #[serde(default = "default_sentinel")]
    pub sentinel: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig {
            spec_dir: default_spec_dir(),
            sentinel: default_sentinel(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PromptConfig {
    /// Path (relative to the config file) to a custom normalize template.
    /// Empty → embedded default.
    #[serde(default)]
    pub normalize_template: String,
}

fn default_dot() -> String {
    ".".to_string()
}
fn default_spec_dir() -> String {
    "specs".to_string()
}
fn default_sentinel() -> String {
    ".specforge-pending".to_string()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.project.name.trim().is_empty() {
            anyhow::bail!("project.name must not be empty");
        }
        if self.agent.command.trim().is_empty() {
            anyhow::bail!("agent.command must not be empty");
        }
        Ok(())
    }
}
