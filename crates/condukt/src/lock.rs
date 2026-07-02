//! Per-run file lock that serializes run-state read-modify-write cycles.
//!
//! condukt's run state lives at `<state_dir>/<project-key>/<run-id>.json` and is
//! updated with a load→mutate→save cycle (`pause_run`, `resume_run`,
//! `StateAction::Set`). Two concurrent sessions/worktrees doing this at once
//! race: both load the same snapshot, each mutates a different field, and the
//! second `save` clobbers the first (last-writer-wins TOCTOU). This module gives
//! each run a lock file next to its state so the whole load→mutate→save cycle is
//! mutually exclusive per run — unrelated runs never block each other.
//!
//! Reuses the proven atomicity from `backlog::lock`: the lock is published with a
//! hard link (link(2) fails `EEXIST` if the target already exists, so exactly one
//! racer wins the publish and a reader never observes a partial file), stale
//! locks whose owner pid is gone are reaped, and the reap/retry loop is bounded.
//! Unlike `backlog::lock` — which fails fast when a live holder exists — this lock
//! *waits* (bounded) for the holder to release so concurrent RMW cycles serialize
//! and both complete. It is fail-soft: if the lock cannot be acquired within the
//! deadline it degrades to proceeding unlocked (logged) rather than failing the
//! caller's state update, and it never panics.

use crate::config::Config;
use crate::store::{project_key, repo_root};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
struct LockInfo {
    pid: u32,
    run_id: String,
    acquired_at: i64,
}

/// RAII guard for a per-run state lock. Held across a load→mutate→save cycle and
/// released (best-effort) on drop. When `path` is `None` the lock was not held
/// (fail-soft degrade after a timeout) and drop is a no-op.
#[must_use = "the run lock is released as soon as this guard is dropped"]
pub struct RunLock {
    path: Option<PathBuf>,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_unix_nanos() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        if Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Lock file path for a run — sits beside the run's `<run-id>.json` state file,
/// keyed the same way (sanitised run id, per project) so unrelated runs and
/// unrelated projects never share a lock.
fn lock_path(cfg: &Config, cwd: &Path, run_id: &str) -> PathBuf {
    let dir = cfg.state_dir.join(project_key(&repo_root(cwd)));
    dir.join(format!(
        "{}.lock",
        harness_core::store::safe_session(run_id)
    ))
}

fn read_info(path: &Path) -> Option<LockInfo> {
    let txt = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&txt).ok()
}

impl RunLock {
    /// Default bounded wait before degrading to unlocked. Generous enough that a
    /// normal RMW cycle (a few file ops) always releases well within it.
    const DEADLINE: Duration = Duration::from_secs(10);

    /// Acquire the per-run lock, waiting (bounded) for any live holder to
    /// release. Reaps a stale lock whose owner pid is gone. Never fails: on
    /// timeout it logs and returns an unlocked guard so the caller's state
    /// update still proceeds (fail-soft).
    pub fn acquire(cfg: &Config, cwd: &Path, run_id: &str) -> Self {
        Self::acquire_with_deadline(cfg, cwd, run_id, Self::DEADLINE)
    }

    /// Like [`RunLock::acquire`] but with an explicit deadline (used by tests).
    pub fn acquire_with_deadline(
        cfg: &Config,
        cwd: &Path,
        run_id: &str,
        deadline: Duration,
    ) -> Self {
        let path = lock_path(cfg, cwd, run_id);

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "condukt: could not create lock dir {} ({e}); proceeding unlocked",
                    parent.display()
                );
                return RunLock { path: None };
            }
        }

        // Fully write our lock contents to a private temp file first, then
        // publish it atomically via hard link. A concurrent reader can never
        // observe a partial lock at the final path.
        let info = LockInfo {
            pid: std::process::id(),
            run_id: run_id.to_string(),
            acquired_at: now_unix(),
        };
        let json = match serde_json::to_string(&info) {
            Ok(j) => j,
            Err(_) => return RunLock { path: None },
        };
        let tmp_path = path.with_extension(format!(
            "lock.tmp.{}.{}",
            std::process::id(),
            now_unix_nanos()
        ));
        {
            use std::io::Write;
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
            {
                Ok(mut f) => {
                    if f.write_all(json.as_bytes()).is_err() {
                        let _ = std::fs::remove_file(&tmp_path);
                        eprintln!("condukt: could not write temp lock; proceeding unlocked");
                        return RunLock { path: None };
                    }
                    f.sync_all().ok();
                }
                Err(e) => {
                    eprintln!("condukt: could not create temp lock ({e}); proceeding unlocked");
                    return RunLock { path: None };
                }
            }
        }
        let _guard = TmpGuard(&tmp_path);

        let start = Instant::now();
        loop {
            match std::fs::hard_link(&tmp_path, &path) {
                Ok(()) => return RunLock { path: Some(path) },
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Someone holds the lock. Reap it only if we can positively
                    // confirm the owner pid is gone; otherwise wait for release.
                    match read_info(&path) {
                        Some(existing) if !pid_alive(existing.pid) => {
                            let _ = std::fs::remove_file(&path);
                            continue;
                        }
                        _ => {
                            if start.elapsed() >= deadline {
                                eprintln!(
                                    "condukt: run '{run_id}' state lock contended for {:?}; \
                                     proceeding unlocked (update may race)",
                                    deadline
                                );
                                return RunLock { path: None };
                            }
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "condukt: could not publish lock {} ({e}); proceeding unlocked",
                        path.display()
                    );
                    return RunLock { path: None };
                }
            }
        }
    }
}

/// Removes a temp lock file when dropped, on every exit path.
struct TmpGuard<'a>(&'a Path);
impl Drop for TmpGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}
