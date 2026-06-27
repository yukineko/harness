//! autoflow — session-end auto-flow gate for Claude Code.
//!
//! Stop hook state machine (per session):
//!   idle → [enough work?] → block: /record → record_requested
//!   record_requested | continuing → [condukt pending?]
//!     yes → block: /condukt (condukt tasks) → continuing
//!     no  → [backlog open?]
//!       yes → block: /condukt <next item> → continuing
//!       no  → done (allow)
//!   done → allow
//!
//!   condukt_prompts < 5  → block automatically
//!   condukt_prompts ≥ 5  → block: ask user each time

mod backlog;
mod condukt;
mod config;
mod insights;
mod lock;
mod state;

use clap::{Parser, Subcommand};
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;

use config::Config;
use state::Phase;

#[derive(Parser)]
#[command(
    name = "autoflow",
    version,
    about = "Session-end auto-flow gate for Claude Code."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: run the record→condukt state machine.
    Stop,
    /// SessionStart hook: injects /backlog prompt if open items exist.
    SessionStart,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Stop => stop_command(),
        Command::SessionStart => sessionstart_command(),
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

        // Stand down while another live session holds the backlog lock: a /flow
        // or /backlog driver is already running condukt against this queue, and
        // autoflow's auto-loop would double-drive it.
        if lock::backlog_driver_active() {
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
            Phase::RecordRequested | Phase::Continuing => {
                let pending = condukt::find_pending(&cwd);
                if !pending.is_empty() {
                    s.condukt_prompts += 1;
                    s.phase = Phase::Continuing;
                    state::save(&cfg.state_dir, &session_id, &s);

                    // Mark tasks as running so interruptions can be detected.
                    let ids: Vec<&str> = pending.iter().map(|t| t.id.as_str()).collect();
                    condukt::mark_running(&cwd, &ids);

                    let list = pending
                        .iter()
                        .map(|t| format!("- {} ({})", t.id, t.status))
                        .collect::<Vec<_>>()
                        .join("\n");

                    if s.condukt_prompts <= 4 {
                        block(&format!(
                            "condukt に残課題が {} 件あります:\n{}\n\n/condukt で続きを処理してください。",
                            pending.len(),
                            list
                        ));
                    } else {
                        block(&format!(
                            "condukt に残課題が {} 件あります ({}回目):\n{}\n\n自動実行を停止しています。続けるかどうかユーザーに確認してください。",
                            pending.len(),
                            s.condukt_prompts,
                            list
                        ));
                    }
                } else {
                    // condukt 完了 → backlog を確認
                    let open = backlog::find_open(&cwd);
                    if open.is_empty() {
                        s.phase = Phase::Done;
                        state::save(&cfg.state_dir, &session_id, &s);
                    } else {
                        s.backlog_prompts += 1;
                        // If we've hit the limit, give up — the skill or command likely failed.
                        if s.backlog_prompts > cfg.max_backlog_prompts {
                            s.phase = Phase::Done;
                            state::save(&cfg.state_dir, &session_id, &s);
                            return;
                        }
                        s.phase = Phase::Continuing;
                        state::save(&cfg.state_dir, &session_id, &s);

                        let next = &open[0];
                        let remaining = open.len();
                        let msg = format!(
                            "残課題バックログに {} 件の未完了課題があります。\n\n次の課題 [{}]: {}\n\n/backlog を実行してください。",
                            remaining, next.id, next.text
                        );
                        block(&msg);
                    }
                }
            }
            Phase::Done => {}
        }
    })
}

fn sessionstart_command() -> ! {
    run_hook(|| {
        let raw = read_stdin();
        let input = HookInput::parse(&raw).unwrap_or_default();

        let cfg = Config::load();
        if !cfg.enabled || Config::disabled_env() {
            return;
        }

        let cwd = input.cwd_or_current();
        let open = backlog::find_open(&cwd);
        if open.is_empty() {
            return;
        }

        let next = &open[0];
        let count = open.len();
        let context = format!(
            "バックログに {} 件の未完了課題があります。/backlog を実行してください。\n次の課題: [{}] {}",
            count, next.id, next.text
        );
        println!("{}", json!({ "additionalContext": context }));
    })
}

fn block(reason: &str) {
    println!("{}", json!({ "decision": "block", "reason": reason }));
}
