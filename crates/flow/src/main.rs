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
//! It deliberately does NOT recompute task counts — compass `nudge`, backlog
//! `session-start`, and condukt `restore` already inject their own state into
//! the SessionStart context. `propose` just adds the directive that ties them
//! together. The hook never breaks a turn: it only writes to stdout and exits 0.

use clap::{Parser, Subcommand};

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
    /// SessionStart hook: inject the L2 directive that proposes `/flow` when this
    /// session has open work (a compass next move, open backlog items, or an
    /// unfinished condukt run). Always exits 0 — never breaks the turn.
    Propose,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Propose => print!("{DIRECTIVE}"),
    }
}

/// Injected as SessionStart additionalContext. Phrased conditionally so a session
/// with no open work produces no `/flow` chatter (the agent simply stays silent).
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
