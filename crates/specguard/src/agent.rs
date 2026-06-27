//! Invoke the read-only auditing agent — one process per audit shard.
//!
//! The agent is any command that reads a prompt on stdin and writes its report
//! to stdout (the Claude Code CLI in `--print` mode by default). Each shard
//! (one in-scope area, or the invariant set) is audited by its OWN agent process
//! with a fresh context, so a large multi-area run never accumulates unrelated
//! files into a single context window (context-rot mitigation). Shards run
//! concurrently up to [`MAX_PARALLEL`].
//!
//! For each process we stream the prompt in on a separate thread so a large
//! stdout can drain concurrently, avoiding a pipe-buffer deadlock.

use crate::config::AgentConfig;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;

/// Cap on concurrent agent processes. Each `claude` call is heavy, so we bound
/// fan-out rather than spawning one process per area unconditionally.
pub const MAX_PARALLEL: usize = 4;

pub struct AgentOutput {
    pub stdout: String,
    pub stderr: String,
    /// Process exit code (-1 if terminated by a signal or never spawned).
    pub code: i32,
}

/// One shard's prompt plus the label used for it in logs and the merged report.
pub struct ShardPrompt {
    pub label: String,
    pub prompt: String,
}

/// Result of auditing one shard.
pub struct ShardOutput {
    pub label: String,
    pub out: AgentOutput,
}

/// Audit every shard, at most [`MAX_PARALLEL`] processes at a time. Never errors:
/// a spawn/exec failure for a shard is captured as an [`AgentOutput`] with
/// `code = -1` and the error on stderr, so the caller's exit-code aggregation
/// treats it as an agent failure like any other non-zero exit.
pub fn run_shards(
    cfg: &AgentConfig,
    repo_root: &std::path::Path,
    shards: Vec<ShardPrompt>,
) -> Vec<ShardOutput> {
    let n = shards.len();
    let next = Mutex::new(0usize);
    let results: Mutex<Vec<Option<ShardOutput>>> = Mutex::new((0..n).map(|_| None).collect());
    let workers = MAX_PARALLEL.min(n).max(1);

    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let idx = {
                    let mut g = next.lock().unwrap();
                    if *g >= n {
                        break;
                    }
                    let i = *g;
                    *g += 1;
                    i
                };
                let shard = &shards[idx];
                let out = run_one(cfg, repo_root, &shard.prompt);
                results.lock().unwrap()[idx] = Some(ShardOutput {
                    label: shard.label.clone(),
                    out,
                });
            });
        }
    });

    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|o| o.expect("every shard slot is filled"))
        .collect()
}

/// Run one agent process, feeding `prompt` on stdin. Spawn/exec failures are
/// folded into an `AgentOutput { code: -1 }` so fan-out never aborts midway.
fn run_one(cfg: &AgentConfig, repo_root: &std::path::Path, prompt: &str) -> AgentOutput {
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

    // Write the prompt on a dedicated thread; if writing fails (e.g. the agent
    // closes stdin early) we ignore it and let the exit code carry the failure.
    let stdin = child.stdin.take();
    let prompt_owned = prompt.to_string();
    let writer = thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(prompt_owned.as_bytes());
            // stdin dropped here -> EOF for the child.
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
