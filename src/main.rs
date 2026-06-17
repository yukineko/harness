//! specguard — project-agnostic spec/implementation drift audit harness.
//!
//! Reads a TOML config describing a project's canon (areas, canon pointers,
//! invariants), resolves a change-triggered scope from `git diff`, renders a
//! read-only audit prompt, drives an LLM agent to produce findings, and
//! persists a report plus a sentinel when something needs human review.
//!
//! The judgment lives in the agent (it reads the live canon and quotes it
//! verbatim); this binary is the deterministic harness around it.

mod agent;
mod config;
mod parse;
mod prompt;
mod report;
mod scope;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes (stable contract for schedulers/hooks).
const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_NO_MARKER: u8 = 3;
const EXIT_AGENT_FAILED: u8 = 4;

#[derive(Parser)]
#[command(name = "specguard", version, about = "Spec/implementation drift audit harness")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "specguard.toml", global = true)]
    config: PathBuf,

    /// Override the baseline ref (also via SPECGUARD_BASELINE_REF).
    #[arg(short, long, global = true)]
    baseline: Option<String>,

    /// Override the run date (YYYY-MM-DD); default is today (also SPECGUARD_NOW).
    #[arg(long, global = true)]
    date: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the full audit (default).
    Run,
    /// Print the resolved scope only (no agent call).
    Scope,
    /// Print the rendered prompt only (no agent call).
    Prompt,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("specguard: error: {e:#}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

struct Loaded {
    cfg: Config,
    repo_root: PathBuf,
    template: String,
    date: String,
}

/// Load config + resolve repo root, template and date (shared by all commands).
fn load(cli: &Cli) -> Result<Loaded> {
    let cfg = Config::load(&cli.config)?;
    let config_dir = cli
        .config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let repo_root = canonicalize(&config_dir.join(&cfg.project.root))
        .with_context(|| "resolving project.root")?;
    let template = load_template(&cfg, config_dir)?;
    let date = resolve_date(cli.date.clone());
    Ok(Loaded {
        cfg,
        repo_root,
        template,
        date,
    })
}

fn run(cli: &Cli) -> Result<u8> {
    let l = load(cli)?;
    let paths = report::paths(&l.cfg, &l.repo_root, &l.date);
    let last_ref = report::read_last_ref(&paths);
    let override_ref = cli
        .baseline
        .clone()
        .or_else(|| std::env::var("SPECGUARD_BASELINE_REF").ok());

    let scope = scope::resolve(&l.cfg, &l.repo_root, override_ref.as_deref(), last_ref.as_deref())?;

    match cli.command {
        Some(Command::Scope) => {
            print_scope(&l.cfg, &scope);
            return Ok(EXIT_OK);
        }
        Some(Command::Prompt) => {
            print!("{}", prompt::render(&l.template, &l.cfg, &scope, &l.date));
            return Ok(EXIT_OK);
        }
        Some(Command::Run) | None => {}
    }

    let rendered = prompt::render(&l.template, &l.cfg, &scope, &l.date);
    eprintln!(
        "specguard: auditing {} (baseline {}, {} in-scope area(s), {} invariant(s))",
        l.cfg.project.name,
        scope.baseline,
        scope.in_scope.len(),
        l.cfg.invariants.len()
    );

    let out = agent::run(&l.cfg.agent, &l.repo_root, &rendered)?;
    if out.code != 0 {
        eprintln!(
            "specguard: agent exited with code {}\n--- agent stderr ---\n{}",
            out.code,
            out.stderr.trim_end()
        );
        return Ok(EXIT_AGENT_FAILED);
    }

    let parsed = parse::parse(&out.stdout);
    let head = scope::current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());

    if !parsed.marker_found {
        // Incomplete report: save it for inspection but do NOT advance the
        // baseline or raise a sentinel (avoids false positives / lost findings).
        if let Some(dir) = paths.report.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&paths.report, &out.stdout)
            .with_context(|| format!("writing report {}", paths.report.display()))?;
        eprintln!(
            "specguard: WARN marker '{}' missing; saved {} but cannot assess findings",
            parse::MARKER,
            paths.report.display()
        );
        return Ok(EXIT_NO_MARKER);
    }

    report::write_report(&paths, &parsed.report, &head)?;

    let report_rel = format!("{}/{}.md", l.cfg.output.report_dir, l.date);
    if parsed.needs_user {
        report::write_sentinel(&paths, &l.date, &report_rel, &parsed.summary)?;
        println!(
            "specguard: 修正候補あり -> {} (report: {})",
            paths.sentinel.display(),
            paths.report.display()
        );
    } else {
        // No findings: leave any pre-existing sentinel untouched (a prior,
        // still-unhandled run may own it).
        println!("specguard: 修正候補なし (report: {})", paths.report.display());
    }
    Ok(EXIT_OK)
}

fn print_scope(cfg: &Config, scope: &scope::Scope) {
    println!("baseline: {}", scope.baseline);
    if scope.fell_back {
        println!("(baseline fell back from configured/recorded ref)");
    }
    println!("changed files: {}", scope.changed_files.len());
    println!("in-scope areas:");
    if scope.in_scope.is_empty() {
        println!("  (none)");
    }
    for hit in &scope.in_scope {
        println!(
            "  - {} ({} file(s))",
            cfg.areas[hit.area_index].name,
            hit.matched_files.len()
        );
    }
    println!("skipped areas: {}", join(&scope.skipped_areas));
    println!(
        "invariants (always): {}",
        join(&cfg.invariants.iter().map(|i| i.name.clone()).collect::<Vec<_>>())
    );
}

fn join(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}

/// Resolve the template text: explicit `[prompt].template` path, else embedded.
fn load_template(cfg: &Config, config_dir: &Path) -> Result<String> {
    if cfg.prompt.template.trim().is_empty() {
        Ok(prompt::DEFAULT_TEMPLATE.to_string())
    } else {
        let p = config_dir.join(&cfg.prompt.template);
        std::fs::read_to_string(&p)
            .with_context(|| format!("reading prompt template {}", p.display()))
    }
}

fn resolve_date(cli_date: Option<String>) -> String {
    if let Some(d) = cli_date {
        return d;
    }
    if let Ok(d) = std::env::var("SPECGUARD_NOW") {
        if !d.trim().is_empty() {
            return d.trim().to_string();
        }
    }
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Canonicalize a path, falling back to the joined path if it doesn't exist yet.
fn canonicalize(p: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(p).or_else(|_| Ok(p.to_path_buf()))
}
