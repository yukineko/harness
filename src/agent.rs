//! Invoke the read-only auditing agent.
//!
//! The agent is any command that reads a prompt on stdin and writes its report
//! to stdout (the Claude Code CLI in `--print` mode by default). We stream the
//! prompt in on a separate thread so a large stdout can drain concurrently,
//! avoiding a pipe-buffer deadlock.

use crate::config::AgentConfig;
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

pub struct AgentOutput {
    pub stdout: String,
    pub stderr: String,
    /// Process exit code (or -1 if terminated by a signal).
    pub code: i32,
}

/// Run the agent in `repo_root`, feeding `prompt` on stdin.
pub fn run(cfg: &AgentConfig, repo_root: &std::path::Path, prompt: &str) -> Result<AgentOutput> {
    let mut child = Command::new(&cfg.command)
        .args(&cfg.args)
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning agent '{}'", cfg.command))?;

    // Write the prompt on a dedicated thread; if writing fails (e.g. the agent
    // closes stdin early) we ignore it and let the exit code carry the failure.
    let mut stdin = child.stdin.take().context("agent stdin unavailable")?;
    let prompt_owned = prompt.to_string();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(prompt_owned.as_bytes());
        // stdin dropped here -> EOF for the child.
    });

    let output = child.wait_with_output().context("waiting for agent")?;
    let _ = writer.join();

    Ok(AgentOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    })
}
