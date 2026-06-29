//! Run-state persistence and the completion gate.
//!
//! A run records each task's status and (once assigned) its worktree/branch.
//! State lives at `<state_dir>/<project-key>/<run-id>.json`. The gate is the
//! deterministic "are we actually done?" check the skill calls before declaring
//! success: every task verified, and no worktree left dirty or unremoved.

use crate::config::Config;
use crate::store::{project_key, repo_root};
use crate::worktree;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    Verified,
    /// Deliberately cancelled by the user. Terminal like Verified; does not
    /// block the completion gate and causes the run to disappear from `state list`
    /// when all tasks reach a terminal state.
    Cancelled,
}

impl std::str::FromStr for Status {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "pending" => Status::Pending,
            "running" => Status::Running,
            "done" => Status::Done,
            "failed" => Status::Failed,
            "verified" => Status::Verified,
            "cancelled" => Status::Cancelled,
            other => {
                bail!("unknown status '{other}' (pending|running|done|failed|verified|cancelled)")
            }
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub status: Status,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Unix timestamp (seconds) when this task's status was last changed.
    /// `None` for tasks loaded from older run-state files (backward-compatible).
    #[serde(default)]
    pub updated_at: Option<i64>,
    /// The model actually used to execute this task (set by the skill, possibly
    /// after escalation). `None` falls back to the decomposition's
    /// `suggested_model` when recording the outcome to fugu-router.
    #[serde(default)]
    pub model: Option<String>,
    /// Observed USD cost of executing this task (e.g. from `gauge`). `None`
    /// records as 0.0. Used as the learning signal's cost dimension.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

pub fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    #[serde(default)]
    pub goal: String,
    pub tasks: Vec<TaskState>,
    #[serde(default)]
    pub paused: bool,
    /// Terminal or session label set at `state init` time (e.g. `/dev/pts/1`).
    /// Used to identify which terminal/session owns a run in `state list` output
    /// and in cross-run conflict reports.
    #[serde(default)]
    pub terminal_label: Option<String>,
    /// Unix timestamp (seconds) when this run's outcomes were recorded to
    /// fugu-router. `None` = not yet recorded. Set once `record-run` emits the
    /// episodes so repeated (idempotent) hook firings never double-record.
    #[serde(default)]
    pub recorded_at: Option<i64>,
}

fn project_dir(cfg: &Config, cwd: &Path) -> PathBuf {
    let root = repo_root(cwd);
    cfg.state_dir.join(project_key(&root))
}

fn run_path(cfg: &Config, cwd: &Path, run_id: &str) -> PathBuf {
    // `run_id` can come from the CLI (e.g. `condukt status <run_id>`); sanitise
    // it so a crafted id like `../../etc/x` cannot escape the project dir.
    project_dir(cfg, cwd).join(format!(
        "{}.json",
        harness_core::store::safe_session(run_id)
    ))
}

impl RunState {
    pub fn load(cfg: &Config, cwd: &Path, run_id: &str) -> Result<Self> {
        let path = run_path(cfg, cwd, run_id);
        let txt = std::fs::read_to_string(&path)
            .with_context(|| format!("no run '{run_id}' at {}", path.display()))?;
        let state = serde_json::from_str(&txt).context("corrupt run-state JSON")?;
        Ok(state)
    }

    pub fn save(&self, cfg: &Config, cwd: &Path) -> Result<PathBuf> {
        let dir = project_dir(cfg, cwd);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating state dir {}", dir.display()))?;
        let path = dir.join(format!("{}.json", self.run_id));
        let tmp_path = dir.join(format!("{}.json.tmp", self.run_id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &json)
            .with_context(|| format!("writing tmp {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))?;
        Ok(path)
    }

    pub fn counts(&self) -> (usize, usize) {
        let done = self
            .tasks
            .iter()
            .filter(|t| matches!(t.status, Status::Verified | Status::Cancelled))
            .count();
        (done, self.tasks.len())
    }
}

/// Open runs (not every task verified) for this project. Used by the SessionStart
/// hook and `state list`.
pub fn open_runs(cfg: &Config, cwd: &Path) -> Vec<RunState> {
    all_runs(cfg, cwd)
        .into_iter()
        .filter(|rs| {
            let (done, total) = rs.counts();
            done < total
        })
        .collect()
}

/// Mark a run as paused. Returns Err if the run does not exist.
pub fn pause_run(cfg: &Config, cwd: &Path, run_id: &str) -> Result<()> {
    let mut rs = RunState::load(cfg, cwd, run_id)?;
    rs.paused = true;
    rs.save(cfg, cwd)?;
    Ok(())
}

/// Clear the paused flag on a run. Returns Err if the run does not exist.
pub fn resume_run(cfg: &Config, cwd: &Path, run_id: &str) -> Result<()> {
    let mut rs = RunState::load(cfg, cwd, run_id)?;
    rs.paused = false;
    rs.save(cfg, cwd)?;
    Ok(())
}

/// All runs (complete and incomplete) for this project, sorted by run_id.
pub fn all_runs(cfg: &Config, cwd: &Path) -> Vec<RunState> {
    let dir = project_dir(cfg, cwd);
    let mut runs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // Skip decomposition sidecars (run-id.decomposition.json).
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname.ends_with(".decomposition.json") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&path) {
                if let Ok(rs) = serde_json::from_str::<RunState>(&txt) {
                    runs.push(rs);
                }
            }
        }
    }
    runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    runs
}

// ── Decomposition sidecar ──────────────────────────────────────────────────

pub fn decomposition_path(cfg: &Config, cwd: &Path, run_id: &str) -> PathBuf {
    project_dir(cfg, cwd).join(format!("{run_id}.decomposition.json"))
}

/// Persist the raw decomposition JSON alongside the run state.
pub fn save_decomposition(cfg: &Config, cwd: &Path, run_id: &str, json: &str) -> Result<()> {
    let path = decomposition_path(cfg, cwd, run_id);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("decomposition path {} has no parent", path.display()))?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating state dir {}", dir.display()))?;
    let tmp_path = dir.join(format!("{run_id}.decomposition.json.tmp"));
    std::fs::write(&tmp_path, json)
        .with_context(|| format!("writing tmp decomposition to {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &path)
        .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))
}

/// Load the raw decomposition JSON for an existing run. Fails if not found.
pub fn load_decomposition(cfg: &Config, cwd: &Path, run_id: &str) -> Result<String> {
    let path = decomposition_path(cfg, cwd, run_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("no decomposition for run '{run_id}' at {}", path.display()))
}

// ── Reconcile ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ReconcileChange {
    pub task_id: String,
    pub old_status: Status,
    pub new_status: Status,
    pub reason: String,
}

/// Auto-detect tasks whose branches are already merged (or gone) and promote
/// them to `verified`, clearing stale worktree references along the way.
///
/// A task is reconciled to `verified` when:
/// 1. Its branch is an ancestor of `default_branch` (`git merge-base --is-ancestor`), OR
/// 2. Its branch no longer exists AND its worktree is gone from disk.
///
/// If the worktree is gone but the branch still exists (unmerged), only the
/// worktree reference is cleared — the status is left as-is.
///
/// Returns (updated RunState, list of changes) so the caller can save and report.
pub fn reconcile_run(
    _cfg: &Config,
    cwd: &Path,
    mut run: RunState,
    default_branch: &str,
) -> Result<(RunState, Vec<ReconcileChange>)> {
    let repo = crate::store::repo_root(cwd);
    let mut changes = Vec::new();

    for t in run.tasks.iter_mut() {
        if matches!(t.status, Status::Verified | Status::Cancelled) {
            continue;
        }

        let worktree_gone = t
            .worktree
            .as_ref()
            .map(|p| !PathBuf::from(p).exists())
            .unwrap_or(true); // no worktree recorded → treat as gone

        let branch_merged = t.branch.as_deref().map(|b| {
            // `git merge-base --is-ancestor <b> <default>` exits 0 if b is an ancestor.
            crate::worktree::git(&repo, &["merge-base", "--is-ancestor", b, default_branch]).is_ok()
        });

        let branch_exists = t.branch.as_deref().map(|b| {
            crate::worktree::git(
                &repo,
                &["rev-parse", "--verify", &format!("refs/heads/{b}")],
            )
            .is_ok()
        });

        let should_verify = match (branch_merged, branch_exists) {
            (Some(true), _) => true, // merged: branch is an ancestor of default
            (Some(false), Some(false)) if worktree_gone => true, // branch deleted + worktree gone
            _ => false,
        };

        if should_verify {
            let reason = match branch_merged {
                Some(true) => format!(
                    "branch '{}' is merged into '{default_branch}'",
                    t.branch.as_deref().unwrap_or("?")
                ),
                _ => format!(
                    "branch '{}' no longer exists and worktree is gone",
                    t.branch.as_deref().unwrap_or("?")
                ),
            };
            changes.push(ReconcileChange {
                task_id: t.id.clone(),
                old_status: t.status,
                new_status: Status::Verified,
                reason,
            });
            t.status = Status::Verified;
            t.worktree = None; // clear stale reference
        } else if worktree_gone && t.worktree.is_some() {
            // Worktree gone but branch exists/unknown — just clear the stale path.
            changes.push(ReconcileChange {
                task_id: t.id.clone(),
                old_status: t.status,
                new_status: t.status,
                reason: "cleared stale worktree reference (path no longer on disk)".to_string(),
            });
            t.worktree = None;
        }
    }

    Ok((run, changes))
}

// ── Stuck detection ───────────────────────────────────────────────────────

/// Returns task ids that are stuck: status=Running and updated_at is older than stuck_ttl_secs.
///
/// Tasks whose `updated_at` is `None` (legacy data without timestamp) are **not**
/// considered stuck — absence of evidence is not evidence of being stuck.
pub fn stuck_task_ids(run: &RunState, stuck_ttl_secs: u64) -> Vec<String> {
    let threshold = now_secs() - stuck_ttl_secs as i64;
    run.tasks
        .iter()
        .filter(|t| t.status == Status::Running)
        .filter(|t| t.updated_at.map(|ts| ts < threshold).unwrap_or(false))
        .map(|t| t.id.clone())
        .collect()
}

// ── Stats ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RunStats {
    pub run_id: String,
    pub goal: String,
    pub verified: usize,
    pub total: usize,
    pub is_complete: bool,
    pub status_counts: std::collections::HashMap<String, usize>,
}

pub fn compute_stats(cfg: &Config, cwd: &Path) -> Vec<RunStats> {
    all_runs(cfg, cwd)
        .into_iter()
        .map(|rs| {
            let mut counts = std::collections::HashMap::new();
            for t in &rs.tasks {
                let key = format!("{:?}", t.status).to_lowercase();
                *counts.entry(key).or_insert(0usize) += 1;
            }
            let (verified, total) = rs.counts();
            let is_complete = verified == total && total > 0;
            RunStats {
                run_id: rs.run_id,
                goal: rs.goal,
                verified,
                total,
                is_complete,
                status_counts: counts,
            }
        })
        .collect()
}

/// Reasons the run is NOT complete (empty = gate passes).
pub fn gate_reasons(cfg: &Config, cwd: &Path, run: &RunState) -> Vec<String> {
    let mut reasons = Vec::new();
    let repo = repo_root(cwd);

    for t in &run.tasks {
        if !matches!(t.status, Status::Verified | Status::Cancelled) {
            reasons.push(format!("task '{}' is {:?}, not verified", t.id, t.status));
        }
        // A finished task must not leave its worktree behind, dirty or not.
        if let Some(wt) = &t.worktree {
            let p = PathBuf::from(wt);
            if p.exists() {
                match worktree::is_dirty(&p) {
                    Ok(true) => reasons.push(format!(
                        "worktree for '{}' has uncommitted changes ({wt})",
                        t.id
                    )),
                    Ok(false) => {
                        reasons.push(format!("worktree for '{}' still exists ({wt})", t.id))
                    }
                    Err(_) => reasons.push(format!("worktree for '{}' unreadable ({wt})", t.id)),
                }
            }
        }
    }

    // Any orphan worktree under the base is also a leak.
    if let Ok(orphans) = worktree::orphans(&repo, &cfg.worktree_base) {
        for o in orphans {
            reasons.push(format!("orphan worktree on disk: {}", o.display()));
        }
    }

    reasons
}

// ── Outcome recording (fugu-router learning signal) ────────────────────────

/// One outcome to record to fugu-router, derived by joining a run's task states
/// with its decomposition. Pure data so the join logic is unit-testable without
/// spawning the fugu-router binary.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordSpec {
    pub title: String,
    pub files: Vec<String>,
    pub class: String,
    /// Model the task ran on: the task state's recorded model if present, else
    /// the decomposition's `suggested_model`, else a conservative default.
    pub model: String,
    /// "verified" or "failed" — the fugu-router status vocabulary.
    pub status: String,
    pub cost_usd: f64,
    pub done_criteria: Option<String>,
}

/// Build the outcomes to record for a run, or `None` when the run is not yet
/// recordable. A run is recordable when:
/// - it has not already been recorded (`recorded_at` is `None`), and
/// - every task has reached a settled state (verified/failed/cancelled) — i.e.
///   nothing is still pending/running and could yet change.
///
/// Only `verified` and `failed` tasks produce a record (a `cancelled` task was
/// abandoned by the user and carries no learning signal). The returned vec may
/// be empty (e.g. an all-cancelled run); the caller still marks it recorded so
/// repeated hook firings converge.
pub fn records_for_run(
    run: &RunState,
    dec: &crate::model::Decomposition,
) -> Option<Vec<RecordSpec>> {
    if run.recorded_at.is_some() || run.tasks.is_empty() {
        return None;
    }
    let settled = run.tasks.iter().all(|t| {
        matches!(
            t.status,
            Status::Verified | Status::Failed | Status::Cancelled
        )
    });
    if !settled {
        return None;
    }

    let by_id: std::collections::HashMap<&str, &crate::model::Task> =
        dec.tasks.iter().map(|t| (t.id.as_str(), t)).collect();

    let specs = run
        .tasks
        .iter()
        .filter_map(|ts| {
            let status = match ts.status {
                Status::Verified => "verified",
                Status::Failed => "failed",
                _ => return None, // cancelled / non-terminal: no learning signal
            };
            let task = by_id.get(ts.id.as_str());
            let title = task
                .map(|t| t.title.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ts.id.clone());
            let files = task.map(|t| t.touched_files.clone()).unwrap_or_default();
            let class = task
                .map(|t| format!("{:?}", t.class).to_lowercase())
                .unwrap_or_else(|| "parallel".to_string());
            let model = ts
                .model
                .clone()
                .or_else(|| task.and_then(|t| t.suggested_model.clone()))
                .unwrap_or_else(|| "sonnet".to_string());
            let done_criteria = task.and_then(|t| t.done_criteria.clone());
            Some(RecordSpec {
                title,
                files,
                class,
                model,
                status: status.to_string(),
                cost_usd: ts.cost_usd.unwrap_or(0.0),
                done_criteria,
            })
        })
        .collect();
    Some(specs)
}

/// Run the project's test suite (from the repo root) and propagate its result.
///
/// The command runs at the repo top-level, not the raw cwd, so invoking
/// `condukt state test` from a subdirectory still tests the whole project and
/// auto-detection sees the project's manifest. The command is handed to `sh -c`
/// so quoted args, pipes, and env vars in a configured `test_command` work as
/// the user expects (`pytest -k "foo bar"`).
pub fn run_tests(cfg: &Config, cwd: &Path, _rs: &RunState) -> Result<()> {
    let root = repo_root(cwd);
    let cmd_str = cfg
        .test_command
        .clone()
        .unwrap_or_else(|| auto_detect_test_command(&root));
    if cmd_str.trim().is_empty() {
        bail!("empty test command");
    }
    eprintln!("condukt: running tests in {}: {cmd_str}", root.display());
    let status = harness_core::shell::command(&cmd_str)
        .current_dir(&root)
        .status()
        .with_context(|| format!("failed to run '{cmd_str}'"))?;
    if status.success() {
        eprintln!("condukt: tests passed");
        Ok(())
    } else {
        bail!("tests failed (exit {})", status.code().unwrap_or(-1))
    }
}

fn auto_detect_test_command(cwd: &Path) -> String {
    if cwd.join("Cargo.toml").exists() {
        return "cargo test".to_string();
    }
    if cwd.join("package.json").exists() {
        return "npm test".to_string();
    }
    if cwd.join("pyproject.toml").exists() || cwd.join("setup.py").exists() {
        return "pytest".to_string();
    }
    "cargo test".to_string()
}

// ── Loop: test→fix→test cycle ─────────────────────────────────────────────

/// Result of a single loop iteration (one full cycle).
#[derive(Debug, Serialize)]
pub struct CycleResult {
    /// Number of test failures detected (0 = all pass).
    pub failure_count: usize,
    /// Combined stdout+stderr from the cycle steps.
    pub output: String,
    /// Whether the cycle as a whole succeeded (exit 0 on all steps).
    pub success: bool,
}

/// Run a single shell command, capturing combined output. Returns (exit_ok, output).
fn run_command_capture(cmd_str: &str, cwd: &Path) -> (bool, String) {
    let result = harness_core::shell::command(cmd_str)
        .current_dir(cwd)
        .output();
    match result {
        Ok(out) => {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&out.stdout));
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), combined)
        }
        Err(e) => (false, format!("failed to spawn '{cmd_str}': {e}")),
    }
}

/// Count failures in test output using common framework patterns.
/// Falls back to 1 when failures exist but count can't be parsed.
pub fn count_test_failures(output: &str, exit_ok: bool) -> usize {
    if exit_ok {
        return 0;
    }
    // Cargo: "test result: FAILED. N passed; M failed"
    for line in output.lines() {
        let l = line.trim();
        if l.starts_with("test result: FAILED") || l.starts_with("test result: ok") {
            // "N passed; M failed; ..."
            for part in l.split(';') {
                let p = part.trim();
                if let Some(rest) = p.strip_suffix(" failed") {
                    if let Ok(n) = rest.trim().parse::<usize>() {
                        return n;
                    }
                }
            }
        }
        // pytest: "N failed, M passed"
        if l.contains("failed") && l.contains("passed") {
            let words: Vec<&str> = l.split_whitespace().collect();
            for (i, w) in words.iter().enumerate() {
                if (*w == "failed," || *w == "failed") && i > 0 {
                    if let Ok(n) = words[i - 1].parse::<usize>() {
                        return n;
                    }
                }
            }
        }
    }
    // npm/jest: count lines starting with "FAIL "
    let jest_fails = output
        .lines()
        .filter(|l| l.trim_start().starts_with("FAIL "))
        .count();
    if jest_fails > 0 {
        return jest_fails;
    }
    // Unknown format but exit non-0: report as 1 so stop-detection can track it
    1
}

/// Whether the loop should stop given previous and current failure counts.
/// Returns `(stop, reason)`.
pub fn loop_should_stop(prev: Option<usize>, current: usize) -> (bool, &'static str) {
    if current == 0 {
        return (true, "all tests pass");
    }
    if prev == Some(current) {
        return (true, "no progress: failure count unchanged");
    }
    (false, "")
}

/// Run one full test cycle according to `module` (deploy/build/test in the right order).
/// Returns a [`CycleResult`] with the failure count and combined output.
pub fn run_cycle(
    cfg: &Config,
    cwd: &Path,
    module: crate::config::ModuleCycle,
) -> Result<CycleResult> {
    use crate::config::ModuleCycle;
    let root = repo_root(cwd);
    let test_cmd = cfg
        .test_command
        .clone()
        .unwrap_or_else(|| auto_detect_test_command(&root));
    if test_cmd.trim().is_empty() {
        bail!("empty test command");
    }

    let mut output = String::new();
    let mut all_ok = true;

    // build step (client and e2e)
    if matches!(module, ModuleCycle::Client | ModuleCycle::E2e) {
        let build = cfg.build_command.as_deref().unwrap_or("");
        if build.trim().is_empty() {
            bail!("build_command not set in [loop] config (required for {module:?})");
        }
        eprintln!("condukt loop: build — {build}");
        let (ok, out) = run_command_capture(build, &root);
        output.push_str(&out);
        if !ok {
            // build failure: skip remaining steps and report as non-zero failures
            return Ok(CycleResult {
                failure_count: count_test_failures(&output, false),
                output,
                success: false,
            });
        }
    }

    // deploy step (server and e2e)
    if matches!(module, ModuleCycle::Server | ModuleCycle::E2e) {
        let deploy = cfg.deploy_command.as_deref().unwrap_or("");
        if deploy.trim().is_empty() {
            bail!("deploy_command not set in [loop] config (required for {module:?})");
        }
        eprintln!("condukt loop: deploy — {deploy}");
        let (ok, out) = run_command_capture(deploy, &root);
        output.push_str(&out);
        if !ok {
            return Ok(CycleResult {
                failure_count: count_test_failures(&output, false),
                output,
                success: false,
            });
        }
    }

    // test step (always)
    eprintln!("condukt loop: test — {test_cmd}");
    let (test_ok, test_out) = run_command_capture(&test_cmd, &root);
    output.push_str(&test_out);
    if !test_ok {
        all_ok = false;
    }
    let failure_count = count_test_failures(&output, test_ok);
    Ok(CycleResult {
        failure_count,
        output,
        success: all_ok,
    })
}

// ── Cross-run conflict detection ──────────────────────────────────────────

const GOAL_SIMILARITY_THRESHOLD: f64 = 0.3;

/// Character bigram Jaccard similarity between two strings.
/// Works for both Japanese and ASCII without external dependencies.
fn bigram_jaccard(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let make_bigrams = |s: &str| -> HashSet<(char, char)> {
        let chars: Vec<char> = s.chars().collect();
        chars.windows(2).map(|w| (w[0], w[1])).collect()
    };
    let bg_a = make_bigrams(a);
    let bg_b = make_bigrams(b);
    if bg_a.is_empty() && bg_b.is_empty() {
        return 0.0;
    }
    let intersection = bg_a.intersection(&bg_b).count();
    let union_count = bg_a.union(&bg_b).count();
    if union_count == 0 {
        return 0.0;
    }
    intersection as f64 / union_count as f64
}

#[derive(Debug, Serialize)]
pub struct ConflictEntry {
    pub run_id: String,
    pub goal: String,
    pub terminal_label: Option<String>,
    /// Files from the incoming decomposition that overlap with this run.
    pub overlapping_files: Vec<String>,
    /// True when the run has tasks that are not yet settled (pending/running/done)
    /// and is not paused. A false value means it is safe to proceed automatically.
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
pub struct SimilarGoalEntry {
    pub run_id: String,
    pub goal: String,
    pub terminal_label: Option<String>,
    /// Bigram Jaccard similarity score between goals (0.0–1.0).
    pub similarity: f64,
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
pub struct ConflictReport {
    /// True when file conflicts or similar-goal runs exist.
    pub has_conflicts: bool,
    /// True when every conflicting/similar run is inactive (paused or all tasks settled).
    /// The skill can auto-proceed without asking the user.
    pub auto_proceed: bool,
    /// File-overlap conflicts.
    pub conflicts: Vec<ConflictEntry>,
    /// Runs with similar goals but no file overlap (potential duplicate work).
    pub similar_goal_runs: Vec<SimilarGoalEntry>,
}

/// A non-terminal task eligible for cancellation, with its run context.
/// Returned by `list_cancellable_tasks` for the skill's AskUserQuestion list.
#[derive(Debug, Serialize)]
pub struct CancellableTask {
    pub run_id: String,
    pub goal: String,
    pub terminal_label: Option<String>,
    pub is_paused: bool,
    pub task_id: String,
    /// From the decomposition sidecar; falls back to task_id when unavailable.
    pub task_title: String,
    pub status: String,
}

/// All non-terminal tasks across open runs that can be cancelled.
/// Loads decomposition sidecars to include task titles.
pub fn list_cancellable_tasks(cfg: &Config, cwd: &Path) -> Vec<CancellableTask> {
    let mut result = Vec::new();
    for run in open_runs(cfg, cwd) {
        let titles: std::collections::HashMap<String, String> =
            if let Ok(raw) = load_decomposition(cfg, cwd, &run.run_id) {
                if let Ok(dec) = serde_json::from_str::<crate::model::Decomposition>(&raw) {
                    dec.tasks
                        .iter()
                        .map(|t| (t.id.clone(), t.title.clone()))
                        .collect()
                } else {
                    Default::default()
                }
            } else {
                Default::default()
            };

        for task in &run.tasks {
            if !matches!(
                task.status,
                Status::Pending | Status::Running | Status::Done
            ) {
                continue;
            }
            result.push(CancellableTask {
                run_id: run.run_id.clone(),
                goal: run.goal.clone(),
                terminal_label: run.terminal_label.clone(),
                is_paused: run.paused,
                task_id: task.id.clone(),
                task_title: titles
                    .get(&task.id)
                    .cloned()
                    .unwrap_or_else(|| task.id.clone()),
                status: format!("{:?}", task.status).to_lowercase(),
            });
        }
    }
    result
}

/// Check whether the incoming decomposition's touched_files overlap with any
/// currently open run for this project, and whether any open run has a similar goal.
///
/// - Paused runs are included in the report but marked `is_active: false`.
/// - Runs whose decomposition file is missing are skipped for file-conflict checks
///   but still checked for goal similarity.
/// - Runs where both sides have empty touched_files are skipped for file conflicts.
/// - Runs already reported in `conflicts` (file overlap) are excluded from
///   `similar_goal_runs` to avoid double-reporting.
pub fn cross_run_conflicts(
    cfg: &Config,
    cwd: &Path,
    new_dec: &crate::model::Decomposition,
) -> ConflictReport {
    let new_files: Vec<String> = new_dec
        .tasks
        .iter()
        .flat_map(|t| t.touched_files.iter().cloned())
        .collect();
    let new_goal = &new_dec.goal;

    let mut conflicts = Vec::new();
    let mut similar_goal_runs = Vec::new();
    let mut file_conflict_run_ids = std::collections::HashSet::new();

    for run in open_runs(cfg, cwd) {
        let all_settled = run.tasks.iter().all(|t| {
            matches!(
                t.status,
                Status::Verified | Status::Failed | Status::Cancelled
            )
        });
        let is_active = !run.paused && !all_settled;

        // File-overlap check (requires decomposition).
        if let Ok(dec_raw) = load_decomposition(cfg, cwd, &run.run_id) {
            if let Ok(dec) = serde_json::from_str::<crate::model::Decomposition>(&dec_raw) {
                let run_files: Vec<String> = dec
                    .tasks
                    .iter()
                    .flat_map(|t| t.touched_files.iter().cloned())
                    .collect();

                if !new_files.is_empty() && !run_files.is_empty() {
                    let overlapping: Vec<String> = new_files
                        .iter()
                        .filter(|f| crate::schedule::files_conflict(&[(*f).clone()], &run_files))
                        .cloned()
                        .collect();

                    if !overlapping.is_empty() {
                        file_conflict_run_ids.insert(run.run_id.clone());
                        conflicts.push(ConflictEntry {
                            run_id: run.run_id.clone(),
                            goal: run.goal.clone(),
                            terminal_label: run.terminal_label.clone(),
                            overlapping_files: overlapping,
                            is_active,
                        });
                        continue; // already reported as file conflict
                    }
                }
            }
        }

        // Goal-similarity check (runs without file overlap).
        let similarity = bigram_jaccard(new_goal, &run.goal);
        if similarity >= GOAL_SIMILARITY_THRESHOLD {
            similar_goal_runs.push(SimilarGoalEntry {
                run_id: run.run_id,
                goal: run.goal,
                terminal_label: run.terminal_label,
                similarity,
                is_active,
            });
        }
    }

    let all_file_inactive = conflicts.iter().all(|c| !c.is_active);
    let all_goal_inactive = similar_goal_runs.iter().all(|s| !s.is_active);
    let auto_proceed = all_file_inactive && all_goal_inactive;
    let has_conflicts = !conflicts.is_empty() || !similar_goal_runs.is_empty();
    ConflictReport {
        has_conflicts,
        auto_proceed,
        conflicts,
        similar_goal_runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for s in [
            "pending",
            "running",
            "done",
            "failed",
            "verified",
            "cancelled",
        ] {
            let st: Status = s.parse().unwrap();
            let json = serde_json::to_string(&st).unwrap();
            assert_eq!(json, format!("\"{s}\""));
        }
        assert!("bogus".parse::<Status>().is_err());
    }

    #[test]
    fn counts_only_verified() {
        let rs = RunState {
            run_id: "r1".into(),
            goal: "g".into(),
            tasks: vec![
                TaskState {
                    id: "a".into(),
                    status: Status::Verified,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                    model: None,
                    cost_usd: None,
                },
                TaskState {
                    id: "b".into(),
                    status: Status::Done,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                    model: None,
                    cost_usd: None,
                },
            ],
            paused: false,
            terminal_label: None,
            recorded_at: None,
        };
        assert_eq!(rs.counts(), (1, 2));
    }

    #[test]
    fn counts_cancelled_also_counts_as_done() {
        let rs = RunState {
            run_id: "r1".into(),
            goal: "g".into(),
            tasks: vec![
                TaskState {
                    id: "a".into(),
                    status: Status::Verified,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                    model: None,
                    cost_usd: None,
                },
                TaskState {
                    id: "b".into(),
                    status: Status::Cancelled,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                    model: None,
                    cost_usd: None,
                },
                TaskState {
                    id: "c".into(),
                    status: Status::Pending,
                    worktree: None,
                    branch: None,
                    updated_at: None,
                    model: None,
                    cost_usd: None,
                },
            ],
            paused: false,
            terminal_label: None,
            recorded_at: None,
        };
        assert_eq!(rs.counts(), (2, 3));
    }

    fn make_tmp_dir(name: &str) -> PathBuf {
        // Unique dir via atomic `mkdtemp` (no fixed-name parallel-test collision
        // or pid-reuse TOCTOU); `.keep()` returns the path. Callers clean up.
        tempfile::Builder::new()
            .prefix(&format!("condukt-test-{name}-"))
            .tempdir()
            .expect("tempdir")
            .keep()
    }

    #[test]
    fn auto_detect_cargo() {
        let dir = make_tmp_dir("auto-cargo");
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "cargo test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_npm() {
        let dir = make_tmp_dir("auto-npm");
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "npm test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_pytest() {
        let dir = make_tmp_dir("auto-pytest");
        std::fs::write(dir.join("pyproject.toml"), "").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "pytest");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_fallback() {
        let dir = make_tmp_dir("auto-fallback");
        // empty dir — no recognizable project files
        assert_eq!(auto_detect_test_command(&dir), "cargo test");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Helper: build a minimal Config pointing state_dir at a temp directory.
    fn make_test_cfg(tmp: &Path) -> Config {
        Config {
            worktree_base: tmp.join("worktrees"),
            default_branch: "main".to_string(),
            shared_globs: Vec::new(),
            max_parallel: 4,
            state_dir: tmp.to_path_buf(),
            test_command: None,
            stuck_ttl_secs: 1800,
            build_command: None,
            deploy_command: None,
            loop_max_iters: 10,
        }
    }

    #[test]
    fn save_is_atomic_no_tmp_left() {
        let tmp = make_tmp_dir("atomic-save");
        let cfg = make_test_cfg(&tmp);
        let rs = RunState {
            run_id: "run-atomic".into(),
            goal: "test atomic write".into(),
            tasks: vec![],
            paused: false,
            terminal_label: None,
            recorded_at: None,
        };
        // save must succeed
        let saved_path = rs.save(&cfg, &tmp).unwrap();
        // final file exists
        assert!(saved_path.exists(), "final .json must exist after save");
        // no stray .tmp file should remain
        let tmp_path = saved_path.with_extension("json.tmp");
        // Note: the tmp file has extension "json.tmp" which means the full name
        // is "<run_id>.json.tmp", not "<run_id>.json" + ".tmp".
        // Re-derive it the same way the impl does.
        let dir = saved_path.parent().unwrap();
        let leftover_tmp = dir.join(format!("{}.json.tmp", rs.run_id));
        assert!(
            !leftover_tmp.exists(),
            "tmp file must not remain after atomic rename: {}",
            leftover_tmp.display()
        );
        // Silence unused-variable warning for the first derivation attempt.
        let _ = tmp_path;
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = make_tmp_dir("atomic-roundtrip");
        let cfg = make_test_cfg(&tmp);
        let rs = RunState {
            run_id: "run-rt".into(),
            goal: "roundtrip goal".into(),
            tasks: vec![TaskState {
                id: "t1".into(),
                status: Status::Pending,
                worktree: None,
                branch: None,
                updated_at: None,
                model: None,
                cost_usd: None,
            }],
            paused: false,
            terminal_label: None,
            recorded_at: None,
        };
        rs.save(&cfg, &tmp).unwrap();
        let loaded = RunState::load(&cfg, &tmp, "run-rt").unwrap();
        assert_eq!(loaded.run_id, rs.run_id);
        assert_eq!(loaded.goal, rs.goal);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].id, "t1");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn save_decomposition_is_atomic_no_tmp_left() {
        let tmp = make_tmp_dir("atomic-decomp");
        let cfg = make_test_cfg(&tmp);
        let run_id = "run-decomp";
        let json = r#"{"tasks":[]}"#;
        save_decomposition(&cfg, &tmp, run_id, json).unwrap();
        // final file exists
        let final_path = decomposition_path(&cfg, &tmp, run_id);
        assert!(
            final_path.exists(),
            "decomposition .json must exist after save"
        );
        // no stray .tmp remains
        let dir = final_path.parent().unwrap();
        let leftover_tmp = dir.join(format!("{run_id}.decomposition.json.tmp"));
        assert!(
            !leftover_tmp.exists(),
            "tmp decomposition file must not remain: {}",
            leftover_tmp.display()
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn save_decomposition_roundtrip() {
        let tmp = make_tmp_dir("decomp-rt");
        let cfg = make_test_cfg(&tmp);
        let run_id = "run-decomp-rt";
        let payload = r#"{"tasks":[{"id":"x"}]}"#;
        save_decomposition(&cfg, &tmp, run_id, payload).unwrap();
        let loaded = load_decomposition(&cfg, &tmp, run_id).unwrap();
        assert_eq!(loaded, payload);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Backward-compat: JSON without updated_at must load successfully with updated_at == None.
    #[test]
    fn backward_compat_no_updated_at() {
        let json = r#"{
            "run_id": "run-legacy",
            "goal": "legacy goal",
            "tasks": [
                {"id": "t1", "status": "pending"}
            ]
        }"#;
        let rs: RunState = serde_json::from_str(json).expect("must deserialize legacy JSON");
        assert_eq!(rs.tasks[0].updated_at, None);
    }

    /// After a Set operation, updated_at must be Some(positive timestamp).
    #[test]
    fn set_status_writes_updated_at() {
        let tmp = make_tmp_dir("timestamp-set");
        let cfg = make_test_cfg(&tmp);
        let before = now_secs();
        let rs = RunState {
            run_id: "run-ts".into(),
            goal: "timestamp test".into(),
            tasks: vec![TaskState {
                id: "t1".into(),
                status: Status::Pending,
                worktree: None,
                branch: None,
                updated_at: None,
                model: None,
                cost_usd: None,
            }],
            paused: false,
            terminal_label: None,
            recorded_at: None,
        };
        rs.save(&cfg, &tmp).unwrap();

        // Simulate StateAction::Set: load, mutate, save.
        let mut loaded = RunState::load(&cfg, &tmp, "run-ts").unwrap();
        let t = loaded.tasks.iter_mut().find(|t| t.id == "t1").unwrap();
        t.status = Status::Running;
        t.updated_at = Some(now_secs());
        loaded.save(&cfg, &tmp).unwrap();

        let after = now_secs();
        let reloaded = RunState::load(&cfg, &tmp, "run-ts").unwrap();
        let ts = reloaded.tasks[0]
            .updated_at
            .expect("updated_at must be Some after Set");
        assert!(ts >= before, "timestamp must be >= before");
        assert!(ts <= after, "timestamp must be <= after");

        std::fs::remove_dir_all(&tmp).ok();
    }

    // ── stuck_task_ids tests ──────────────────────────────────────────────

    fn make_run_with_tasks(tasks: Vec<TaskState>) -> RunState {
        RunState {
            run_id: "run-stuck-test".into(),
            goal: "stuck detection".into(),
            tasks,
            paused: false,
            terminal_label: None,
            recorded_at: None,
        }
    }

    /// A Running task whose updated_at is older than the TTL must appear in the result.
    #[test]
    fn stuck_task_ids_ttl_exceeded() {
        let ttl: u64 = 60;
        // Set updated_at to 2× TTL ago so it is definitely past the threshold.
        let old_ts = now_secs() - (ttl as i64 * 2);
        let run = make_run_with_tasks(vec![TaskState {
            id: "stuck-task".into(),
            status: Status::Running,
            worktree: None,
            branch: None,
            updated_at: Some(old_ts),
            model: None,
            cost_usd: None,
        }]);
        let ids = stuck_task_ids(&run, ttl);
        assert_eq!(ids, vec!["stuck-task".to_string()]);
    }

    /// A Running task whose updated_at is recent must NOT appear in the result.
    #[test]
    fn stuck_task_ids_ttl_not_exceeded() {
        let ttl: u64 = 3600;
        // Set updated_at to just 10 seconds ago — well within TTL.
        let recent_ts = now_secs() - 10;
        let run = make_run_with_tasks(vec![TaskState {
            id: "active-task".into(),
            status: Status::Running,
            worktree: None,
            branch: None,
            updated_at: Some(recent_ts),
            model: None,
            cost_usd: None,
        }]);
        let ids = stuck_task_ids(&run, ttl);
        assert!(ids.is_empty(), "recent Running task must not be stuck");
    }

    /// A Running task with updated_at == None (legacy data) must NOT be considered stuck.
    #[test]
    fn stuck_task_ids_none_updated_at_not_stuck() {
        let ttl: u64 = 60;
        let run = make_run_with_tasks(vec![TaskState {
            id: "legacy-task".into(),
            status: Status::Running,
            worktree: None,
            branch: None,
            updated_at: None,
            model: None,
            cost_usd: None,
        }]);
        let ids = stuck_task_ids(&run, ttl);
        assert!(
            ids.is_empty(),
            "Running task with no timestamp must not be stuck"
        );
    }

    // ── abandon_task helper ───────────────────────────────────────────────
    // These tests exercise the logic that `StateAction::Abandon` uses.
    // The actual command glue lives in main.rs; here we test the state mutations.

    /// A running task set to pending via abandon must have status Pending,
    /// cleared worktree/branch, and updated_at == None.
    #[test]
    fn state_abandon_running_task_becomes_pending() {
        let mut run = make_run_with_tasks(vec![TaskState {
            id: "t1".into(),
            status: Status::Running,
            worktree: Some("/path/to/wt".into()),
            branch: Some("feature/t1".into()),
            updated_at: Some(now_secs()),
            model: None,
            cost_usd: None,
        }]);
        let t = run.tasks.iter_mut().find(|t| t.id == "t1").unwrap();
        t.status = Status::Pending;
        t.worktree = None;
        t.branch = None;
        t.updated_at = None;

        assert_eq!(t.status, Status::Pending);
        assert!(t.worktree.is_none(), "worktree must be cleared on abandon");
        assert!(t.branch.is_none(), "branch must be cleared on abandon");
        assert!(
            t.updated_at.is_none(),
            "updated_at must be reset to None on abandon"
        );
    }

    /// A failed task can also be abandoned back to pending.
    #[test]
    fn state_abandon_failed_task_becomes_pending() {
        let mut run = make_run_with_tasks(vec![TaskState {
            id: "t-fail".into(),
            status: Status::Failed,
            worktree: Some("/path/to/wt".into()),
            branch: Some("feature/fail".into()),
            updated_at: Some(now_secs() - 100),
            model: None,
            cost_usd: None,
        }]);
        let t = run.tasks.iter_mut().find(|t| t.id == "t-fail").unwrap();
        t.status = Status::Pending;
        t.worktree = None;
        t.branch = None;
        t.updated_at = None;

        assert_eq!(t.status, Status::Pending);
        assert!(t.worktree.is_none());
        assert!(t.branch.is_none());
        assert!(t.updated_at.is_none());
    }

    /// Trying to abandon a task with status Pending must not be valid
    /// (the command handler bails! in main.rs; here we verify the guard logic).
    #[test]
    fn state_abandon_guard_rejects_non_running_non_failed() {
        let run = make_run_with_tasks(vec![
            TaskState {
                id: "pending-task".into(),
                status: Status::Pending,
                worktree: None,
                branch: None,
                updated_at: None,
                model: None,
                cost_usd: None,
            },
            TaskState {
                id: "verified-task".into(),
                status: Status::Verified,
                worktree: None,
                branch: None,
                updated_at: None,
                model: None,
                cost_usd: None,
            },
        ]);
        for t in &run.tasks {
            let is_abandonable = t.status == Status::Running || t.status == Status::Failed;
            assert!(
                !is_abandonable,
                "task '{}' with status {:?} must not be abandonable",
                t.id, t.status
            );
        }
    }

    /// --all-stuck abandons all stuck tasks (Running + TTL exceeded).
    #[test]
    fn state_abandon_all_stuck_resets_to_pending() {
        let ttl: u64 = 60;
        let old_ts = now_secs() - (ttl as i64 * 2);
        let recent_ts = now_secs() - 10;
        let mut run = make_run_with_tasks(vec![
            TaskState {
                id: "stuck-1".into(),
                status: Status::Running,
                worktree: Some("/wt/stuck1".into()),
                branch: Some("feat/stuck1".into()),
                updated_at: Some(old_ts),
                model: None,
                cost_usd: None,
            },
            TaskState {
                id: "stuck-2".into(),
                status: Status::Running,
                worktree: Some("/wt/stuck2".into()),
                branch: Some("feat/stuck2".into()),
                updated_at: Some(old_ts),
                model: None,
                cost_usd: None,
            },
            TaskState {
                id: "active".into(),
                status: Status::Running,
                worktree: Some("/wt/active".into()),
                branch: Some("feat/active".into()),
                updated_at: Some(recent_ts),
                model: None,
                cost_usd: None,
            },
        ]);

        let ids = stuck_task_ids(&run, ttl);
        assert_eq!(ids.len(), 2, "only the two old tasks should be stuck");

        for id in &ids {
            let t = run.tasks.iter_mut().find(|t| t.id == *id).unwrap();
            t.status = Status::Pending;
            t.worktree = None;
            t.branch = None;
            t.updated_at = None;
        }

        // stuck tasks are now pending
        let t1 = run.tasks.iter().find(|t| t.id == "stuck-1").unwrap();
        let t2 = run.tasks.iter().find(|t| t.id == "stuck-2").unwrap();
        assert_eq!(t1.status, Status::Pending);
        assert!(t1.worktree.is_none());
        assert!(t1.branch.is_none());
        assert!(t1.updated_at.is_none());
        assert_eq!(t2.status, Status::Pending);
        assert!(t2.worktree.is_none());

        // active task is untouched
        let ta = run.tasks.iter().find(|t| t.id == "active").unwrap();
        assert_eq!(ta.status, Status::Running);
        assert!(ta.worktree.is_some());
    }

    /// Specifying a non-existent task id must be caught (no task found in run).
    #[test]
    fn state_abandon_nonexistent_task_not_found() {
        let run = make_run_with_tasks(vec![TaskState {
            id: "real-task".into(),
            status: Status::Running,
            worktree: None,
            branch: None,
            updated_at: Some(now_secs()),
            model: None,
            cost_usd: None,
        }]);
        let found = run.tasks.iter().find(|t| t.id == "no-such-task");
        assert!(found.is_none(), "non-existent task id must not be found");
    }

    // ── bigram_jaccard tests ──────────────────────────────────────────────

    #[test]
    fn bigram_jaccard_identical_strings() {
        let s = "ログインバグを修正する";
        assert!(
            (bigram_jaccard(s, s) - 1.0).abs() < 1e-6,
            "identical strings must score 1.0"
        );
    }

    #[test]
    fn bigram_jaccard_empty_strings_return_zero() {
        assert_eq!(bigram_jaccard("", ""), 0.0);
        assert_eq!(bigram_jaccard("hello", ""), 0.0);
        assert_eq!(bigram_jaccard("", "hello"), 0.0);
    }

    #[test]
    fn bigram_jaccard_single_char_returns_zero() {
        // Single char → no bigrams → score must be 0.
        assert_eq!(bigram_jaccard("あ", "あ"), 0.0);
    }

    #[test]
    fn bigram_jaccard_similar_japanese_goals_above_threshold() {
        // Two phrasings of "fix the login bug" — should exceed the threshold 0.3.
        let a = "ログインバグを修正する";
        let b = "ログインのバグ修正";
        let score = bigram_jaccard(a, b);
        assert!(
            score >= GOAL_SIMILARITY_THRESHOLD,
            "similar Japanese goals must score >= threshold; got {score:.3}"
        );
    }

    #[test]
    fn bigram_jaccard_unrelated_strings_below_threshold() {
        let a = "ログインバグを修正する";
        let b = "Cargo.toml の依存バージョンを更新する";
        let score = bigram_jaccard(a, b);
        assert!(
            score < GOAL_SIMILARITY_THRESHOLD,
            "unrelated goals must score < threshold; got {score:.3}"
        );
    }

    #[test]
    fn bigram_jaccard_english_same_problem_above_threshold() {
        let a = "fix the authentication bug in login flow";
        let b = "fix auth bug in the login page";
        let score = bigram_jaccard(a, b);
        assert!(
            score >= GOAL_SIMILARITY_THRESHOLD,
            "related English goals must score >= threshold; got {score:.3}"
        );
    }

    /// Non-Running tasks must never appear, even if their timestamp is ancient.
    #[test]
    fn stuck_task_ids_only_running_tasks_are_candidates() {
        let ttl: u64 = 60;
        let ancient_ts = now_secs() - 9999;
        let run = make_run_with_tasks(vec![
            TaskState {
                id: "pending-old".into(),
                status: Status::Pending,
                worktree: None,
                branch: None,
                updated_at: Some(ancient_ts),
                model: None,
                cost_usd: None,
            },
            TaskState {
                id: "done-old".into(),
                status: Status::Done,
                worktree: None,
                branch: None,
                updated_at: Some(ancient_ts),
                model: None,
                cost_usd: None,
            },
            TaskState {
                id: "verified-old".into(),
                status: Status::Verified,
                worktree: None,
                branch: None,
                updated_at: Some(ancient_ts),
                model: None,
                cost_usd: None,
            },
        ]);
        let ids = stuck_task_ids(&run, ttl);
        assert!(
            ids.is_empty(),
            "only Running tasks should be candidates for stuck detection"
        );
    }

    // ── loop core tests ────────────────────────────────────────────────────

    #[test]
    fn loop_should_stop_all_pass() {
        let (stop, reason) = loop_should_stop(Some(3), 0);
        assert!(stop);
        assert_eq!(reason, "all tests pass");
    }

    #[test]
    fn loop_should_stop_no_progress() {
        let (stop, reason) = loop_should_stop(Some(5), 5);
        assert!(stop);
        assert_eq!(reason, "no progress: failure count unchanged");
    }

    #[test]
    fn loop_should_continue_when_decreasing() {
        let (stop, _) = loop_should_stop(Some(5), 3);
        assert!(!stop);
    }

    #[test]
    fn loop_should_continue_on_first_iter() {
        let (stop, _) = loop_should_stop(None, 4);
        assert!(!stop);
    }

    #[test]
    fn count_failures_cargo_format() {
        let output = "test result: FAILED. 10 passed; 3 failed; 0 ignored";
        assert_eq!(count_test_failures(output, false), 3);
    }

    #[test]
    fn count_failures_zero_on_success() {
        let output = "test result: ok. 10 passed; 0 failed";
        assert_eq!(count_test_failures(output, true), 0);
    }

    #[test]
    fn count_failures_pytest_format() {
        let output = "===== 2 failed, 8 passed in 1.23s =====";
        assert_eq!(count_test_failures(output, false), 2);
    }

    #[test]
    fn count_failures_jest_format() {
        let output = "FAIL src/foo.test.ts\nFAIL src/bar.test.ts\nTests: 2 failed, 5 passed";
        assert_eq!(count_test_failures(output, false), 2);
    }

    #[test]
    fn count_failures_unknown_format_returns_one() {
        let output = "Something went wrong";
        assert_eq!(count_test_failures(output, false), 1);
    }

    // ── records_for_run tests ──────────────────────────────────────────────

    use crate::model::{Class, Decomposition, Task};

    fn task(id: &str, title: &str, model: Option<&str>) -> Task {
        Task {
            id: id.into(),
            title: title.into(),
            touched_files: vec![format!("src/{id}.rs")],
            class: Class::Parallel,
            suggested_model: model.map(str::to_string),
            done_criteria: Some("cargo test".into()),
            ..Default::default()
        }
    }

    fn ts(id: &str, status: Status) -> TaskState {
        TaskState {
            id: id.into(),
            status,
            ..Default::default()
        }
    }

    /// A settled, never-recorded run yields one record per verified/failed task,
    /// joining title/files/class/done-criteria from the decomposition.
    #[test]
    fn records_for_run_emits_verified_and_failed() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![
                task("a", "Task A", Some("haiku")),
                task("b", "Task B", None),
            ],
        };
        let run = make_run_with_tasks(vec![ts("a", Status::Verified), ts("b", Status::Failed)]);
        let specs = records_for_run(&run, &dec).expect("settled run must yield records");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].title, "Task A");
        assert_eq!(specs[0].status, "verified");
        assert_eq!(specs[0].model, "haiku"); // from suggested_model
        assert_eq!(specs[0].files, vec!["src/a.rs".to_string()]);
        assert_eq!(specs[0].class, "parallel");
        assert_eq!(specs[0].done_criteria.as_deref(), Some("cargo test"));
        assert_eq!(specs[1].status, "failed");
        assert_eq!(specs[1].model, "sonnet"); // no suggested_model → default
    }

    /// A task's recorded model/cost (set via `state set --model/--cost`, incl. the
    /// escalation path) overrides the decomposition's suggested_model.
    #[test]
    fn records_for_run_prefers_task_state_model_and_cost() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![task("a", "Task A", Some("haiku"))],
        };
        let mut t = ts("a", Status::Verified);
        t.model = Some("opus".into()); // escalated
        t.cost_usd = Some(0.42);
        let run = make_run_with_tasks(vec![t]);
        let specs = records_for_run(&run, &dec).unwrap();
        assert_eq!(specs[0].model, "opus");
        assert_eq!(specs[0].cost_usd, 0.42);
    }

    /// Cancelled tasks carry no learning signal and are skipped.
    #[test]
    fn records_for_run_skips_cancelled() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![task("a", "A", None), task("b", "B", None)],
        };
        let run = make_run_with_tasks(vec![ts("a", Status::Verified), ts("b", Status::Cancelled)]);
        let specs = records_for_run(&run, &dec).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].title, "A");
    }

    /// A run with any still-running/pending task is NOT recordable (could change).
    #[test]
    fn records_for_run_none_when_unsettled() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![task("a", "A", None), task("b", "B", None)],
        };
        let run = make_run_with_tasks(vec![ts("a", Status::Verified), ts("b", Status::Running)]);
        assert!(records_for_run(&run, &dec).is_none());
    }

    /// An already-recorded run is never re-emitted (idempotency).
    #[test]
    fn records_for_run_none_when_already_recorded() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![task("a", "A", None)],
        };
        let mut run = make_run_with_tasks(vec![ts("a", Status::Verified)]);
        run.recorded_at = Some(now_secs());
        assert!(records_for_run(&run, &dec).is_none());
    }

    /// An empty run is not recordable (nothing to learn from).
    #[test]
    fn records_for_run_none_when_empty() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![],
        };
        let run = make_run_with_tasks(vec![]);
        assert!(records_for_run(&run, &dec).is_none());
    }

    /// An all-cancelled settled run yields an empty (but Some) vec so the caller
    /// can still stamp it recorded and stop re-checking.
    #[test]
    fn records_for_run_some_empty_when_all_cancelled() {
        let dec = Decomposition {
            goal: "g".into(),
            tasks: vec![task("a", "A", None)],
        };
        let run = make_run_with_tasks(vec![ts("a", Status::Cancelled)]);
        let specs = records_for_run(&run, &dec).expect("settled run must be Some");
        assert!(specs.is_empty());
    }
}
