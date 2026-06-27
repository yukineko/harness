//! budgetguard — real-time cost budget gate for Claude Code.
//!
//! On every Stop it reads the session transcript, computes the USD cost using
//! the same pricing table as gauge, and blocks the turn if session or daily
//! limits are exceeded. Harness errors always exit 0 (never break the turn).

mod config;
mod gate;
mod install;
mod lock;

use clap::{Parser, Subcommand};

use harness_core::hook::{read_stdin, run_hook};

use config::Config;

#[derive(Parser)]
#[command(
    name = "budgetguard",
    version,
    about = "Real-time cost budget gate for Claude Code (Stop hook + spend reports)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: check session and daily cost against configured limits.
    Gate,
    /// Merge the budgetguard Stop hook into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the budgetguard Stop hook from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a starter ./budgetguard.toml.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Show the resolved config and today's spend.
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Gate => run_hook(gate_command),
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

fn gate_command() {
    if Config::disabled_env() {
        return;
    }
    let raw = read_stdin();
    let Some(input) = harness_core::hook::HookInput::parse(&raw) else {
        return;
    };
    if input.transcript_path.is_empty() || input.session_id.is_empty() {
        return;
    }

    let cwd = input.cwd_or_current();
    let cfg = Config::load(&cwd);
    if !cfg.enabled {
        return;
    }

    let today = today_str();
    let result = gate::evaluate(&cfg, &input.session_id, &input.transcript_path, &today);
    gate::emit_and_exit(result);
}

fn today_str() -> String {
    // Use chrono for reliable UTC date.
    use chrono::Utc;
    Utc::now().format("%Y-%m-%d").to_string()
}

fn init(force: bool) -> anyhow::Result<()> {
    let path = std::path::PathBuf::from("budgetguard.toml");
    if path.exists() && !force {
        eprintln!("budgetguard.toml already exists — pass --force to overwrite");
        return Ok(());
    }
    let template = include_str!("../budgetguard.example.toml");
    std::fs::write(&path, template)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn status() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let cfg = Config::load(&cwd);
    let today = today_str();
    let ledger = harness_core::ledger::Ledger::load(&cfg.state_dir);
    let day_usd = ledger.day_total(&today);

    println!("enabled:            {}", cfg.enabled);
    println!("state_dir:          {}", cfg.state_dir.display());
    println!();
    println!("session.warn_usd:   {:.2}", cfg.session_warn_usd);
    println!("session.block_usd:  {:.2}", cfg.session_block_usd);
    println!("daily.warn_usd:     {:.2}", cfg.daily_warn_usd);
    println!("daily.block_usd:    {:.2}", cfg.daily_block_usd);
    println!();
    println!("today ({today}):     ${day_usd:.4} spent");
}
