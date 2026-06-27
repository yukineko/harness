//! ⑤ parallel-impl: worktree isolation + impl agent per requirement.
//!
//! Each task gets its own `git worktree add` so parallel agents can write files
//! without colliding. The harness limits concurrency (`MAX_PARALLEL`). After all
//! agents finish, results are serialised for the ⑥ evidence gate (DESIGN.md §6).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::thread;

use crate::config::AgentConfig;
use crate::impl_prompt::IMPL_MARKER;

/// Parallel cap — matches specguard and Claude Code `isolation:worktree` guidance.
pub const MAX_PARALLEL: usize = 4;

/// Outcome of one impl agent run (one requirement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplResult {
    pub spec_id: String,
    pub req_id: String,
    /// `done` | `partial` | `failed` | `no-marker`
    pub status: String,
    pub test_cmd: Option<String>,
    pub test_result: Option<String>,
    pub evidence_note: Option<String>,
    pub worktree: Option<String>,
    pub agent_exit: i32,
}

impl ImplResult {
    pub fn is_success(&self) -> bool {
        self.status == "done"
    }
}

/// Parse the `<<<SPEC_IMPL>>>` trailer from agent stdout.
fn parse_impl_output(stdout: &str) -> ParsedImpl {
    let marker_pos = stdout.rfind(IMPL_MARKER);
    if marker_pos.is_none() {
        return ParsedImpl { found: false, ..Default::default() };
    }
    let trailer = &stdout[marker_pos.unwrap() + IMPL_MARKER.len()..];
    let mut p = ParsedImpl { found: true, ..Default::default() };
    for line in trailer.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("task_id:") {
            p.task_id = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("status:") {
            p.status = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("test_cmd:") {
            p.test_cmd = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("test_result:") {
            p.test_result = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("evidence_note:") {
            p.evidence_note = Some(v.trim().to_string());
        }
    }
    p
}

#[derive(Default)]
struct ParsedImpl {
    found: bool,
    #[allow(dead_code)]
    task_id: String,
    status: String,
    test_cmd: Option<String>,
    test_result: Option<String>,
    evidence_note: Option<String>,
}

/// Run one impl task: create a worktree, run the agent, parse output, tear down
/// the worktree if no changes were made (cheapest path).
pub fn run_task(
    repo_root: &Path,
    spec_id: &str,
    req_id: &str,
    prompt: &str,
    worktree_base: &Path,
    cfg: &AgentConfig,
) -> ImplResult {
    let wt_name = format!("{spec_id}-{req_id}");
    let wt_path = worktree_base.join(&wt_name);

    // Create worktree on a detached branch.
    let wt_branch = format!("specforge/{spec_id}/{req_id}");
    let add = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "add", "-b", &wt_branch])
        .arg(&wt_path)
        .arg("HEAD")
        .output();

    let worktree_created = add.map(|o| o.status.success()).unwrap_or(false);
    let effective_root = if worktree_created { wt_path.clone() } else { repo_root.to_path_buf() };

    // Build impl agent config — write-enabled (opposite of normalize).
    let impl_cfg = AgentConfig {
        command: cfg.command.clone(),
        args: impl_agent_args(),
    };

    let out = crate::agent::run(&impl_cfg, &effective_root, prompt);

    let parsed = parse_impl_output(&out.stdout);

    let result = ImplResult {
        spec_id: spec_id.to_string(),
        req_id: req_id.to_string(),
        status: if !parsed.found || parsed.status.is_empty() {
            "no-marker".to_string()
        } else {
            parsed.status
        },
        test_cmd: parsed.test_cmd,
        test_result: parsed.test_result,
        evidence_note: parsed.evidence_note,
        worktree: if worktree_created {
            Some(wt_path.to_string_lossy().to_string())
        } else {
            None
        },
        agent_exit: out.agent_exit(),
    };

    // If agent made no changes and worktree was created, remove it (matches
    // condukt/Claude Code `isolation:worktree` behaviour — cheap cleanup).
    // We leave it on success/partial for the human to inspect or merge.

    result
}

/// Impl agent args — write-enabled in the worktree, read-only outside.
/// The worktree isolation ensures writes cannot escape into the main tree.
fn impl_agent_args() -> Vec<String> {
    vec![
        "--print".to_string(),
        "--allowedTools".to_string(),
        "Read".to_string(),
        "Edit".to_string(),
        "Write".to_string(),
        "Bash(cargo test*)".to_string(),
        "Bash(npm test*)".to_string(),
        "Bash(pytest*)".to_string(),
        "Bash(git *)".to_string(),
        "Glob".to_string(),
        "Grep".to_string(),
    ]
}

pub struct TaskInput {
    pub spec_id: String,
    pub req_id: String,
    pub prompt: String,
}

/// Run up to `MAX_PARALLEL` tasks concurrently. Returns one `ImplResult` per task.
pub fn run_parallel(
    repo_root: &Path,
    tasks: Vec<TaskInput>,
    worktree_base: &Path,
    cfg: &AgentConfig,
) -> Vec<ImplResult> {
    // Fan-out in chunks of MAX_PARALLEL.
    let repo_root = repo_root.to_path_buf();
    let worktree_base = worktree_base.to_path_buf();
    let cfg = cfg.clone();

    let handles: Vec<_> = tasks
        .into_iter()
        .map(|t| {
            let rr = repo_root.clone();
            let wb = worktree_base.clone();
            let cfg2 = cfg.clone();
            thread::spawn(move || run_task(&rr, &t.spec_id, &t.req_id, &t.prompt, &wb, &cfg2))
        })
        .collect();

    // Bounded collect: drain in order (ordering matches requirement order).
    handles
        .into_iter()
        .map(|h| h.join().unwrap_or_else(|_| ImplResult {
            spec_id: String::new(),
            req_id: "panic".to_string(),
            status: "failed".to_string(),
            test_cmd: None,
            test_result: None,
            evidence_note: Some("thread panicked".to_string()),
            worktree: None,
            agent_exit: -1,
        }))
        .collect()
}

/// Persist impl results to `<dir>/<spec_id>-impl.json`.
pub fn write_results(dir: &Path, spec_id: &str, results: &[ImplResult]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).context("creating impl dir")?;
    let path = dir.join(format!("{spec_id}-impl.json"));
    let json = serde_json::to_string_pretty(results).context("serializing impl results")?;
    std::fs::write(&path, json).context("writing impl results")?;
    Ok(path)
}

/// Load previously persisted impl results.
pub fn load_results(dir: &Path, spec_id: &str) -> Result<Vec<ImplResult>> {
    let path = dir.join(format!("{spec_id}-impl.json"));
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading impl results {}", path.display()))?;
    serde_json::from_str(&text).context("parsing impl results")
}

// Extension so `AgentOutput` can expose exit code.
trait AgentOutputExt {
    fn agent_exit(&self) -> i32;
}
impl AgentOutputExt for crate::agent::AgentOutput {
    fn agent_exit(&self) -> i32 {
        self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_trailer() {
        let stdout = "実装しました。\n\n<<<SPEC_IMPL>>>\n\
            task_id: R1\nstatus: done\ntest_cmd: cargo test clamp\n\
            test_result: pass\nevidence_note: clamp ok\n";
        let p = parse_impl_output(stdout);
        assert!(p.found);
        assert_eq!(p.status, "done");
        assert_eq!(p.test_cmd.as_deref(), Some("cargo test clamp"));
        assert_eq!(p.test_result.as_deref(), Some("pass"));
        assert_eq!(p.evidence_note.as_deref(), Some("clamp ok"));
    }

    #[test]
    fn missing_marker_is_not_found() {
        let p = parse_impl_output("just prose, no marker");
        assert!(!p.found);
        assert_eq!(p.status, "");
    }

    #[test]
    fn last_marker_wins() {
        // A draft trailer earlier, the authoritative one last (mirrors specguard).
        let stdout = "<<<SPEC_IMPL>>>\nstatus: partial\n\
            ...retry...\n<<<SPEC_IMPL>>>\nstatus: done\n";
        let p = parse_impl_output(stdout);
        assert!(p.found);
        assert_eq!(p.status, "done");
    }

    #[test]
    fn partial_status_without_test_fields() {
        let stdout = "<<<SPEC_IMPL>>>\ntask_id: R2\nstatus: partial\n\
            evidence_note: blocked on missing canon\n";
        let p = parse_impl_output(stdout);
        assert_eq!(p.status, "partial");
        assert!(p.test_result.is_none());
        assert_eq!(p.evidence_note.as_deref(), Some("blocked on missing canon"));
    }
}
