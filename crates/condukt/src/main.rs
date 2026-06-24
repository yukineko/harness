//! condukt — deterministic orchestration engine for Claude Code.
//!
//! The LLM (the `/condukt` skill + interpreter/worker/verifier agents) does the
//! judgement work: interpret a request, decompose it into tasks, implement and
//! verify each. This binary does the deterministic work the LLM should not
//! eyeball: schedule tasks into parallel/serial batches by file-conflict
//! analysis, manage the git-worktree lifecycle, track run state, and gate
//! completion. Hooks (restore/statusline) never break a turn — they exit 0.

mod config;
mod hooks;
mod install;
mod model;
mod schedule;
mod state;
mod store;
mod worktree;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "condukt",
    version,
    about = "Deterministic orchestration engine for Claude Code"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// SessionStart hook: remind about unfinished runs / orphan worktrees.
    Restore,
    /// One-line progress readout for the statusLine.
    Statusline,
    /// Compute a schedule from a decomposition JSON (stdin or --file).
    Schedule {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Validate a decomposition JSON (stdin or --file).
    Validate {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// git worktree lifecycle.
    Worktree {
        #[command(subcommand)]
        action: WtAction,
    },
    /// Run-state tracking and the completion gate.
    State {
        #[command(subcommand)]
        action: StateAction,
    },
    /// Create ~/.condukt and a default config.toml.
    Init,
    /// Merge the SessionStart hook into ~/.claude/settings.json (manual install).
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove condukt hooks from ~/.claude/settings.json.
    Uninstall,
}

#[derive(Subcommand)]
enum WtAction {
    /// Create a worktree at <worktree_base>/<topic> on a new branch; prints path.
    Create {
        #[arg(long)]
        topic: String,
        #[arg(long)]
        branch: String,
    },
    /// Merge <branch> into the configured default branch.
    Merge {
        #[arg(long)]
        branch: String,
    },
    /// Remove a worktree and (best-effort) delete its branch.
    Remove {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        branch: Option<String>,
    },
    /// Report orphan worktrees under worktree_base (--remove to delete them).
    Cleanup {
        #[arg(long)]
        remove: bool,
    },
    /// List registered worktrees (path<TAB>branch).
    List,
}

#[derive(Subcommand)]
enum StateAction {
    /// Seed a run from a decomposition JSON; prints the run id on stdout.
    Init {
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Update one task's status (and optionally its worktree/branch).
    Set {
        #[arg(long)]
        run: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        branch: Option<String>,
    },
    /// Print a run's full state as JSON.
    Show {
        #[arg(long)]
        run: String,
    },
    /// Exit 0 if the run is complete, else print reasons and exit 1.
    Gate {
        #[arg(long)]
        run: String,
    },
    /// List open runs (run_id<TAB>done/total<TAB>goal).
    List,
    /// Run the project's test suite (from config [test].command or auto-detect).
    Test {
        #[arg(long)]
        run: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        // Hooks never break a turn: swallow panics and always exit 0.
        Command::Restore => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let input = model::HookInput::parse(&read_stdin());
            hooks::restore::run(input.cwd.as_deref().unwrap_or(""));
        }),
        Command::Statusline => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let input = model::HookInput::parse(&read_stdin());
            hooks::statusline::run(input.cwd.as_deref().unwrap_or(""));
        }),
        other => {
            if let Err(e) = run_user(other) {
                eprintln!("condukt: {e:#}");
                std::process::exit(1);
            }
        }
    }
}

fn run_hook<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> ! {
    let _ = std::panic::catch_unwind(f);
    std::process::exit(0);
}

fn run_user(cmd: Command) -> Result<()> {
    let cfg = Config::load();
    let cwd = std::env::current_dir().context("getting cwd")?;
    match cmd {
        Command::Schedule { file } => {
            let dec = read_decomposition(file)?;
            let errs = schedule::validate(&dec);
            if !errs.is_empty() {
                bail!("invalid decomposition:\n  - {}", errs.join("\n  - "));
            }
            let sched = schedule::schedule(&dec, &cfg.shared_globs);
            println!("{}", serde_json::to_string_pretty(&sched)?);
        }
        Command::Validate { file } => {
            let dec = read_decomposition(file)?;
            let errs = schedule::validate(&dec);
            if errs.is_empty() {
                eprintln!("ok: {} task(s)", dec.tasks.len());
            } else {
                bail!("invalid:\n  - {}", errs.join("\n  - "));
            }
        }
        Command::Worktree { action } => run_worktree(&cfg, &cwd, action)?,
        Command::State { action } => run_state(&cfg, &cwd, action)?,
        Command::Init => init(&cfg)?,
        Command::Install { dry_run } => {
            if dry_run {
                install::dry_run()?;
            } else {
                install::install()?;
            }
        }
        Command::Uninstall => install::uninstall()?,
        Command::Restore | Command::Statusline => unreachable!("handled in main"),
    }
    Ok(())
}

fn run_worktree(cfg: &Config, cwd: &Path, action: WtAction) -> Result<()> {
    let repo = worktree::toplevel(cwd)?;
    match action {
        WtAction::Create { topic, branch } => {
            let path = worktree::create(&repo, &cfg.worktree_base, &topic, &branch)?;
            println!("{}", path.display());
        }
        WtAction::Merge { branch } => worktree::merge(&repo, &branch, &cfg.default_branch)?,
        WtAction::Remove { path, branch } => {
            worktree::remove(&repo, &path, branch.as_deref())?;
        }
        WtAction::Cleanup { remove } => {
            let orphans = worktree::orphans(&repo, &cfg.worktree_base)?;
            let _ = worktree::git(&repo, &["worktree", "prune"]);
            if orphans.is_empty() {
                eprintln!("no orphan worktrees under {}", cfg.worktree_base.display());
            }
            for o in orphans {
                if remove {
                    std::fs::remove_dir_all(&o)
                        .with_context(|| format!("removing {}", o.display()))?;
                    eprintln!("removed orphan {}", o.display());
                } else {
                    eprintln!("orphan (pass --remove to delete): {}", o.display());
                }
            }
        }
        WtAction::List => {
            for (p, b) in worktree::list(&repo)? {
                println!("{}\t{}", p.display(), b.unwrap_or_else(|| "-".into()));
            }
        }
    }
    Ok(())
}

fn run_state(cfg: &Config, cwd: &Path, action: StateAction) -> Result<()> {
    match action {
        StateAction::Init { run, file } => {
            let dec = read_decomposition(file)?;
            let errs = schedule::validate(&dec);
            if !errs.is_empty() {
                bail!("invalid decomposition:\n  - {}", errs.join("\n  - "));
            }
            let run_id = run.unwrap_or_else(default_run_id);
            let tasks = dec
                .tasks
                .iter()
                .map(|t| state::TaskState {
                    id: t.id.clone(),
                    status: state::Status::Pending,
                    worktree: None,
                    branch: None,
                })
                .collect();
            let rs = state::RunState {
                run_id: run_id.clone(),
                goal: dec.goal.clone(),
                tasks,
            };
            let path = rs.save(cfg, cwd)?;
            eprintln!("initialized run '{run_id}' at {}", path.display());
            println!("{run_id}");
        }
        StateAction::Set {
            run,
            task,
            status,
            worktree,
            branch,
        } => {
            let mut rs = state::RunState::load(cfg, cwd, &run)?;
            let st: state::Status = status.parse()?;
            let t = rs
                .tasks
                .iter_mut()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            t.status = st;
            if worktree.is_some() {
                t.worktree = worktree;
            }
            if branch.is_some() {
                t.branch = branch;
            }
            rs.save(cfg, cwd)?;
            let (done, total) = rs.counts();
            eprintln!("run '{run}': {done}/{total} verified");
        }
        StateAction::Show { run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            println!("{}", serde_json::to_string_pretty(&rs)?);
        }
        StateAction::Gate { run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            let reasons = state::gate_reasons(cfg, cwd, &rs);
            if reasons.is_empty() {
                eprintln!("gate PASS: run '{run}' complete");
            } else {
                eprintln!("gate FAIL ({} reason(s)):", reasons.len());
                for r in &reasons {
                    eprintln!("  - {r}");
                }
                std::process::exit(1);
            }
        }
        StateAction::List => {
            for rs in state::open_runs(cfg, cwd) {
                let (done, total) = rs.counts();
                println!("{}\t{}/{}\t{}", rs.run_id, done, total, rs.goal);
            }
        }
        StateAction::Test { run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            state::run_tests(cfg, cwd, &rs)?;
        }
    }
    Ok(())
}

fn init(cfg: &Config) -> Result<()> {
    let base = config::base_dir();
    std::fs::create_dir_all(&cfg.state_dir).ok();
    std::fs::create_dir_all(&cfg.worktree_base).ok();
    let cfg_path = base.join("config.toml");
    if cfg_path.exists() {
        eprintln!("config already exists: {}", cfg_path.display());
        return Ok(());
    }
    let default = format!(
        "# condukt configuration\n\
         # Where worktrees are created (MUST be outside any repo).\n\
         worktree_base = \"{}\"\n\
         # Branch completed work merges back into.\n\
         default_branch = \"{}\"\n\
         # Soft cap on concurrent workers (advisory; the skill honors it).\n\
         max_parallel = {}\n\
         # Globs that force a touching task to run serially (never in parallel),\n\
         # e.g. [\"**/models.py\", \"**/migrations/**\", \"docs/glossary.md\"].\n\
         shared_globs = []\n\
         \n\
         # Command `condukt state test` runs (via `sh -c`, from the repo root).\n\
         # Omit to auto-detect (cargo test / npm test / pytest).\n\
         # [test]\n\
         # command = \"cargo test\"\n",
        cfg.worktree_base.display(),
        cfg.default_branch,
        cfg.max_parallel
    );
    std::fs::write(&cfg_path, default).with_context(|| format!("writing {}", cfg_path.display()))?;
    eprintln!("wrote {}", cfg_path.display());
    Ok(())
}

fn default_run_id() -> String {
    chrono::Local::now().format("run-%Y%m%d-%H%M%S").to_string()
}

fn read_decomposition(file: Option<PathBuf>) -> Result<model::Decomposition> {
    let raw = match file {
        Some(p) => {
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?
        }
        None => read_stdin(),
    };
    serde_json::from_str(&raw).context("parsing decomposition JSON")
}

fn read_stdin() -> String {
    use std::io::Read;
    let mut s = String::new();
    let _ = std::io::stdin().read_to_string(&mut s);
    s
}
