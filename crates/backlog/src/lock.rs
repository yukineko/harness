use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use harness_core::config::base_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub session_id: String,
    pub pid: u32,
    pub project: String,
    pub acquired_at: i64,
}

#[derive(Debug)]
pub enum LockStatus {
    /// Lock is held by an active process.
    Active(LockInfo),
    /// Lock file exists but the process is gone.
    Stale(LockInfo),
    /// No lock file.
    None,
}

fn lock_path() -> PathBuf {
    base_dir("backlog").join("run.lock")
}

fn lock_path_for(base: &Path) -> PathBuf {
    base.join("run.lock")
}

fn read_lock(path: &Path) -> Option<LockInfo> {
    let txt = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&txt).ok()
}

fn pid_alive(pid: u32) -> bool {
    // Fast path on Linux: /proc/<pid> exists iff the process is alive.
    #[cfg(target_os = "linux")]
    {
        if Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
    }
    // Portable fallback (macOS and any platform without /procfs): `kill -0 <pid>`
    // exits 0 when the process exists and is signalable, non-zero (ESRCH) otherwise.
    // Without this, /proc-only checks treat every live lock as stale off Linux.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Acquire the lock. Returns an error if the lock is currently active.
/// `lock_dir` allows tests to override the directory; pass `None` to use the
/// default `~/.backlog/` location.
///
/// Acquisition is atomic: the lock file is created with `create_new` (O_EXCL),
/// so two processes racing to create the same lock cannot both succeed — exactly
/// one wins the create, the loser sees `AlreadyExists`. There is no longer a
/// check-then-write window (the previous TOCTOU bug): the create itself is the
/// check. Stale locks (whose owning pid is gone) are reaped and the create is
/// retried a bounded number of times, so a dead holder never blocks acquisition.
pub fn acquire_at(
    session_id: &str,
    pid: u32,
    project: &str,
    lock_dir: Option<&Path>,
) -> Result<()> {
    acquire_inner(session_id, pid, project, lock_dir, false)
}

/// Force-acquire the lock, displacing even a *live* holder. This is the
/// documented `--force` ("強制奪取") escape hatch: a human has decided the
/// current holder — e.g. an abandoned session whose process is still alive —
/// should be taken over. Unlike [`acquire_at`], which only reaps locks whose
/// owner pid is gone, this reaps the existing lock regardless of liveness.
/// The publish step is still atomic, and the steal happens *inside* the bounded
/// retry loop, so a competitor that re-grabs the lock in the race window is
/// itself displaced (up to the attempt cap).
pub fn acquire_forced_at(
    session_id: &str,
    pid: u32,
    project: &str,
    lock_dir: Option<&Path>,
) -> Result<()> {
    acquire_inner(session_id, pid, project, lock_dir, true)
}

/// Force-acquire using the default lock path. See [`acquire_forced_at`].
pub fn acquire_forced(session_id: &str, pid: u32, project: &str) -> Result<()> {
    acquire_forced_at(session_id, pid, project, None)
}

/// Shared acquire implementation. `force = true` steals a live holder's lock
/// (the `--force` path); `force = false` only reaps confirmed-stale locks.
fn acquire_inner(
    session_id: &str,
    pid: u32,
    project: &str,
    lock_dir: Option<&Path>,
    force: bool,
) -> Result<()> {
    let path = match lock_dir {
        Some(d) => lock_path_for(d),
        None => lock_path(),
    };

    // Ensure directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create lock dir {}", parent.display()))?;
    }

    let info = LockInfo {
        session_id: session_id.to_string(),
        pid,
        project: project.to_string(),
        acquired_at: now_unix(),
    };
    // Serialize the lock contents into a temp file *before* publishing it, then
    // expose it atomically. `create_new` on the temp file gives us a private,
    // exclusively-owned path (the pid+nanos suffix makes collisions effectively
    // impossible), and a hard link from temp -> final path is the atomic
    // publish: link(2) fails with EEXIST if the final path already exists, so
    // exactly one racer can publish. Critically the file is *fully written*
    // before it is ever visible at the final path, so a concurrent reader can
    // never observe an empty/partial lock and misjudge it as stale.
    let json = serde_json::to_string_pretty(&info)?;
    let tmp_path = path.with_extension(format!("tmp.{}.{}", pid, now_unix_nanos()));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .with_context(|| format!("create temp lock file {}", tmp_path.display()))?;
        f.write_all(json.as_bytes())
            .with_context(|| format!("write temp lock file {}", tmp_path.display()))?;
        f.sync_all().ok();
    }
    // Ensure the temp file is cleaned up on every exit path.
    let _guard = TmpGuard(&tmp_path);

    // Bound the stale-reap/retry loop so a pathological race (another process
    // re-creating the lock right after we reap it) cannot spin forever.
    const MAX_ATTEMPTS: u32 = 8;
    for _ in 0..MAX_ATTEMPTS {
        // Atomic publish: link only succeeds if `path` does not yet exist.
        match std::fs::hard_link(&tmp_path, &path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Someone else published a lock. Inspect it. We only reap when
                // we can positively confirm the owner pid is dead; an
                // unreadable/partial read is treated as "still being written"
                // (active) so we never delete a live holder's lock.
                match read_lock(&path) {
                    Some(existing) if pid_alive(existing.pid) => {
                        if force {
                            // --force: displace even a live holder, then retry
                            // the atomic publish.
                            let _ = std::fs::remove_file(&path);
                            continue;
                        }
                        anyhow::bail!(
                            "lock already held by session {} (pid {}, project {})",
                            existing.session_id,
                            existing.pid,
                            existing.project
                        );
                    }
                    Some(_dead) => {
                        // Confirmed stale (readable, owner gone) — reap and retry.
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    None => {
                        // Unreadable: a writer is mid-publish (impossible with
                        // link, but possible if some external actor wrote a
                        // partial file). Briefly wait for it to settle, then
                        // re-judge on the next iteration without deleting it.
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                }
            }
            Err(e) => {
                return Err(e).with_context(|| format!("publish lock file {}", path.display()));
            }
        }
    }

    anyhow::bail!(
        "could not acquire lock at {} after {} attempts (contended/stale-thrashing)",
        path.display(),
        MAX_ATTEMPTS
    )
}

fn now_unix_nanos() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Removes a temp lock file when dropped, on every exit path.
struct TmpGuard<'a>(&'a Path);
impl Drop for TmpGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

/// Acquire using the default lock path.
pub fn acquire(session_id: &str, pid: u32, project: &str) -> Result<()> {
    acquire_at(session_id, pid, project, None)
}

/// Release the lock.  No-op if no lock file exists.
/// `lock_dir` allows tests to override the directory.
pub fn release_at(lock_dir: Option<&Path>) -> Result<()> {
    let path = match lock_dir {
        Some(d) => lock_path_for(d),
        None => lock_path(),
    };
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove lock file {}", path.display()))?;
    }
    Ok(())
}

/// Release using the default lock path.
pub fn release() -> Result<()> {
    release_at(None)
}

/// Return the current lock status.
/// `lock_dir` allows tests to override the directory.
pub fn status_at(lock_dir: Option<&Path>) -> LockStatus {
    let path = match lock_dir {
        Some(d) => lock_path_for(d),
        None => lock_path(),
    };
    match read_lock(&path) {
        None => LockStatus::None,
        Some(info) => {
            if pid_alive(info.pid) {
                LockStatus::Active(info)
            } else {
                LockStatus::Stale(info)
            }
        }
    }
}

/// Return the lock status using the default lock path.
pub fn status() -> LockStatus {
    status_at(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn pid_alive_is_cross_platform() {
        // Regression guard: pid_alive must work off Linux too. A /proc-only check
        // reports every live process as dead on macOS, which made fresh locks read
        // as stale. The current process is definitely alive; a huge pid is not.
        assert!(
            pid_alive(std::process::id()),
            "current process should be reported alive"
        );
        assert!(
            !pid_alive(99_999_999),
            "an unused high pid should be reported not alive"
        );
    }

    #[test]
    fn acquire_status_release_cycle() {
        let dir = tmp();
        let d = dir.path();
        let pid = std::process::id(); // current process — definitely alive

        // Initially no lock.
        assert!(matches!(status_at(Some(d)), LockStatus::None));

        // Acquire.
        acquire_at("sess-1", pid, "my-project", Some(d)).expect("acquire");

        // Status should be Active.
        match status_at(Some(d)) {
            LockStatus::Active(info) => {
                assert_eq!(info.session_id, "sess-1");
                assert_eq!(info.pid, pid);
                assert_eq!(info.project, "my-project");
            }
            other => panic!("expected Active, got {other:?}"),
        }

        // Release.
        release_at(Some(d)).expect("release");

        // Status should be None again.
        assert!(matches!(status_at(Some(d)), LockStatus::None));
    }

    #[test]
    fn stale_detection() {
        let dir = tmp();
        let d = dir.path();

        // Write a lockfile with a pid that should not exist.
        let stale_pid: u32 = 99_999_999;
        // Confirm /proc/<pid> really doesn't exist (it won't on Linux for that pid).
        assert!(
            !pid_alive(stale_pid),
            "assumption: pid {stale_pid} should not be alive"
        );

        let info = LockInfo {
            session_id: "stale-sess".to_string(),
            pid: stale_pid,
            project: "some-project".to_string(),
            acquired_at: 0,
        };
        std::fs::write(lock_path_for(d), serde_json::to_string(&info).unwrap()).unwrap();

        match status_at(Some(d)) {
            LockStatus::Stale(i) => {
                assert_eq!(i.pid, stale_pid);
            }
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[test]
    fn acquire_fails_when_active_lock_exists() {
        let dir = tmp();
        let d = dir.path();
        let pid = std::process::id();

        acquire_at("sess-a", pid, "proj-a", Some(d)).expect("first acquire");

        // Second acquire should fail because pid is alive.
        let err = acquire_at("sess-b", pid, "proj-b", Some(d));
        assert!(err.is_err(), "expected error acquiring locked resource");
    }

    #[test]
    fn acquire_overwrites_stale_lock() {
        let dir = tmp();
        let d = dir.path();
        let stale_pid: u32 = 99_999_999;

        let info = LockInfo {
            session_id: "old".to_string(),
            pid: stale_pid,
            project: "old-proj".to_string(),
            acquired_at: 0,
        };
        std::fs::write(lock_path_for(d), serde_json::to_string(&info).unwrap()).unwrap();

        let pid = std::process::id();
        acquire_at("new-sess", pid, "new-proj", Some(d)).expect("should succeed over stale lock");

        match status_at(Some(d)) {
            LockStatus::Active(i) => assert_eq!(i.session_id, "new-sess"),
            other => panic!("expected Active, got {other:?}"),
        }
    }

    // (a) Two consecutive acquires without a release: the second must fail.
    // With the atomic create_new path, the second acquire sees an existing lock
    // file owned by a live pid and bails — the lock cannot be double-held.
    #[test]
    fn second_acquire_without_release_fails() {
        let dir = tmp();
        let d = dir.path();
        let pid = std::process::id();

        acquire_at("first", pid, "proj", Some(d)).expect("first acquire");
        let second = acquire_at("second", pid, "proj", Some(d));
        assert!(
            second.is_err(),
            "second acquire without release must fail while lock is active"
        );

        // The original owner must still hold the lock unchanged.
        match status_at(Some(d)) {
            LockStatus::Active(i) => assert_eq!(i.session_id, "first"),
            other => panic!("expected Active held by 'first', got {other:?}"),
        }
    }

    // (b) A stale lock (dead pid) must be reaped so a fresh acquire wins.
    #[test]
    fn acquire_steals_stale_lock() {
        let dir = tmp();
        let d = dir.path();
        let stale_pid: u32 = 99_999_999;
        assert!(!pid_alive(stale_pid), "assumption: stale pid is not alive");

        let info = LockInfo {
            session_id: "dead".to_string(),
            pid: stale_pid,
            project: "dead-proj".to_string(),
            acquired_at: 0,
        };
        std::fs::write(lock_path_for(d), serde_json::to_string(&info).unwrap()).unwrap();

        let pid = std::process::id();
        acquire_at("live", pid, "live-proj", Some(d)).expect("acquire must steal a stale lock");

        match status_at(Some(d)) {
            LockStatus::Active(i) => {
                assert_eq!(i.session_id, "live");
                assert_eq!(i.pid, pid);
            }
            other => panic!("expected Active held by 'live', got {other:?}"),
        }
    }

    // --force steals a lock held by a *live* process, where a plain acquire
    // would (correctly) fail. This is the documented 強制奪取 escape hatch.
    #[test]
    fn force_acquire_steals_a_live_lock() {
        let dir = tmp();
        let d = dir.path();
        let live_pid = std::process::id(); // alive holder

        acquire_at("incumbent", live_pid, "their-proj", Some(d)).expect("incumbent acquires");

        // Plain acquire must refuse a live holder.
        assert!(
            acquire_at("usurper", live_pid, "our-proj", Some(d)).is_err(),
            "plain acquire must not steal a live lock"
        );

        // Forced acquire takes it over.
        acquire_forced_at("usurper", live_pid, "our-proj", Some(d))
            .expect("--force must steal a live lock");

        match status_at(Some(d)) {
            LockStatus::Active(i) => {
                assert_eq!(i.session_id, "usurper");
                assert_eq!(i.project, "our-proj");
            }
            other => panic!("expected the usurper's lock active, got {other:?}"),
        }
    }

    // --force on an *unheld* lock behaves like a normal acquire (no existing
    // file to displace), so the escape hatch is always safe to pass.
    #[test]
    fn force_acquire_on_free_lock_just_acquires() {
        let dir = tmp();
        let d = dir.path();
        let pid = std::process::id();
        assert!(matches!(status_at(Some(d)), LockStatus::None));
        acquire_forced_at("solo", pid, "proj", Some(d)).expect("force on free lock acquires");
        match status_at(Some(d)) {
            LockStatus::Active(i) => assert_eq!(i.session_id, "solo"),
            other => panic!("expected Active, got {other:?}"),
        }
    }

    // (c) Concurrency stand-in: a lock file already exists on disk at the moment
    // acquire runs (as if a competitor created it just before us). If the owner
    // is active, acquire must fail; if the owner is stale, acquire must succeed.
    #[test]
    fn acquire_against_preexisting_lock_file() {
        // Active owner present -> must fail.
        {
            let dir = tmp();
            let d = dir.path();
            let live_pid = std::process::id();
            let existing = LockInfo {
                session_id: "competitor".to_string(),
                pid: live_pid,
                project: "comp-proj".to_string(),
                acquired_at: now_unix(),
            };
            std::fs::write(lock_path_for(d), serde_json::to_string(&existing).unwrap()).unwrap();

            let res = acquire_at("us", live_pid, "our-proj", Some(d));
            assert!(
                res.is_err(),
                "acquire must fail when an active lock file already exists"
            );
            match status_at(Some(d)) {
                LockStatus::Active(i) => assert_eq!(i.session_id, "competitor"),
                other => panic!("expected the competitor's lock intact, got {other:?}"),
            }
        }

        // Stale owner present -> must succeed and take over.
        {
            let dir = tmp();
            let d = dir.path();
            let stale_pid: u32 = 99_999_999;
            assert!(!pid_alive(stale_pid), "assumption: stale pid is not alive");
            let existing = LockInfo {
                session_id: "ghost".to_string(),
                pid: stale_pid,
                project: "ghost-proj".to_string(),
                acquired_at: 0,
            };
            std::fs::write(lock_path_for(d), serde_json::to_string(&existing).unwrap()).unwrap();

            let our_pid = std::process::id();
            acquire_at("us", our_pid, "our-proj", Some(d))
                .expect("acquire must succeed over a stale pre-existing lock file");
            match status_at(Some(d)) {
                LockStatus::Active(i) => assert_eq!(i.session_id, "us"),
                other => panic!("expected our lock active, got {other:?}"),
            }
        }
    }

    // Direct TOCTOU regression: many threads race to acquire the same empty
    // lock. With the old check-then-write code, multiple threads observed "no
    // lock" and all wrote, so >1 acquire succeeded (double acquisition). With
    // the atomic create_new path exactly one wins. All pids are the live
    // current process, so a winner is never reaped as stale.
    #[test]
    fn concurrent_acquire_admits_exactly_one() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let dir = tmp();
        let d = Arc::new(dir.path().to_path_buf());
        let pid = std::process::id();

        const N: usize = 16;
        let barrier = Arc::new(Barrier::new(N));
        let winners = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(N);
        for i in 0..N {
            let d = Arc::clone(&d);
            let barrier = Arc::clone(&barrier);
            let winners = Arc::clone(&winners);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                if acquire_at(&format!("sess-{i}"), pid, "proj", Some(d.as_path())).is_ok() {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().expect("thread join");
        }

        assert_eq!(
            winners.load(Ordering::SeqCst),
            1,
            "exactly one concurrent acquire must succeed (no double acquisition)"
        );
    }
}
