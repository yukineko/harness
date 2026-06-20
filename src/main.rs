//! session-insights — per-session work metrics for Claude Code.
//!
//! `record` is the PostToolUse hook (counts each tool call); `stop` is the Stop
//! hook (counts a turn, and optionally writes a dated session note to an Obsidian
//! vault). `report` prints the rollup: a size class (XS–XL), a work category, the
//! turn/tool/file counts, and the top tools — the deterministic, no-API-key
//! analogue of Devin's Session Insights. Recording only ever writes to its own
//! state (and, opt-in, the vault); it never blocks a turn and always exits 0.

mod config;
mod install;
mod metrics;
mod model;
mod obsidian;

use std::io::Read;
use std::path::Path;

use clap::{Parser, Subcommand};

use config::Config;
use model::HookInput;

#[derive(Parser)]
#[command(
    name = "session-insights",
    version,
    about = "Per-session work metrics for Claude Code (PostToolUse + Stop hooks)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// PostToolUse hook: record a tool call into the session rollup.
    Record,
    /// Stop hook: count a turn, optionally write the Obsidian session note.
    Stop,
    /// Print session insights (latest by default).
    Report {
        /// Session id prefix to report on.
        #[arg(long)]
        session: Option<String>,
        /// Summarize all recorded sessions instead.
        #[arg(long)]
        all: bool,
    },
    /// Merge the PostToolUse + Stop hooks into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the session-insights hooks from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Create the state dir.
    Init,
    /// Show the resolved config.
    Status,
}

fn read_stdin() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

fn run_hook<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> ! {
    let _ = std::panic::catch_unwind(f);
    std::process::exit(0);
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Record => run_hook(record),
        Command::Stop => run_hook(stop),
        Command::Report { session, all } => report(session, all),
        Command::Install { dry_run } => exit_on_err(install::install(dry_run)),
        Command::Uninstall { dry_run } => exit_on_err(install::uninstall(dry_run)),
        Command::Init => exit_on_err(init()),
        Command::Status => status(),
    }
}

fn exit_on_err(r: anyhow::Result<()>) {
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn record() {
    if Config::disabled_env() {
        return;
    }
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return;
    };
    let root = input.cwd_or_current();
    let cfg = Config::load(&root);
    if !cfg.enabled || input.tool_name.is_empty() || cfg.is_ignored(&input.tool_name) {
        return;
    }
    let session = input.session_key();
    let mut s = metrics::load(&cfg, &session);
    s.ensure(&session, &input.project_name(), &root.to_string_lossy());
    s.record_tool(&input.tool_name, input.target());
    metrics::save(&cfg, &session, &s);
}

fn stop() {
    if Config::disabled_env() {
        return;
    }
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return;
    };
    let root = input.cwd_or_current();
    let cfg = Config::load(&root);
    if !cfg.enabled {
        return;
    }
    let session = input.session_key();
    let mut s = metrics::load(&cfg, &session);
    s.ensure(&session, &input.project_name(), &root.to_string_lossy());
    s.record_turn();
    metrics::save(&cfg, &session, &s);
    obsidian::write_note(&cfg, &s);
}

fn report(session: Option<String>, all: bool) {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    if all {
        let sessions = metrics::load_all(&cfg);
        if sessions.is_empty() {
            println!("(no sessions recorded yet)");
            return;
        }
        for s in &sessions {
            println!(
                "{}  {:<14} {:>2} turns  {:>3} tools  size {:<2} {:<8} {}",
                metrics::short(&s.session_id),
                truncate(&s.project, 14),
                s.turns,
                s.tool_events,
                s.size(&cfg.size_thresholds),
                s.category(),
                &s.last_at.chars().take(19).collect::<String>(),
            );
        }
        return;
    }
    let chosen = match session {
        Some(p) => metrics::find(&cfg, &p),
        None => metrics::latest(&cfg),
    };
    match chosen {
        Some(s) => print!("{}", s.render_report(&cfg)),
        None => println!("(no matching session — run some tools first, or try --all)"),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn init() -> anyhow::Result<()> {
    let cfg = Config::default();
    std::fs::create_dir_all(&cfg.state_dir)?;
    println!("state dir ready: {}", cfg.state_dir.display());
    println!("Run `session-insights install` to wire the hooks, then `session-insights report`.");
    Ok(())
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
    println!("config:          {}", src.display());
    println!("enabled:         {}", cfg.enabled);
    println!("ignore_tools:    {}", cfg.ignore_tools.join(", "));
    println!("size_thresholds: {:?} (S,M,L,XL)", cfg.size_thresholds);
    println!("obsidian_log:    {}", cfg.obsidian_log);
    println!("obsidian_vault:  {}", cfg.obsidian_vault.display());
    println!("state_dir:       {}", cfg.state_dir.display());
    println!("sessions:        {}", metrics::load_all(&cfg).len());
}
