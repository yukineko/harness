//! reviewgate — a code-review gate for Claude Code.
//!
//! One binary, one subcommand per job. The `review` subcommand is the **Stop**
//! hook: before the agent can declare a turn done, reviewgate reviews the diff.
//! In `inject` mode it blocks the stop and injects a review rubric so the
//! running (subscription) agent reviews its own changes; in `subprocess` mode it
//! runs an independent reviewer and blocks only when issues are reported. It is
//! the "is this good code?" complement to donegate's "does it actually run?".
//!
//! Failure modes are split deliberately:
//!   * a *harness* error (bad config, no git, reviewer crash, our own bug) →
//!     exit 0, allow the stop. We must never trap a turn because reviewgate broke.
//!   * a real review finding (subprocess) or a not-yet-reviewed diff (inject) →
//!     block on purpose, with an actionable reason.

mod config;
mod git;
mod install;
mod model;
mod review;
mod state;

use std::path::Path;

use clap::{Parser, Subcommand};
use harness_core::hook::read_stdin;
use serde_json::json;

use config::{Config, Mode};
use model::HookInput;
use review::Decision;

#[derive(Parser)]
#[command(
    name = "reviewgate",
    version,
    about = "Code-review gate for Claude Code: review the diff on Stop; block until reviewed."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: review the diff; block the stop until it has been reviewed.
    Review,
    /// Merge the reviewgate Stop hook into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the reviewgate Stop hook from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a starter ./reviewgate.toml.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Show the resolved config + what would be reviewed for the cwd.
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Review => review_command(),
        Command::Install { dry_run } => exit_on_err(install::install(dry_run)),
        Command::Uninstall { dry_run } => exit_on_err(install::uninstall(dry_run)),
        Command::Init { force } => exit_on_err(init(force)),
        Command::Status => status(),
    }
}

fn exit_on_err(r: anyhow::Result<()>) {
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// The Stop hook. Always exits 0 toward Claude (the `decision` field, not the
/// exit code, is what blocks a stop). Returns exit 1 only in manual CLI mode.
fn review_command() -> ! {
    let raw = read_stdin();
    let hook = HookInput::parse(&raw);
    let interactive = hook.is_none();
    let input = hook.unwrap_or_default();
    let root = input.cwd_or_current();

    if Config::disabled_env() {
        if interactive {
            eprintln!("reviewgate: disabled (REVIEWGATE_DISABLE)");
        }
        std::process::exit(0);
    }

    let cfg = Config::load(&root);
    if !cfg.enabled {
        if interactive {
            eprintln!("reviewgate: disabled in config");
        }
        std::process::exit(0);
    }

    let session = input.session_key();

    // one-shot escape hatch
    if let Some(reason) = consume_skip(&root) {
        state::reset(&cfg.state_dir, &session);
        log_event(&cfg, &session, "skip", &[], 0);
        eprintln!("reviewgate: .reviewgate-skip consumed — allowing stop ({reason})");
        std::process::exit(0);
    }

    let prior = state::load(&cfg.state_dir, &session);
    let decision = review::evaluate(&cfg, &root, &prior);

    match decision {
        Decision::Allow {
            tag,
            attempts,
            last_hash,
        } => {
            if attempts == 0 && last_hash.is_empty() {
                state::reset(&cfg.state_dir, &session);
            } else {
                state::save(
                    &cfg.state_dir,
                    &session,
                    &state::SessionState {
                        attempts,
                        last_hash,
                        last_ts: chrono::Local::now().timestamp(),
                    },
                );
            }
            log_event(&cfg, &session, tag, &[], attempts);
            if interactive {
                println!("reviewgate: allow ({tag})");
            }
            std::process::exit(0);
        }
        Decision::Block {
            reason,
            tag,
            files,
            attempts,
            last_hash,
        } => {
            state::save(
                &cfg.state_dir,
                &session,
                &state::SessionState {
                    attempts,
                    last_hash,
                    last_ts: chrono::Local::now().timestamp(),
                },
            );
            log_event(&cfg, &session, tag, &files, attempts);
            if interactive {
                eprintln!("{reason}");
                std::process::exit(1);
            }
            println!("{}", json!({ "decision": "block", "reason": reason }));
            std::process::exit(0);
        }
    }
}

/// `.reviewgate-skip` in the project root: consumed once, returns its reason.
fn consume_skip(root: &Path) -> Option<String> {
    let p = root.join(".reviewgate-skip");
    if !p.exists() {
        return None;
    }
    let reason = std::fs::read_to_string(&p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no reason given)".to_string());
    let _ = std::fs::remove_file(&p);
    Some(reason)
}

/// Append one JSONL line per decision. Best effort, local only.
fn log_event(cfg: &Config, session: &str, verdict: &str, files: &[String], attempt: u32) {
    let path = cfg.state_dir.join("log.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = json!({
        "ts": chrono::Local::now().to_rfc3339(),
        "session": session,
        "verdict": verdict,
        "mode": cfg.mode.as_str(),
        "files": files,
        "attempt": attempt,
    });
    if let (Ok(line), Ok(mut f)) = (
        serde_json::to_string(&entry),
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path),
    ) {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

fn status() {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let src = if Config::project_path(&root).exists() {
        Config::project_path(&root)
    } else if Config::home_path().exists() {
        Config::home_path()
    } else {
        Path::new("(defaults — no config file)").to_path_buf()
    };
    println!("config:        {}", src.display());
    println!("enabled:       {}", cfg.enabled);
    println!("mode:          {}", cfg.mode.as_str());
    if cfg.mode == Mode::Subprocess {
        println!("reviewer_cmd:  {}", cfg.reviewer_cmd);
    }
    println!("max_attempts:  {}", cfg.max_attempts);
    println!("state_dir:     {}", cfg.state_dir.display());
    match git::changed_files(&root) {
        None => println!("changed:       (not a git repo — nothing to review)"),
        Some(changed) => {
            let files = review::reviewable_files(&cfg, &changed);
            println!(
                "changed:       {} file(s), {} reviewable",
                changed.len(),
                files.len()
            );
            for f in files.iter().take(20) {
                println!("  - {f}");
            }
            if files.len() > 20 {
                println!("  … (+{} more)", files.len() - 20);
            }
            if files.len() < cfg.min_changed_files {
                println!(
                    "(below min_changed_files={} — stop would be allowed)",
                    cfg.min_changed_files
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// init: write a starter reviewgate.toml.
// ---------------------------------------------------------------------------

fn init(force: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    std::fs::create_dir_all(Config::default().state_dir)?;
    let path = Config::project_path(&root);
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    std::fs::write(&path, STARTER)?;
    println!("wrote {}", path.display());
    println!("Review the settings, then run `reviewgate install` once to wire the Stop hook.");
    Ok(())
}

const STARTER: &str = r#"# reviewgate.toml — code-review gate for Claude Code.
#
# On Stop, reviewgate reviews the diff before the agent can declare done.
#   mode = "inject"     block once per new diff and inject a rubric; the running
#                       agent reviews its own changes (no API key).
#   mode = "subprocess" run `reviewer_cmd` as an independent reviewer and block
#                       only when it reports issues.

enabled = true
mode = "inject"

max_attempts = 2          # consecutive review rounds before giving up (allow stop)
reset_after_secs = 600    # per-session counter resets after this idle gap
min_changed_files = 1     # don't bother reviewing fewer than this many files
max_diff_bytes = 200000   # cap the diff handed to the hasher / reviewer

# Only files matching `include` (and not `exclude`) are reviewed. Omit to use
# the built-in source-extension defaults.
# include = ["**/*.rs", "**/*.ts", "**/*.py"]
# exclude = ["**/*.lock", "**/node_modules/**", "**/target/**"]

# The checklist injected into the model / handed to the reviewer. Omit for the
# built-in default rubric.
# rubric = """
# - 正しさ: 境界・エッジケース、off-by-one、null/None
# - エラー処理 / セキュリティ / 並行性 / テスト / 設計
# """

# subprocess mode only: a command that reads the review prompt on stdin and
# prints findings on stdout ("LGTM" = clean).
# reviewer_cmd = "claude -p"
# reviewer_timeout_secs = 300
"#;
