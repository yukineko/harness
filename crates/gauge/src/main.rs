//! gauge — local LLMOps telemetry for Claude Code.
//!
//! One hook, one job: after every turn (Stop), re-read the session transcript
//! and record cumulative token usage, cache hits, tool calls, latency, and
//! estimated cost to a local store. `gauge report` then rolls that up by
//! project, model, and day. No API key, nothing leaves the machine.
//!
//! Like the rest of the toolkit the hook can only *observe*: `record` runs
//! under `run_hook`, which catches panics and always exits 0, so bad/empty
//! stdin, a missing transcript, or an unwritable store all silently record
//! nothing rather than break the turn.

mod config;
mod install;
mod model;
mod report;
mod store;

use std::path::Path;

use clap::{Parser, Subcommand};

use harness_core::hook::{read_stdin, run_hook};
use harness_core::{pricing, usage};

use config::Config;
use model::HookInput;
use store::SessionRecord;

#[derive(Parser)]
#[command(
    name = "gauge",
    version,
    about = "Local LLMOps telemetry for Claude Code (Stop hook + usage/cost reports)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: aggregate the current session's transcript into the store.
    Record,
    /// Print a usage + cost report across all recorded sessions.
    Report {
        /// Only sessions whose project name contains this string.
        #[arg(long)]
        project: Option<String>,
        /// Only sessions on/after this day (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
    },
    /// Show details for the most recent session.
    Session,
    /// Merge the gauge Stop hook into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the gauge hook from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a starter ./gauge.toml.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Show the resolved config, store path, and recorded session count.
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Record => run_hook(record_hook),
        Command::Report { project, since } => report_cmd(project, since),
        Command::Session => session_cmd(),
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

fn record_hook() {
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
    let Some(agg) = usage::aggregate(&input.transcript_path) else {
        return;
    };

    let rec = SessionRecord::from_aggregate(
        input.session_id.clone(),
        input.project_name(),
        root.to_string_lossy().to_string(),
        agg,
        cfg.track_tools,
        chrono::Local::now().to_rfc3339(),
    );
    store::upsert(&cfg.state_dir, &rec);
}

fn report_cmd(project: Option<String>, since: Option<String>) {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let mut records = store::load_all(&cfg.state_dir);
    if let Some(p) = project.as_deref() {
        let needle = p.to_lowercase();
        records.retain(|r| r.project.to_lowercase().contains(&needle));
    }
    if let Some(s) = since.as_deref() {
        records.retain(|r| r.day().map(|d| d.as_str() >= s).unwrap_or(false));
    }
    println!("{}", report::render(&records, &cfg.pricing));
}

fn session_cmd() {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let records = store::load_all(&cfg.state_dir);
    let Some(rec) = records.into_iter().max_by(|a, b| a.updated_at.cmp(&b.updated_at)) else {
        println!("no sessions recorded yet.");
        return;
    };

    let cost: f64 = rec
        .models
        .iter()
        .map(|(m, u)| pricing::cost(m, u, &cfg.pricing))
        .sum();

    println!("session  {}", rec.session_id);
    println!("project  {} ({})", rec.project, rec.cwd);
    println!("turns    {}", rec.turns);
    println!(
        "cost     {}  ·  tokens {} ({})",
        report::money(cost),
        report::tokens_short(rec.total_tokens()),
        report::commas(rec.total_tokens()),
    );
    if let Some(dur) = duration_secs(&rec) {
        println!("duration {}", fmt_duration(dur));
    }

    println!("\nmodels");
    for (m, u) in &rec.models {
        println!(
            "  {:<24} {:>9}  in {} / out {} / cache {}",
            m,
            report::money(pricing::cost(m, u, &cfg.pricing)),
            report::tokens_short(u.input),
            report::tokens_short(u.output),
            report::tokens_short(u.cache_write_5m + u.cache_write_1h + u.cache_read),
        );
    }

    if !rec.agents.is_empty() {
        println!("\nagents (main vs sub-agent)");
        let costs = rec.agent_costs(&cfg.pricing);
        let mut agents: Vec<_> = rec.agents.iter().collect();
        agents.sort_by(|a, b| b.1.turns.cmp(&a.1.turns));
        for (name, a) in agents {
            println!(
                "  {:<12} {:>9}  {:>8}  {} turns",
                name,
                report::money(costs.get(name).copied().unwrap_or(0.0)),
                report::tokens_short(a.total_tokens()),
                a.turns,
            );
        }
    }

    if !rec.tools.is_empty() {
        println!("\ntools");
        let mut tools: Vec<_> = rec.tools.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in tools.iter().take(15) {
            println!("  {:<24} {}", name, count);
        }
    }
}

fn duration_secs(rec: &SessionRecord) -> Option<i64> {
    let first = rec.first_ts.as_ref()?;
    let last = rec.last_ts.as_ref()?;
    let a = chrono::DateTime::parse_from_rfc3339(first).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(last).ok()?;
    Some((b - a).num_seconds().max(0))
}

fn fmt_duration(secs: i64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn status() {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let records = store::load_all(&cfg.state_dir);
    let total_cost: f64 = records
        .iter()
        .flat_map(|r| r.models.iter().map(|(m, u)| pricing::cost(m, u, &cfg.pricing)))
        .sum();

    println!("config:       {}", Config::config_source(&root).display());
    println!("enabled:      {}", cfg.enabled);
    println!("track_tools:  {}", cfg.track_tools);
    println!("state_dir:    {}", cfg.state_dir.display());
    println!("sessions:     {}", records.len());
    println!("total cost:   {}", report::money(total_cost));
    println!(
        "pricing:      {}",
        if cfg.pricing.is_empty() {
            "built-in".to_string()
        } else {
            format!("{} override(s) + built-in", cfg.pricing.len())
        }
    );
    println!("\nrun `gauge report` for the full breakdown.");
}

const STARTER: &str = r#"# gauge.toml — local LLMOps telemetry for Claude Code.
#
# A Stop hook reads each session's transcript and records token usage, cache
# hits, tool calls, latency, and estimated cost to a local store. The hook only
# records — it never blocks a turn and never sends anything off the machine.
# Set GAUGE_DISABLE=1 to turn recording off entirely.

enabled = true
track_tools = true        # record per-tool call counts (Bash, Edit, …)

# state_dir = "~/.gauge/store"

# --- pricing overrides (optional) ---
# Built-in rates (USD per 1M tokens): Opus 5/25 · Sonnet 3/15 · Haiku 1/5 ·
# Fable 10/50. Cache writes bill at 1.25x input (5m) / 2x (1h); reads at 0.1x.
# `pattern` is matched as a substring against the model id; first match wins.
#
# [[pricing]]
# pattern = "opus"
# input = 5.0
# output = 25.0
"#;

fn init(force: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let path = Config::project_path(&root);
    if path.exists() && !force {
        anyhow::bail!("{} already exists (use --force to overwrite)", path.display());
    }
    std::fs::write(&path, STARTER)?;
    println!("wrote {}", path.display());
    println!("Run `gauge install` to wire the Stop hook, then `gauge report` after some turns.");
    Ok(())
}
