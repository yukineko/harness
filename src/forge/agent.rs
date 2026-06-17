//! Invoke the normalize agent — one process, prompt on stdin, draft on stdout.
//!
//! Mirrors specguard's agent runner (a prompt is streamed in on a dedicated
//! thread so a large stdout can drain concurrently without a pipe deadlock).
//! Normalize is a single shard for now; parallel fan-out (⑤) comes later.

use crate::config::AgentConfig;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

pub struct AgentOutput {
    pub stdout: String,
    pub stderr: String,
    /// Process exit code (-1 if terminated by a signal or never spawned).
    pub code: i32,
}

/// Run one agent process feeding `prompt` on stdin. Spawn/exec failures fold
/// into `AgentOutput { code: -1 }` so the caller's exit handling is uniform.
pub fn run(cfg: &AgentConfig, repo_root: &std::path::Path, prompt: &str) -> AgentOutput {
    let child = Command::new(&cfg.command)
        .args(&cfg.args)
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return AgentOutput {
                stdout: String::new(),
                stderr: format!("spawning agent '{}': {e}", cfg.command),
                code: -1,
            };
        }
    };

    let stdin = child.stdin.take();
    let prompt_owned = prompt.to_string();
    let writer = thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(prompt_owned.as_bytes());
        }
    });

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            let _ = writer.join();
            return AgentOutput {
                stdout: String::new(),
                stderr: format!("waiting for agent: {e}"),
                code: -1,
            };
        }
    };
    let _ = writer.join();

    AgentOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}
