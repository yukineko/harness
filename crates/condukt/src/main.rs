//! condukt — deterministic orchestration engine for Claude Code.
//!
//! The LLM (the `/condukt` skill + interpreter/worker/verifier agents) does the
//! judgement work: interpret a request, decompose it into tasks, implement and
//! verify each. This binary does the deterministic work the LLM should not
//! eyeball: schedule tasks into parallel/serial batches by file-conflict
//! analysis, manage the git-worktree lifecycle, track run state, and gate
//! completion. Hooks (restore/statusline) never break a turn — they exit 0.

mod config;
mod consensus;
mod hooks;
mod install;
mod lock;
mod model;
mod oracle;
mod schedule;
mod state;
mod status;
mod store;
mod verify;
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
    /// Multi-sample self-consistency: plan an opt-in fan-out, or tally N verifier
    /// verdicts for one task into a majority winner + escalate-to-opus decision.
    Consensus {
        #[command(subcommand)]
        action: ConsensusAction,
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
    /// Show open runs and their tasks as an ASCII tree.
    Status {
        /// Include all runs, not just open ones.
        #[arg(long)]
        all: bool,
    },
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
enum ConsensusAction {
    /// Decide whether a task should fan out into N candidate implementations.
    /// OPT-IN: enabled only by config `[consensus] enabled` / `CONDUKT_CONSENSUS`,
    /// or a per-task `--risk high`. Prints `{enabled,samples,threshold,risk}` and
    /// exits 0 when a fan-out is warranted (so the skill can branch on the exit
    /// code, like `state autonomy-check`), 1 for the ordinary single-sample path.
    Plan {
        /// Per-task risk flag; "high"/"critical" forces fan-out even when the
        /// global switch is off.
        #[arg(long)]
        risk: Option<String>,
    },
    /// Tally N verifier verdicts (JSON on stdin or --file) for the same task into
    /// a majority winner, agreement rate, and escalate-to-opus decision. Prints
    /// the decision JSON; exits 0 when a consensus winner is taken, 1 when the
    /// caller should escalate (all-fail, tie, or agreement below threshold).
    ///
    /// Input is either a bare array of verdicts or an object
    /// `{"verdicts":[...],"threshold":0.5}`. Each verdict is
    /// `{"candidate":"<id>","pass":<bool>,"group":"<optional answer bucket>"}`.
    Vote {
        #[arg(long)]
        file: Option<PathBuf>,
        /// Agreement threshold override (CLI > JSON > config > built-in default).
        #[arg(long)]
        threshold: Option<f64>,
    },
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
        /// Model actually used to execute the task (recorded for fugu-router).
        /// Omitted → unchanged; falls back to the decomposition's suggested_model.
        #[arg(long)]
        model: Option<String>,
        /// Observed USD cost of the task (recorded for fugu-router). Omitted → unchanged.
        #[arg(long)]
        cost: Option<f64>,
    },
    /// Print a run's full state as JSON.
    Show {
        #[arg(long)]
        run: String,
    },
    /// Record completed runs' outcomes to fugu-router (the learning signal).
    /// Deterministic, idempotent: only settled, not-yet-recorded runs are emitted,
    /// and each run is marked so repeated firings (e.g. the Stop hook) never
    /// double-record. Soft: a no-op when the fugu-router binary is not on PATH.
    RecordRun {
        /// Record one specific run. Omit with --all to sweep every settled run.
        #[arg(long)]
        run: Option<String>,
        /// Record every settled, not-yet-recorded run for this project.
        #[arg(long)]
        all: bool,
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
    /// Run the mechanical gate for a task's done_criteria and print JSON.
    /// Exits 0 if the criteria is met (or non-mechanical), exits 1 if it fails.
    /// The JSON `skip_verifier` field is true ONLY for purely mechanical criteria
    /// that pass; behavioral criteria always report `skip_verifier:false`.
    CheckCriteria {
        #[arg(long)]
        run: String,
        #[arg(long)]
        task: String,
    },
    /// Deterministically ask `tdd oracle` whether a fix/feature task's RED→GREEN
    /// proofs form a valid Fail→Pass reproduction oracle. Fail-soft: no `tdd` on
    /// PATH, a spawn failure, a gone worktree, or corrupt/missing stdout all
    /// degrade to `fallback:true` ("use the legacy gate instead") rather than
    /// panicking. Tasks that don't require an oracle (not fix/feature, or no
    /// `reproduction_tests`) also report `required:false, fallback:true`.
    /// Always prints JSON and exits 0 — this is an advisory signal, not a gate.
    CheckOracle {
        #[arg(long)]
        run: String,
        #[arg(long)]
        task: String,
    },
    /// Report whether condukt is in autonomous mode (config.toml `autonomous` +
    /// `CONDUKT_AUTONOMOUS` env). Prints `{"autonomous":<bool>}` and exits 0 when
    /// autonomous, 1 when not — so the /condukt skill can branch on the exit code
    /// to skip human gates (e.g. the Phase 3 agreement) only when autonomous.
    AutonomyCheck,
    /// Resolve the verifier model so it never equals the worker model (shared
    /// blind-spot guard). Prints the chosen model on stdout. A distinct
    /// --suggested is honoured; otherwise a distinct tier is picked.
    VerifierModel {
        /// The model the worker used / will use for this task.
        #[arg(long)]
        worker: String,
        /// The verifier_model from route.json, if any (may equal --worker).
        #[arg(long)]
        suggested: Option<String>,
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
        Command::Consensus { action } => run_consensus(&cfg, action)?,
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
        Command::Status { all } => status::render(&cfg, &cwd, all),
        // These are dispatched as hooks in main() (via run_hook, which exits and
        // never returns here). Reaching this arm would be an internal dispatch
        // bug; return a clean error instead of panicking the process.
        Command::Restore | Command::Statusline => {
            bail!("internal: Restore/Statusline must be dispatched in main(), not run_user()")
        }
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

/// JSON input for `consensus vote`: either a bare array of verdicts, or an
/// object that may also carry a `threshold`.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum ConsensusInput {
    Wrapped {
        #[serde(default)]
        verdicts: Vec<consensus::Verdict>,
        #[serde(default)]
        threshold: Option<f64>,
    },
    Bare(Vec<consensus::Verdict>),
}

fn run_consensus(cfg: &Config, action: ConsensusAction) -> Result<()> {
    match action {
        ConsensusAction::Plan { risk } => {
            let plan = consensus::plan(
                cfg.consensus_enabled,
                cfg.consensus_samples,
                cfg.consensus_threshold,
                risk.as_deref(),
            );
            println!("{}", serde_json::to_string(&plan)?);
            // Exit-code contract (mirrors `state autonomy-check`): 0 = fan out,
            // 1 = ordinary single-sample path. The skill branches on this.
            if !plan.enabled {
                std::process::exit(1);
            }
        }
        ConsensusAction::Vote { file, threshold } => {
            let raw = match &file {
                Some(p) => std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?,
                None => read_stdin(),
            };
            let input: ConsensusInput =
                serde_json::from_str(&raw).context("parsing consensus verdicts JSON")?;
            let (verdicts, json_threshold) = match input {
                ConsensusInput::Wrapped {
                    verdicts,
                    threshold,
                } => (verdicts, threshold),
                ConsensusInput::Bare(v) => (v, None),
            };
            // Precedence: --threshold flag > JSON threshold > config > default.
            let thr = threshold
                .or(json_threshold)
                .unwrap_or(cfg.consensus_threshold);
            if !(0.0..=1.0).contains(&thr) {
                bail!("threshold must be within [0.0, 1.0], got {thr}");
            }
            let decision = consensus::tally(&verdicts, thr);
            println!("{}", serde_json::to_string_pretty(&decision)?);
            // Exit 1 when the caller should escalate (all-fail, tie, or agreement
            // below threshold); 0 when a consensus winner is taken. Distinct JSON
            // on stdout lets the skill tell this apart from a hard error.
            if decision.escalate {
                std::process::exit(1);
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
                    ..Default::default()
                })
                .collect();
            let rs = state::RunState {
                run_id: run_id.clone(),
                goal: dec.goal.clone(),
                tasks,
                paused: false,
                terminal_label: resolved_label,
                recorded_at: None,
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
            model,
            cost,
        } => {
            // Hold the per-run state lock across the entire load → oracle-gate →
            // mutate → save cycle so a concurrent session/worktree cannot lose
            // this update (last-writer-wins TOCTOU). Fail-soft: degrades to
            // unlocked on contention rather than failing the update.
            let _lock = lock::RunLock::acquire(cfg, cwd, &run);
            let mut rs = state::RunState::load(cfg, cwd, &run)?;
            let st: state::Status = status.parse()?;

            // F→P oracle completion gate: before promoting a task to
            // `verified`, ask `tdd oracle` (via `oracle::check_oracle`)
            // whether it carries a valid Fail→Pass reproduction proof. This
            // mirrors `CheckOracle`'s load + run_dir resolution exactly, but
            // fails soft (degrade to the legacy gate) whenever the
            // decomposition or matching task can't be found — a run-state
            // task with no matching decomposition entry must still be
            // settable to verified.
            let mut fp_gate_value: Option<bool> = None;
            if st == state::Status::Verified {
                if let Ok(dec_raw) = state::load_decomposition(cfg, cwd, &run) {
                    if let Ok(dec) = serde_json::from_str::<model::Decomposition>(&dec_raw) {
                        if let Some(dt) = dec.tasks.iter().find(|dt| dt.id == task) {
                            // Resolve run_dir from `rs` (immutable read) into a
                            // local *before* taking the `&mut` borrow below.
                            let run_dir = rs
                                .tasks
                                .iter()
                                .find(|s| s.id == task)
                                .and_then(|s| s.worktree.as_deref())
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| cwd.to_path_buf());
                            let verdict = oracle::check_oracle(
                                dt.requires_fp_oracle(),
                                dt.reproduction_tests.as_deref(),
                                &task,
                                &run_dir,
                            );
                            match state::enforce_fp_gate(&verdict) {
                                state::FpGateDecision::Reject => {
                                    bail!(
                                        "refusing to verify task '{task}': no valid fail-to-pass reproduction oracle ({})",
                                        verdict
                                            .get("reason")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("see `condukt state check-oracle` for details")
                                    );
                                }
                                state::FpGateDecision::Allow(v) => {
                                    fp_gate_value = v;
                                }
                            }
                        }
                    }
                }
            }

            let t = rs
                .tasks
                .iter_mut()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            t.status = st;
            t.updated_at = Some(state::now_secs());
            if st == state::Status::Verified {
                t.fp_oracle_valid = fp_gate_value;
            }
            // Record the model/cost used so the fugu-router outcome reflects the
            // truth (incl. escalation), not just the originally suggested model.
            if model.is_some() {
                t.model = model;
            }
            if cost.is_some() {
                t.cost_usd = cost;
            }
            // None 上書き保護: --worktree/--branch が省略された場合は既存値を保持する。
            // 明示的にクリアしたい場合は --clear-worktree / --clear-branch を使う。
            if clear_worktree {
                t.worktree = None;
            } else if worktree.is_some() {
                t.worktree = worktree;
            }
            if clear_branch {
                t.branch = None;
                t.branch_sha = None;
            } else if branch.is_some() {
                // Record the SHA at assignment time so reconcile can detect
                // force-push false-positives by checking the original commit.
                if let Some(ref b) = branch {
                    if t.branch_sha.is_none() {
                        let repo = worktree::toplevel(cwd).ok();
                        t.branch_sha = repo
                            .as_deref()
                            .and_then(|r| worktree::git(r, &["rev-parse", b]).ok())
                            .map(|s| s.trim().to_string());
                    }
                }
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
        StateAction::RecordRun { run, all } => {
            record_runs(cfg, cwd, run, all)?;
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
                println!(
                    "{}\t{}/{}\t{}{}{}",
                    rs.run_id, done, total, rs.goal, paused_tag, label_tag
                );
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
            let rate = (complete * 100).checked_div(total_runs).unwrap_or(0);
            eprintln!("condukt stats: {complete}/{total_runs} complete ({rate}%)\n");
            // Header
            println!("{:<32}  {:>6}  {:>6}  goal", "run_id", "done", "total");
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
            pairs.sort_by_key(|b| std::cmp::Reverse(b.1));
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
                let base = dec_map
                    .get(&ts.id)
                    .map(|t| serde_json::to_value(t).unwrap_or_default());
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
        StateAction::Abandon {
            run,
            task,
            all_stuck,
        } => {
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
                    let t = rs
                        .tasks
                        .iter_mut()
                        .find(|t| t.id == *id)
                        .ok_or_else(|| anyhow!("task '{id}' not found in run '{run}'"))?;
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
        StateAction::CheckCriteria { run, task } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            let dec_raw = state::load_decomposition(cfg, cwd, &run)?;
            let dec: model::Decomposition = serde_json::from_str(&dec_raw)?;
            let t = dec
                .tasks
                .iter()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            let Some(dc) = &t.done_criteria else {
                // No criteria to check → nothing mechanical, verifier still runs.
                println!("{{\"mechanical\":false,\"behavioral\":false,\"skip_verifier\":false}}");
                return Ok(());
            };
            let cls = verify::classify_criteria(dc);
            let run_dir = rs
                .tasks
                .iter()
                .find(|s| s.id == task)
                .and_then(|s| s.worktree.as_deref())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| cwd.to_path_buf());

            // Behavioral (or non-runnable) criteria: the LLM verifier MUST run.
            // Any embedded command is run only to attach evidence — never to
            // skip the verifier, and never to fail this gate.
            if !cls.skip_eligible {
                let evidence = cls.mechanical_cmd.as_ref().map(|cmd| {
                    let (passed, output) = run_mechanical(cmd, &run_dir);
                    serde_json::json!({ "cmd": cmd, "passed": passed, "output": output })
                });
                let out = serde_json::json!({
                    "mechanical": cls.mechanical_cmd.is_some(),
                    "behavioral": cls.behavioral,
                    "skip_verifier": false,
                    "evidence": evidence,
                });
                println!("{}", serde_json::to_string(&out)?);
                return Ok(());
            }

            // Purely mechanical criteria: the verifier may be skipped iff the
            // command passes. A failing mechanical check fails this gate. If the
            // skip_eligible invariant is ever violated (no command), this fails
            // soft to skip_verifier:false rather than panicking an unattended run.
            let (out, gate_failed) =
                verify::mechanical_skip_verdict(&cls, |cmd| run_mechanical(cmd, &run_dir));
            println!("{}", serde_json::to_string(&out)?);
            if gate_failed {
                std::process::exit(1);
            }
        }
        StateAction::CheckOracle { run, task } => {
            let rs = state::RunState::load(cfg, cwd, &run)?;
            let dec_raw = state::load_decomposition(cfg, cwd, &run)?;
            let dec: model::Decomposition = serde_json::from_str(&dec_raw)?;
            let t = dec
                .tasks
                .iter()
                .find(|t| t.id == task)
                .ok_or_else(|| anyhow!("no task '{task}' in run '{run}'"))?;
            let run_dir = rs
                .tasks
                .iter()
                .find(|s| s.id == task)
                .and_then(|s| s.worktree.as_deref())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| cwd.to_path_buf());
            let out = oracle::check_oracle(
                t.requires_fp_oracle(),
                t.reproduction_tests.as_deref(),
                &task,
                &run_dir,
            );
            println!("{}", serde_json::to_string(&out)?);
        }
        StateAction::VerifierModel { worker, suggested } => {
            let chosen = verify::resolve_verifier_model(&worker, suggested.as_deref());
            // Invariant guard: never emit a verifier model equal to the worker.
            debug_assert!(!verify::same_model(&chosen, &worker));
            println!("{chosen}");
        }
        StateAction::AutonomyCheck => {
            let autonomous = cfg.autonomous;
            println!("{{\"autonomous\":{autonomous}}}");
            if !autonomous {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

/// Run a mechanical check command in `run_dir`, returning (passed, combined output).
/// A spawn failure counts as not-passed with the error text as output.
fn run_mechanical(cmd: &[String], run_dir: &Path) -> (bool, String) {
    match std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(run_dir)
        .output()
    {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            (out.status.success(), combined)
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Probe whether the `fugu-router` binary is on PATH and, if so, return its
/// skill fingerprint (stdout of `fugu-router fingerprint`, trimmed). `None`
/// means fugu-router is absent → recording is a soft no-op.
fn fugu_fingerprint() -> Option<String> {
    let out = std::process::Command::new("fugu-router")
        .arg("fingerprint")
        .output()
        .ok()?; // spawn failed (not on PATH) → soft-skip
    let fp = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(fp)
}

/// Record completed runs' outcomes to fugu-router. Deterministic and idempotent:
/// only settled, not-yet-recorded runs emit episodes, and each is stamped with
/// `recorded_at` so repeated firings (the Stop hook) never double-record.
///
/// Soft dependency: when fugu-router is not on PATH this is a clean no-op and
/// does NOT mark runs recorded, so a later invocation (once fugu-router exists)
/// still captures the signal.
fn record_runs(cfg: &Config, cwd: &Path, run: Option<String>, all: bool) -> Result<()> {
    let run_ids: Vec<String> = match (run, all) {
        (Some(r), _) => vec![r],
        (None, true) => state::all_runs(cfg, cwd)
            .into_iter()
            .map(|rs| rs.run_id)
            .collect(),
        (None, false) => bail!("record-run requires --run <id> or --all"),
    };

    // Probe fugu-router once. Absent → soft no-op (leave runs unrecorded).
    let fingerprint = match fugu_fingerprint() {
        Some(fp) => fp,
        None => return Ok(()),
    };

    let mut emitted = 0usize;
    let mut recorded_runs = 0usize;
    for rid in run_ids {
        let mut rs = match state::RunState::load(cfg, cwd, &rid) {
            Ok(rs) => rs,
            Err(_) => continue, // run vanished between listing and load
        };
        // Need the decomposition to recover title/files/class/suggested_model.
        let dec = match state::load_decomposition(cfg, cwd, &rid) {
            Ok(raw) => match serde_json::from_str::<model::Decomposition>(&raw) {
                Ok(d) => d,
                Err(_) => continue, // corrupt sidecar → skip (leave unrecorded)
            },
            Err(_) => continue, // pre-feature run without a sidecar → skip
        };
        let Some(specs) = state::records_for_run(&rs, &dec) else {
            continue; // not settled, or already recorded
        };
        for s in &specs {
            let mut cmd = std::process::Command::new("fugu-router");
            cmd.arg("record")
                .args(["--title", &s.title])
                .args(["--files", &s.files.join(",")])
                .args(["--class", &s.class])
                .args(["--model", &s.model])
                .args(["--status", &s.status])
                .args(["--cost", &s.cost_usd.to_string()]);
            if let Some(dc) = &s.done_criteria {
                cmd.args(["--done-criteria", dc]);
            }
            if !fingerprint.is_empty() {
                cmd.args(["--skill-fingerprint", &fingerprint]);
            }
            // Best-effort: a single failed record must not abort the sweep.
            if let Err(e) = cmd.status() {
                eprintln!("condukt: fugu-router record failed for '{}': {e}", s.title);
            } else {
                emitted += 1;
            }
        }
        rs.recorded_at = Some(state::now_secs());
        rs.save(cfg, cwd)?;
        recorded_runs += 1;
    }

    if emitted > 0 {
        eprintln!(
            "condukt: recorded {emitted} outcome(s) from {recorded_runs} run(s) to fugu-router"
        );
    }
    Ok(())
}

fn init(cfg: &Config) -> Result<()> {
    let base = config::base_dir();
    // Loud-fail: a swallowed mkdir here (e.g. a read-only or non-writable parent)
    // used to surface much later as a cryptic I/O error when run-state or a
    // worktree was first written. Name the dir so the root cause is obvious.
    std::fs::create_dir_all(&cfg.state_dir)
        .with_context(|| format!("could not create state dir {}", cfg.state_dir.display()))?;
    std::fs::create_dir_all(&cfg.worktree_base).with_context(|| {
        format!(
            "could not create worktree base {}",
            cfg.worktree_base.display()
        )
    })?;
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
         # max_iters      = 10\n\
         \n\
         # Multi-sample self-consistency (COST GUARD: opt-in, OFF by default).\n\
         # When enabled, a high-risk task is implemented N independent times,\n\
         # each verified, and a majority vote picks the winner; low agreement\n\
         # escalates the task to opus. N-sample generation is N x the cost, so\n\
         # keep this off unless you want fan-out consensus on risky tasks. A\n\
         # per-task `condukt consensus plan --risk high` forces fan-out even when\n\
         # enabled=false. samples is clamped to a ceiling of 5.\n\
         # [consensus]\n\
         # enabled   = false\n\
         # samples   = 3\n\
         # threshold = 0.5\n",
        cfg.worktree_base.display(),
        cfg.default_branch,
        cfg.max_parallel
    );
    std::fs::write(&cfg_path, default)
        .with_context(|| format!("writing {}", cfg_path.display()))?;
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
                branch_sha: None,
                updated_at: None,
                model: None,
                cost_usd: None,
                fp_oracle_valid: None,
            }],
            paused: false,
            terminal_label: None,
            recorded_at: None,
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

        assert_eq!(
            t.worktree.as_deref(),
            Some("/path/to/tree"),
            "worktree must be preserved"
        );
        assert_eq!(
            t.branch.as_deref(),
            Some("feature/x"),
            "branch must be preserved"
        );
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
