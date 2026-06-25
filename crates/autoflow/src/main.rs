//! autoflow — session-end auto-flow gate for Claude Code.
//!
//! Stop hook state machine (per session, one-shot each step):
//!   idle → [enough work?] → block: please run /record  → record_requested
//!   record_requested → [pending condukt tasks?] → block: please run /condukt → done
//!   done → allow (no more blocking)

mod config;
mod condukt;
mod insights;
mod state;

use clap::{Parser, Subcommand};
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;

use config::Config;
use state::Phase;

#[derive(Parser)]
#[command(name = "autoflow", version, about = "Session-end auto-flow gate for Claude Code.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: run the record→condukt state machine.
    Stop,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Stop => stop_command(),
    }
}

fn stop_command() -> ! {
    run_hook(|| {
        let raw = read_stdin();
        let input = HookInput::parse(&raw).unwrap_or_default();
        let session_id = if input.session_id.is_empty() {
            std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default()
        } else {
            input.session_id.clone()
        };
        if session_id.is_empty() {
            return;
        }

        let cfg = Config::load();
        if !cfg.enabled || Config::disabled_env() {
            return;
        }

        let cwd = input.cwd_or_current();
        let mut s = state::load(&cfg.state_dir, &session_id);

        match s.phase {
            Phase::Idle => {
                let metrics = insights::load_metrics(&session_id);
                if metrics.turns >= cfg.min_turns && metrics.tool_events >= cfg.min_tool_events {
                    s.phase = Phase::RecordRequested;
                    state::save(&cfg.state_dir, &session_id, &s);
                    block("/session-insights:record を実行してセッションを記録してください。");
                }
            }
            Phase::RecordRequested => {
                let pending = condukt::find_pending(&cwd);
                s.phase = Phase::Done;
                state::save(&cfg.state_dir, &session_id, &s);
                if !pending.is_empty() {
                    let list = pending
                        .iter()
                        .map(|t| format!("- {} ({})", t.id, t.status))
                        .collect::<Vec<_>>()
                        .join("\n");
                    block(&format!(
                        "condukt に残課題が {} 件あります:\n{}\n\n/condukt で続きを処理してください。",
                        pending.len(),
                        list
                    ));
                }
            }
            Phase::Done => {}
        }
    })
}

fn block(reason: &str) {
    println!("{}", json!({ "decision": "block", "reason": reason }));
}
