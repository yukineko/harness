//! flow — the unified source→executor driver (autopilot layer).
//!
//! Mental model (the user's framing): **compass / backlog are task SOURCES**
//! ("what should I do next?"), **condukt is the EXECUTOR** ("solve it"), and the
//! `/flow` skill is the loop that binds source → executor. This binary is a
//! thin, subscription-native scaffold (DESIGN: deterministic binary, LLM judges):
//! its only job is the SessionStart `propose` hook that, every session, tells the
//! agent to surface the unified pipeline and proactively offer `/flow` (L2:
//! propose-then-confirm) when there is open work.
//!
//! `propose` queries the backlog for pending items and injects conditionally:
//! - backlog absent (soft dep): fall back to static directive so the agent
//!   can still surface /flow on its own judgment.
//! - 0 pending items: stay silent — no chatter when there is nothing to do.
//! - N ≥ 1 items: inject a terse summary with count + top task title so the
//!   agent can offer a single AskUserQuestion without re-reading the queue.
//! The hook never breaks a turn: it only writes to stdout and exits 0.

use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
#[command(
    name = "flow",
    version,
    about = "Unified source→executor driver: binds compass/backlog (task sources) to condukt (executor)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// SessionStart hook: inject a conditional /flow proposal when pending backlog
    /// items exist. Silent when the queue is empty. Always exits 0.
    Propose,
}

#[derive(Deserialize)]
struct BacklogItem {
    #[serde(default)]
    title: String,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Propose => propose(),
    }
}

fn propose() {
    match query_backlog_pending() {
        None => {
            // backlog binary absent or errored → fall back to static directive so
            // the agent can still surface /flow on its own judgment.
            print!("{DIRECTIVE}");
        }
        Some(items) if items.is_empty() => {
            // No pending work — stay silent.
        }
        Some(items) => {
            let n = items.len();
            let first_title = items[0].title.as_str();
            print!("{}", pending_directive(n, first_title));
        }
    }
}

/// Query the backlog binary for pending items in the current working directory.
/// Returns `None` if the binary is absent or exits non-zero (fail-soft).
fn query_backlog_pending() -> Option<Vec<BacklogItem>> {
    let cwd = std::env::current_dir().ok()?;
    let project = cwd.to_string_lossy().to_string();

    let out = std::process::Command::new("backlog")
        .args(["list", "--project", &project, "--status", "pending", "--json"])
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    serde_json::from_slice::<Vec<BacklogItem>>(&out.stdout).ok()
}

fn pending_directive(n: usize, first_title: &str) -> String {
    format!(
        "[flow] バックログに {n} 件の pending タスクがあります (最優先: '{first_title}')。\n\
         他の作業を始める前に、AskUserQuestion 1 回で `/flow` を開始するかユーザーに確認してください。\n\
         (断られた場合、または open work が無い場合は /flow を再提案しない)\n"
    )
}

/// Fallback directive when the backlog binary is not available.
/// Phrased conditionally so a session with no open work produces no /flow chatter
/// (the agent simply stays silent).
const DIRECTIVE: &str = "\
[flow] A unified task pipeline is available this session:
  • SOURCE   — compass (the next right-sized move), backlog (the open queue),
               and hypothesis (open PDO hypotheses awaiting an experiment)
  • EXECUTOR — condukt (fugu-router routes each task's model)
  • DRIVER   — the /flow skill binds source → executor in a loop.

At the start of THIS session, read the compass / backlog / condukt / hypothesis
state already surfaced above. IF any open work exists (a compass next move, open
backlog items, an open hypothesis, or an unfinished condukt run), then BEFORE
other work, proactively ask the user —
with a SINGLE AskUserQuestion — whether to run `/flow` to drive that work now.
Summarize what is queued (e.g. \"backlog: N open; condukt: run X unfinished\") in
the question. If the user declines, or there is no open work, proceed normally and
do not raise /flow again unless asked.
";
