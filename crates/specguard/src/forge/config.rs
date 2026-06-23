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
    /// ① intake sources (DESIGN-INTAKE.md §3, §7).
    #[serde(default)]
    pub sources: SourcesConfig,
    /// ① gather knobs (DESIGN-INTAKE.md §7).
    #[serde(default)]
    pub gather: GatherConfig,
    /// graded rigor gate (DESIGN-INTAKE.md §4, §7). Fields are accepted now so a
    /// config can declare them; the rigor LOGIC is a later slice (not wired yet).
    #[serde(default)]
    #[allow(dead_code)] // wired by a later (pre-flight/interrogate) slice.
    pub rigor: RigorConfig,
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
    /// Directory for rendered impl prompts (`<spec_id>-<req_id>.prompt.md`).
    #[serde(default = "default_impl_prompt_dir")]
    pub impl_prompt_dir: String,
    /// Directory for impl results (`<spec_id>-impl.json`) and evidence gate.
    #[serde(default = "default_impl_dir")]
    pub impl_dir: String,
    /// Base directory for git worktrees created during ⑤ parallel-impl.
    #[serde(default = "default_worktree_base")]
    pub worktree_base: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig {
            spec_dir: default_spec_dir(),
            sentinel: default_sentinel(),
            impl_prompt_dir: default_impl_prompt_dir(),
            impl_dir: default_impl_dir(),
            worktree_base: default_worktree_base(),
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
    /// Path (relative to the config file) to a custom impl prompt template.
    /// Empty → embedded default.
    #[serde(default)]
    pub impl_template: String,
}

/// ① intake sources (DESIGN-INTAKE.md §3 table, §7 `[sources]`). authority is a
/// *tiebreak advisory order* (high→low) — it is NEVER used to auto-resolve
/// conflicts or drop fragments (DESIGN-INTAKE.md §3.1 / principle 5).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    /// `<vault>` whose `AEGIS/{decisions,sessions}` are walked (authority High).
    /// Empty → Obsidian source skipped.
    #[serde(default)]
    pub obsidian_vault: String,
    /// repo doc globs (authority Mid).
    #[serde(default = "default_canon")]
    pub canon: Vec<String>,
    /// Claude Code transcripts root (authority Low); enc-cwd is auto-resolved.
    /// Empty → past-prompt source skipped.
    #[serde(default = "default_transcripts")]
    pub transcripts: String,
    /// Advisory tiebreak order (high→low). Informational; not used to drop.
    /// Consumed by the later interrogate slice (default order is enforced in
    /// gather by the [`crate::gather::Authority`] enum itself, DESIGN §3.1).
    #[serde(default = "default_authority")]
    #[allow(dead_code)]
    pub authority: Vec<String>,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        SourcesConfig {
            obsidian_vault: String::new(),
            canon: default_canon(),
            transcripts: default_transcripts(),
            authority: default_authority(),
        }
    }
}

/// ① gather knobs (DESIGN-INTAKE.md §7 `[gather]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatherConfig {
    /// Max fragments in the bundle.
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Drop fragments with relevance below this (relevance only, never authority).
    #[serde(default = "default_min_score")]
    pub min_score: i64,
}

impl Default for GatherConfig {
    fn default() -> Self {
        GatherConfig {
            top_k: default_top_k(),
            min_score: default_min_score(),
        }
    }
}

/// graded rigor gate (DESIGN-INTAKE.md §4, §7 `[rigor]`). Declared for forward
/// compatibility; the gather slice does NOT implement the rigor logic.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // fields wired by a later (pre-flight/interrogate) slice.
pub struct RigorConfig {
    #[serde(default = "default_true")]
    pub require_acceptance: bool,
    #[serde(default = "default_true")]
    pub require_canon_citation: bool,
    #[serde(default = "default_true")]
    pub run_d2_audit: bool,
    #[serde(default = "default_max_rounds")]
    pub max_interrogation_rounds: u32,
    #[serde(default)]
    pub record_decisions: bool,
}

impl Default for RigorConfig {
    fn default() -> Self {
        RigorConfig {
            require_acceptance: true,
            require_canon_citation: true,
            run_d2_audit: true,
            max_interrogation_rounds: default_max_rounds(),
            record_decisions: false,
        }
    }
}

fn default_canon() -> Vec<String> {
    vec!["docs/**/*.md".to_string()]
}
fn default_transcripts() -> String {
    "~/.claude/projects".to_string()
}
fn default_authority() -> Vec<String> {
    vec!["obsidian".to_string(), "canon".to_string(), "prompt".to_string()]
}
fn default_top_k() -> usize {
    24
}
fn default_min_score() -> i64 {
    1
}
fn default_true() -> bool {
    true
}
fn default_max_rounds() -> u32 {
    4
}
fn default_dot() -> String {
    ".".to_string()
}
fn default_impl_prompt_dir() -> String {
    "specs/prompts".to_string()
}
fn default_impl_dir() -> String {
    "specs/impl".to_string()
}
fn default_worktree_base() -> String {
    ".specforge-worktrees".to_string()
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
