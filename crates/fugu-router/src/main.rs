//! fugu-router — fugu-style per-model routing for Claude Code orchestration.
//!
//! fugu (Sakana AI) hides a trained coordinator that routes each request across
//! a pool of models by role. We can't train Claude's weights, so the coordinator
//! becomes a deterministic policy over a *retrieval* store: record which model
//! passed verification on which kind of task (and at what cost), then pick the
//! cheapest tier that historically clears similar work. condukt calls `route` to
//! set each task's `suggested_model`, and `record` to feed outcomes back.

mod config;
mod decomp;
mod inject;
mod install;
mod policy;
mod rag;
mod rng;
mod semantic;
mod store;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;

#[derive(Parser)]
#[command(
    name = "fugu-router",
    about = "fugu-style per-model routing for Claude Code orchestration"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enrich a condukt decomposition: set each task's suggested_model from
    /// routing memory. Reads JSON (--file or stdin), writes JSON to stdout.
    Route {
        #[arg(long)]
        file: Option<PathBuf>,
        /// Also write a per-task routing report (worker/verifier/basis) here.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Record one task outcome into the episode store (the learning signal).
    Record {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        files: String,
        #[arg(long, default_value = "")]
        class: String,
        #[arg(long)]
        model: String,
        /// verified | failed (anything but a pass-word counts as a non-pass)
        #[arg(long)]
        status: String,
        #[arg(long, default_value = "worker")]
        role: String,
        #[arg(long, default_value_t = 0.0)]
        cost: f64,
    },
    /// Suggest a model for a single free-text task.
    Suggest {
        #[arg(long, default_value = "")]
        files: String,
        #[arg(long, default_value = "")]
        class: String,
        /// The task description (free text).
        text: Vec<String>,
    },
    /// Show per-model pass-rate / cost learned so far.
    Stats {
        #[arg(long)]
        json: bool,
    },
    /// UserPromptSubmit hook: inject a routing-memory summary.
    Prompt,
    /// Write a starter fugu-router.toml in the current directory.
    Init {
        #[arg(default_value = "fugu-router.toml")]
        target: String,
    },
    /// Merge the UserPromptSubmit hook into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove fugu-router hooks from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
}

fn split_files(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

fn is_pass(status: &str) -> bool {
    matches!(status, "verified" | "pass" | "passed" | "ok" | "true")
}

/// Seed the PRNG from wall-clock (nanosecond) + store size so exploration varies
/// run-to-run — second-granularity would make rapid back-to-back routes identical.
fn seed_rng(eps_len: usize) -> rng::Rng {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    rng::Rng::new(nanos ^ (eps_len as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Route one task: retrieve neighbours, then explore (bandit) or exploit (threshold).
fn route_decision(
    cfg: &config::Config,
    title: &str,
    files: &[String],
    class: &str,
    eps: &[store::Episode],
    rng: &mut rng::Rng,
) -> policy::Decision {
    let nb = rag::knn(title, files, eps, cfg.k, cfg.sim_threshold);
    if cfg.explore {
        policy::decide_bandit(title, files, class, &nb, cfg.pass_threshold, cfg.min_samples, rng)
    } else {
        policy::decide(title, files, class, &nb, cfg.pass_threshold, cfg.min_samples)
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Prompt => run_hook(|| {
            if config::disabled_env() {
                return;
            }
            let cfg = config::Config::load();
            if !cfg.enabled {
                return;
            }
            let input = HookInput::parse(&read_stdin()).unwrap_or_default();
            if !inject::looks_actionable(&input.prompt) {
                return;
            }
            let eps = store::load(&cfg.store_path());
            if let Some(ctx) = inject::summary(&eps, cfg.inject_limit) {
                println!("{}", json!({ "additionalContext": ctx }));
            }
        }),
        other => {
            if let Err(e) = run_user(other) {
                eprintln!("fugu-router: {e:#}");
                std::process::exit(1);
            }
        }
    }
}

fn run_user(cmd: Command) -> Result<()> {
    let cfg = config::Config::load();
    match cmd {
        Command::Route { file, report } => cmd_route(&cfg, file, report),
        Command::Record {
            title,
            files,
            class,
            model,
            status,
            role,
            cost,
        } => {
            let ep = store::Episode {
                ts: store::now_secs(),
                title,
                touched_files: split_files(&files),
                class,
                model,
                role,
                pass: is_pass(&status),
                cost_usd: cost,
            };
            store::append(&cfg.store_path(), &ep).context("appending episode")?;
            eprintln!("recorded: {} \"{}\" pass={}", ep.model, ep.title, ep.pass);
            Ok(())
        }
        Command::Suggest { files, class, text } => {
            let title = text.join(" ");
            let f = split_files(&files);
            let eps = store::load(&cfg.store_path());
            let mut rng = seed_rng(eps.len());
            let d = route_decision(&cfg, &title, &f, &class, &eps, &mut rng);
            println!(
                "worker={} verifier={} ({}, {} confidence)\n  {}",
                d.worker_model, d.verifier_model, d.basis, d.confidence, d.rationale
            );
            Ok(())
        }
        Command::Stats { json } => cmd_stats(&cfg, json),
        Command::Init { target } => config::init_config(&target),
        Command::Install { dry_run } => install::install(dry_run),
        Command::Uninstall { dry_run } => install::uninstall(dry_run),
        Command::Prompt => unreachable!("handled in main"),
    }
}

fn cmd_route(cfg: &config::Config, file: Option<PathBuf>, report: Option<PathBuf>) -> Result<()> {
    let raw = match file {
        Some(p) => {
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?
        }
        None => read_stdin(),
    };
    let mut dec: decomp::Decomposition =
        serde_json::from_str(&raw).context("parsing decomposition JSON")?;
    let eps = store::load(&cfg.store_path());
    let mut rng = seed_rng(eps.len());

    let mut report_map = serde_json::Map::new();
    for t in &mut dec.tasks {
        let d = route_decision(&cfg, &t.title, &t.touched_files, &t.class, &eps, &mut rng);
        // gated tasks keep whatever the interpreter chose; everything else is set.
        if d.basis != "gated" {
            t.suggested_model = d.worker_model.clone();
        }
        eprintln!(
            "  {:<16} worker={:<6} verifier={:<6} [{} {}] {}",
            t.id, t.suggested_model, d.verifier_model, d.basis, d.confidence, d.rationale
        );
        report_map.insert(
            t.id.clone(),
            json!({
                "worker_model": t.suggested_model,
                "verifier_model": d.verifier_model,
                "basis": d.basis,
                "confidence": d.confidence,
                "neighbors": d.neighbors,
                "rationale": d.rationale,
            }),
        );
    }

    println!("{}", serde_json::to_string_pretty(&dec)?);
    if let Some(rp) = report {
        std::fs::write(&rp, serde_json::to_string_pretty(&report_map)?)
            .with_context(|| format!("writing report {}", rp.display()))?;
        eprintln!("wrote routing report -> {}", rp.display());
    }
    Ok(())
}

fn cmd_stats(cfg: &config::Config, as_json: bool) -> Result<()> {
    use std::collections::BTreeMap;
    let eps = store::load(&cfg.store_path());
    let mut agg: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    for e in &eps {
        let s = agg.entry(e.model.clone()).or_insert((0, 0, 0.0));
        s.0 += 1;
        if e.pass {
            s.1 += 1;
        }
        s.2 += e.cost_usd;
    }
    if as_json {
        let obj: serde_json::Map<String, serde_json::Value> = agg
            .iter()
            .map(|(m, (n, p, c))| {
                (
                    m.clone(),
                    json!({
                        "count": n,
                        "passes": p,
                        "pass_rate": if *n > 0 { *p as f64 / *n as f64 } else { 0.0 },
                        "avg_cost_usd": if *n > 0 { c / *n as f64 } else { 0.0 },
                    }),
                )
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "episodes": eps.len(), "models": obj }))?
        );
    } else {
        println!("episodes: {}", eps.len());
        for (m, (n, p, c)) in &agg {
            println!(
                "  {m:<6} {p}/{n} pass ({:.0}%)  avg ${:.4}",
                if *n > 0 { *p as f64 / *n as f64 * 100.0 } else { 0.0 },
                if *n > 0 { c / *n as f64 } else { 0.0 }
            );
        }
    }
    Ok(())
}
