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
pub fn acquire_at(
    session_id: &str,
    pid: u32,
    project: &str,
    lock_dir: Option<&Path>,
) -> Result<()> {
    let path = match lock_dir {
        Some(d) => lock_path_for(d),
        None => lock_path(),
    };

    // Check for existing active lock.
    if let Some(info) = read_lock(&path) {
        if pid_alive(info.pid) {
            anyhow::bail!(
                "lock already held by session {} (pid {}, project {})",
                info.session_id,
                info.pid,
                info.project
            );
        }
        // Stale lock — remove it silently.
        let _ = std::fs::remove_file(&path);
    }

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
    let json = serde_json::to_string_pretty(&info)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write lock file {}", path.display()))?;

    Ok(())
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
        assert!(!pid_alive(stale_pid), "assumption: pid {stale_pid} should not be alive");

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
}
