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
    /// Print ~/.condukt/knowledge.md to stdout (empty output if absent).
    Knowledge,
    /// Run one iteration of the test-fix cycle for the given module type.
    ///
    /// Executes build/deploy/test in the sequence appropriate for the module:
    ///   server: deploy → test
    ///   client: build → test
    ///   e2e:    build → deploy → test
    ///
    /// Prints a JSON object with iteration, failure_count, stop, and stop_reason.
    /// Exit 0 while progress is being made; exit 1 when stop=true and tests failed.
    Loop {
        /// Module type determines the cycle sequence.
        #[arg(long, value_name = "server|client|e2e")]
        module: String,
        /// Which iteration this is (informational; included in JSON output).
        #[arg(long, default_value = "1")]
        iteration: usize,
        /// Failure count from the previous iteration (used for no-progress detection).
        #[arg(long)]
        prev_failures: Option<usize>,
    },
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
        /// Terminal or session label for identification (e.g. from `tty`).
        /// Shown in `state list` and included in conflict reports.
        #[arg(long)]
        label: Option<String>,
    },
    /// Check whether a decomposition's touched_files conflict with open runs.
    /// Exits 0 if no conflicts, exits 1 if conflicts are found (JSON on stdout).
    ConflictCheck {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Update one task's status (and optionally its worktree/branch).
    /// Passing --worktree/--branch without a value has no effect; use
    /// --clear-worktree / --clear-branch to explicitly erase the existing value.
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
        /// Explicitly clear the worktree field (set it to null).
        #[arg(long)]
        clear_worktree: bool,
        /// Explicitly clear the branch field (set it to null).
        #[arg(long)]
        clear_branch: bool,
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
    /// Auto-reconcile: detect merged/gone branches and mark tasks verified.
    Reconcile {
        #[arg(long)]
        run: String,
        /// Preview changes without writing (dry run).
        #[arg(long)]
        dry_run: bool,
    },
    /// Aggregate stats across all runs (completion rate, task distribution).
    Stats,
    /// Resume context for a stopped run: pending/failed/done tasks as JSON.
    ResumeContext {
        #[arg(long)]
        run: String,
    },
    /// Run the project's test suite (from config [test].command or auto-detect).
    Test {
        #[arg(long)]
        run: String,
    },
    /// Reset running/failed tasks back to pending, clearing their worktree/branch refs.
    Abandon {
        #[arg(long)]
        run: String,
        /// Abandon a specific task (set it back to pending).
        #[arg(long)]
        task: Option<String>,
        /// Abandon all stuck tasks (running + TTL exceeded) at once.
        #[arg(long)]
        all_stuck: bool,
    },
    /// Pause a run: no new workers will be dispatched until resumed.
    Pause {
        #[arg(long)]
        run: String,
    },
    /// Resume a paused run, allowing new workers to be dispatched again.
    Resume {
        #[arg(long)]
        run: String,
    },
    /// List all non-terminal tasks across open runs as JSON (for interactive cancel).
    ListTasks,
    /// Cancel a specific task, marking it as cancelled and clearing its worktree/branch.
    /// Works for pending/running/done tasks. Verified tasks cannot be cancelled.
    /// NOTE: cancelling a running task only updates state; an in-flight worker must be
    /// stopped manually (the binary cannot reach into a running Claude agent).
    Cancel {
        #[arg(long)]
        run: String,
        #[arg(long)]
        task: String,
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
        Command::Knowledge => {
            let path = config::base_dir().join("knowledge.md");
            if path.exists() {
                print!("{}", std::fs::read_to_string(&path)?);
            }
        }
        Command::Loop {
            module,
            iteration,
            prev_failures,
        } => {
            use crate::config::ModuleCycle;
            use crate::state::{loop_should_stop, run_cycle};
            let cycle = ModuleCycle::from_str(&module)
                .ok_or_else(|| anyhow!("unknown module '{module}'; use server, client, or e2e"))?;
            let result = run_cycle(&cfg, &cwd, cycle)?;
            let (stop, stop_reason) = loop_should_stop(prev_failures, result.failure_count);
            let out = serde_json::json!({
                "iteration": iteration,
                "module": module,
                "failure_count": result.failure_count,
                "success": result.success,
                "stop": stop,
                "stop_reason": stop_reason,
                "output": result.output,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            // Exit non-zero when not all tests pass so shell scripts can branch on it.
            if !result.success && stop {
                std::process::exit(1);
            }
        }
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
            let undeleted = worktree::remove(&repo, &path, branch.as_deref())?;
            if let Some(b) = undeleted {
                eprintln!(
                    "condukt: worktree removed but branch '{}' remains — \
                     it has unmerged commits. Merge or force-delete it manually.",
                    b
                );
            }
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
        StateAction::Init { run, file, label } => {
            let raw = match &file {
                Some(p) => std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?,
                None => read_stdin(),
            };
            let dec: model::Decomposition =
                serde_json::from_str(&raw).context("parsing decomposition JSON")?;
            let errs = schedule::validate(&dec);
            if !errs.is_empty() {
                bail!("invalid decomposition:\n  - {}", errs.join("\n  - "));
            }
            let run_id = run.unwrap_or_else(default_run_id);
            let resolved_label = label.or_else(resolve_label);
            let tasks = dec
                .tasks
                .iter()
                .map(|t| state::TaskState {
                    id: t.id.clone(),
                    status: state::Status::Pending,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                })
                .collect();
            let rs = state::RunState {
                run_id: run_id.clone(),
                goal: dec.goal.clone(),
                tasks,
                paused: false,
                terminal_label: resolved_label,
            };
            let path = rs.save(cfg, cwd)?;
            // Persist the decomposition so `state resume-context` can reconstruct tasks.
            state::save_decomposition(cfg, cwd, &run_id, &raw)?;
            eprintln!("initialized run '{run_id}' at {}", path.display());
            println!("{run_id}");
        }
        StateAction::ConflictCheck { file } => {
            let raw = match &file {
                Some(p) => std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?,
                None => read_stdin(),
            };
            let dec: model::Decomposition =
                serde_json::from_str(&raw).context("parsing decomposition JSON")?;
            let report = state::cross_run_conflicts(cfg, cwd, &dec);
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.has_conflicts {
                std::process::exit(1);
            }
        }
        StateAction::Set {
            run,
            task,
            status,
            worktree,
            branch,
            clear_worktree,
            clear_branch,
        } => {
            let mut rs = state::RunState::load(cfg, cwd, &run)?;
            let st: state::Status = status.parse()?;
            let t = rs
                .tasks
                .iter_mut()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            t.status = st;
            t.updated_at = Some(state::now_secs());
            // None 上書き保護: --worktree/--branch が省略された場合は既存値を保持する。
            // 明示的にクリアしたい場合は --clear-worktree / --clear-branch を使う。
            if clear_worktree {
                t.worktree = None;
            } else if worktree.is_some() {
                t.worktree = worktree;
            }
            if clear_branch {
                t.branch = None;
            } else if branch.is_some() {
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
        StateAction::Reconcile { run, dry_run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            let (updated, changes) = state::reconcile_run(cfg, cwd, rs, &cfg.default_branch)?;
            if changes.is_empty() {
                eprintln!("reconcile: nothing to change for run '{run}'");
            } else {
                for c in &changes {
                    if c.old_status == c.new_status {
                        eprintln!(
                            "  [{}]  {} (status unchanged: {:?})",
                            c.task_id, c.reason, c.old_status
                        );
                    } else {
                        eprintln!(
                            "  [{}]  {:?} → {:?}  ({})",
                            c.task_id, c.old_status, c.new_status, c.reason
                        );
                    }
                }
                if dry_run {
                    eprintln!("dry-run: {} change(s) would be applied", changes.len());
                } else {
                    updated.save(cfg, cwd)?;
                    let (done, total) = updated.counts();
                    eprintln!(
                        "reconcile: applied {} change(s) — run '{run}': {done}/{total} verified",
                        changes.len()
                    );
                }
            }
        }
        StateAction::List => {
            for rs in state::open_runs(cfg, cwd) {
                let (done, total) = rs.counts();
                let paused_tag = if rs.paused { " [paused]" } else { "" };
                let label_tag = rs
                    .terminal_label
                    .as_deref()
                    .map(|l| format!(" @{l}"))
                    .unwrap_or_default();
                println!("{}\t{}/{}\t{}{}{}", rs.run_id, done, total, rs.goal, paused_tag, label_tag);
            }
        }
        StateAction::Stats => {
            let all = state::compute_stats(cfg, cwd);
            if all.is_empty() {
                eprintln!("no runs found");
                return Ok(());
            }
            let total_runs = all.len();
            let complete = all.iter().filter(|s| s.is_complete).count();
            let rate = if total_runs > 0 {
                complete * 100 / total_runs
            } else {
                0
            };
            eprintln!(
                "condukt stats: {complete}/{total_runs} complete ({rate}%)\n"
            );
            // Header
            println!("{:<32}  {:>6}  {:>6}  {}",
                "run_id", "done", "total", "goal");
            println!("{}", "-".repeat(72));
            for s in &all {
                let marker = if s.is_complete { "✓" } else { " " };
                let goal_short = truncate_chars(&s.goal, 30);
                println!(
                    "{marker} {:<30}  {:>6}  {:>6}  {}",
                    &s.run_id, s.verified, s.total, goal_short
                );
            }
            // Task status distribution across all runs.
            let mut totals: std::collections::HashMap<String, usize> = Default::default();
            for s in &all {
                for (k, v) in &s.status_counts {
                    *totals.entry(k.clone()).or_insert(0) += v;
                }
            }
            println!("\ntask status distribution (all runs):");
            let mut pairs: Vec<_> = totals.into_iter().collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1));
            for (k, v) in pairs {
                println!("  {k}: {v}");
            }
        }
        StateAction::ResumeContext { run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            let dec_raw = state::load_decomposition(cfg, cwd, &run)?;
            let dec: model::Decomposition =
                serde_json::from_str(&dec_raw).context("parsing saved decomposition")?;
            // Build a map from task id → full decomposition task.
            let dec_map: std::collections::HashMap<_, _> =
                dec.tasks.iter().map(|t| (t.id.clone(), t)).collect();

            let mut pending = Vec::new();
            let mut failed = Vec::new();
            let mut needs_verification = Vec::new();
            let mut verified_count = 0usize;

            for ts in &rs.tasks {
                let base = dec_map.get(&ts.id).map(|t| serde_json::to_value(t).unwrap_or_default());
                let entry = serde_json::json!({
                    "id": ts.id,
                    "status": format!("{:?}", ts.status).to_lowercase(),
                    "worktree": ts.worktree,
                    "branch": ts.branch,
                    "task": base,
                });
                match ts.status {
                    state::Status::Pending | state::Status::Running => pending.push(entry),
                    state::Status::Failed => failed.push(entry),
                    state::Status::Done => needs_verification.push(entry),
                    state::Status::Verified | state::Status::Cancelled => verified_count += 1,
                }
            }

            let out = serde_json::json!({
                "run_id": rs.run_id,
                "goal": rs.goal,
                "verified_count": verified_count,
                "total_count": rs.tasks.len(),
                "pending_tasks": pending,
                "failed_tasks": failed,
                "needs_verification": needs_verification,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        StateAction::Test { run } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            state::run_tests(cfg, cwd, &rs)?;
        }
        StateAction::Abandon { run, task, all_stuck } => {
            let mut rs = state::RunState::load(cfg, cwd, &run)?;
            let ids: Vec<String> = if let Some(task_id) = task {
                // Specific task: validate it exists and is running/failed.
                let t = rs
                    .tasks
                    .iter()
                    .find(|t| t.id == task_id)
                    .ok_or_else(|| anyhow!("no task '{task_id}' in run '{run}'"))?;
                if t.status != state::Status::Running && t.status != state::Status::Failed {
                    bail!(
                        "task '{task_id}' has status {:?}; only running/failed tasks can be abandoned",
                        t.status
                    );
                }
                vec![task_id]
            } else if all_stuck {
                state::stuck_task_ids(&rs, cfg.stuck_ttl_secs)
            } else {
                bail!("specify --task <id> or --all-stuck");
            };

            if ids.is_empty() {
                eprintln!("nothing to abandon");
            } else {
                for id in &ids {
                    let t = rs.tasks.iter_mut().find(|t| t.id == *id).unwrap();
                    t.status = state::Status::Pending;
                    t.worktree = None;
                    t.branch = None;
                    t.updated_at = None;
                }
                rs.save(cfg, cwd)?;
                eprintln!("abandoned {} task(s): {}", ids.len(), ids.join(", "));
            }
        }
        StateAction::Pause { run } => {
            state::pause_run(cfg, cwd, &run)?;
        }
        StateAction::Resume { run } => {
            state::resume_run(cfg, cwd, &run)?;
        }
        StateAction::ListTasks => {
            let tasks = state::list_cancellable_tasks(cfg, cwd);
            println!("{}", serde_json::to_string_pretty(&tasks)?);
        }
        StateAction::Cancel { run, task } => {
            let mut rs = state::RunState::load(cfg, cwd, &run)?;
            let t = rs
                .tasks
                .iter_mut()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            if t.status == state::Status::Verified {
                bail!("task '{task}' is already verified and cannot be cancelled");
            }
            if t.status == state::Status::Cancelled {
                eprintln!("task '{task}' is already cancelled");
                return Ok(());
            }
            let was_running = t.status == state::Status::Running;
            let old_status = t.status;
            t.status = state::Status::Cancelled;
            t.worktree = None;
            t.branch = None;
            t.updated_at = Some(state::now_secs());
            rs.save(cfg, cwd)?;
            if was_running {
                eprintln!(
                    "cancelled task '{task}' (was running — in-flight worker may still \
                     complete; stop it manually if needed)"
                );
            } else {
                eprintln!("cancelled task '{task}' (was {:?})", old_status);
            }
            let (done, total) = rs.counts();
            eprintln!("run '{run}': {done}/{total} done");
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
         # command = \"cargo test\"\n\
         \n\
         # Loop feature: commands for `condukt loop` test-fix cycles.\n\
         # Use with /condukt-loop skill to run test→fix→test until all tests pass.\n\
         #   server cycle: deploy → test\n\
         #   client cycle: build  → test\n\
         #   e2e    cycle: build  → deploy → test\n\
         # [loop]\n\
         # build_command  = \"npm run build\"\n\
         # deploy_command = \"kubectl rollout restart deployment/api && kubectl rollout status deployment/api\"\n\
         # max_iters      = 10\n",
        cfg.worktree_base.display(),
        cfg.default_branch,
        cfg.max_parallel
    );
    std::fs::write(&cfg_path, default).with_context(|| format!("writing {}", cfg_path.display()))?;
    eprintln!("wrote {}", cfg_path.display());
    Ok(())
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.char_indices();
    match chars.nth(max_chars) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

fn default_run_id() -> String {
    chrono::Local::now().format("run-%Y%m%d-%H%M%S").to_string()
}

/// Auto-detect a terminal label: try `tty` output first, fall back to `pid-<PID>`.
fn resolve_label() -> Option<String> {
    let tty = std::process::Command::new("tty").output().ok()?;
    if tty.status.success() {
        let name = String::from_utf8_lossy(&tty.stdout).trim().to_string();
        if !name.is_empty() && name != "not a tty" {
            return Some(name);
        }
    }
    Some(format!("pid-{}", std::process::id()))
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

#[cfg(test)]
mod state_set_tests {
    use crate::state::{RunState, Status, TaskState};

    fn make_run(worktree: Option<&str>, branch: Option<&str>) -> RunState {
        RunState {
            run_id: "r1".into(),
            goal: "test".into(),
            tasks: vec![TaskState {
                id: "t1".into(),
                status: Status::Pending,
                worktree: worktree.map(str::to_string),
                branch: branch.map(str::to_string),
                updated_at: None,
            }],
            paused: false,
            terminal_label: None,
        }
    }

    /// worktree/branch が Some のタスクに worktree/branch なしで Set を実行しても
    /// 既存の worktree/branch が消えないこと (None 上書き保護)。
    #[test]
    fn state_set_none_does_not_overwrite_worktree_or_branch() {
        let mut rs = make_run(Some("/path/to/tree"), Some("feature/x"));
        let t = rs.tasks.iter_mut().find(|t| t.id == "t1").unwrap();

        // worktree/branch を None のまま (省略相当) でステータスだけ更新する。
        let new_worktree: Option<String> = None;
        let new_branch: Option<String> = None;
        let clear_worktree = false;
        let clear_branch = false;

        t.status = Status::Running;
        t.updated_at = Some(crate::state::now_secs());
        if clear_worktree {
            t.worktree = None;
        } else if new_worktree.is_some() {
            t.worktree = new_worktree;
        }
        if clear_branch {
            t.branch = None;
        } else if new_branch.is_some() {
            t.branch = new_branch;
        }

        assert_eq!(t.worktree.as_deref(), Some("/path/to/tree"), "worktree must be preserved");
        assert_eq!(t.branch.as_deref(), Some("feature/x"), "branch must be preserved");
        assert_eq!(t.status, Status::Running);
        assert!(t.updated_at.is_some());
    }

    /// --clear-worktree / --clear-branch フラグで既存値を明示的に消去できること。
    #[test]
    fn state_set_clear_flags_erase_worktree_and_branch() {
        let mut rs = make_run(Some("/path/to/tree"), Some("feature/x"));
        let t = rs.tasks.iter_mut().find(|t| t.id == "t1").unwrap();

        let clear_worktree = true;
        let clear_branch = true;
        let new_worktree: Option<String> = None;
        let new_branch: Option<String> = None;

        if clear_worktree {
            t.worktree = None;
        } else if new_worktree.is_some() {
            t.worktree = new_worktree;
        }
        if clear_branch {
            t.branch = None;
        } else if new_branch.is_some() {
            t.branch = new_branch;
        }

        assert!(t.worktree.is_none(), "worktree must be cleared");
        assert!(t.branch.is_none(), "branch must be cleared");
    }
}
