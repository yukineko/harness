//! Read-only view of backlog's run lock.
//!
//! autoflow's Stop hook auto-drives `/condukt` and `/backlog`. But `/flow` and
//! `/backlog` serialize their condukt runs with the backlog lock
//! (`~/.backlog/run.lock`), which autoflow never consulted — so if autoflow's
//! auto-loop fired while one of those drivers held the lock, the same queue would
//! be driven twice (double condukt execution). autoflow therefore stands down
//! whenever another *live* process holds the lock.
//!
//! This only READS the lock (autoflow never acquires it); the writer/owner is the
//! backlog binary.

use std::process::Stdio;

use harness_core::config::base_dir;
use serde::Deserialize;

#[derive(Deserialize)]
struct LockInfo {
    pid: u32,
    /// The session that acquired the lock. Absent in legacy locks → "".
    #[serde(default)]
    session_id: String,
}

/// True if another live process currently holds the backlog run lock. A stale
/// lock (owner pid gone) reads as inactive so autoflow is never wedged by it.
pub fn backlog_driver_active() -> bool {
    let path = base_dir("backlog").join("run.lock");
    let Ok(txt) = std::fs::read_to_string(&path) else {
        return false; // no lock file → no active driver
    };
    let Ok(info) = serde_json::from_str::<LockInfo>(&txt) else {
        return false; // unparseable → don't wedge autoflow on garbage
    };
    pid_alive(info.pid)
}

/// True if the backlog run lock is held by *this* session — i.e. a `/flow` (or
/// `/backlog`) driver is running the queue from within this very Claude session.
/// This is the mirror of [`backlog_driver_active`] (which asks "is *another*
/// process driving?"): here we ask "am *I* the driver?", by matching the lock's
/// `session_id` against the current session.
///
/// Used by the PreCompact hook to decide whether to drop a resume-flow marker —
/// we only want to auto-resume `/flow` after a `/compact` when the flow loop was
/// actually running in this session. An empty `session_id`, a missing/garbage
/// lock file, or a mismatched owner all read as `false` (never resume blindly).
pub fn this_session_holds_lock(session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = base_dir("backlog").join("run.lock");
    let Ok(txt) = std::fs::read_to_string(&path) else {
        return false; // no lock file → this session holds nothing
    };
    let Ok(info) = serde_json::from_str::<LockInfo>(&txt) else {
        return false; // unparseable → don't resume on garbage
    };
    !info.session_id.is_empty() && info.session_id == session_id
}

fn pid_alive(pid: u32) -> bool {
    // Fast path on Linux: /proc/<pid> exists iff the process is alive.
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
    }
    // Portable fallback: `kill -0 <pid>` exits 0 when the process is signalable.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    // These tests mutate the process-global HOME env var; cargo runs tests in a
    // binary concurrently, so they serialize behind the crate-wide
    // `test_home_guard` mutex (shared with main.rs's hook tests, which also read
    // `$HOME/.backlog/run.lock`) to avoid a flaky cross-test race.

    // `_dir` is a `tempfile::TempDir`: a unique, collision-free temp dir that
    // removes itself on drop (no pid-based path, no manual cleanup). `_guard`
    // is held only for its RAII Drop (releases the HOME mutex at the end of the
    // test); neither field is read, hence the underscores. Field order matters:
    // `_dir` drops (cleanup) before `_guard` releases the HOME mutex.
    struct TmpHome {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        _guard: MutexGuard<'static, ()>,
    }
    impl TmpHome {
        fn new() -> Self {
            let guard = crate::test_home_guard();
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().to_path_buf();
            std::fs::create_dir_all(path.join(".backlog")).unwrap();
            std::env::set_var("HOME", &path);
            TmpHome {
                _dir: dir,
                path,
                _guard: guard,
            }
        }
        fn lock_path(&self) -> std::path::PathBuf {
            self.path.join(".backlog").join("run.lock")
        }
    }

    #[test]
    fn absent_lock_is_inactive() {
        let h = TmpHome::new();
        assert!(!backlog_driver_active(), "no lock file → inactive");
        drop(h);
    }

    #[test]
    fn live_pid_lock_is_active_stale_is_not() {
        let h = TmpHome::new();
        // A lock owned by THIS process (definitely alive) is active.
        std::fs::write(
            h.lock_path(),
            format!(
                r#"{{"pid":{},"session_id":"x","project":"/p","acquired_at":0}}"#,
                std::process::id()
            ),
        )
        .unwrap();
        assert!(backlog_driver_active(), "live owner → active");

        // A lock owned by an impossible pid (dead) reads as inactive.
        std::fs::write(h.lock_path(), r#"{"pid":2147483646}"#).unwrap();
        assert!(
            !backlog_driver_active(),
            "dead owner → inactive (not wedged)"
        );

        // Garbage parses to inactive.
        std::fs::write(h.lock_path(), b"not json").unwrap();
        assert!(!backlog_driver_active(), "unparseable → inactive");
        drop(h);
    }

    #[test]
    fn this_session_holds_lock_matches_owner_only() {
        let h = TmpHome::new();
        // No lock file yet → this session holds nothing.
        assert!(!this_session_holds_lock("sess-a"), "no lock → false");

        // Lock owned by this very session id → true.
        std::fs::write(
            h.lock_path(),
            format!(
                r#"{{"pid":{},"session_id":"sess-a","project":"/p","acquired_at":0}}"#,
                std::process::id()
            ),
        )
        .unwrap();
        assert!(
            this_session_holds_lock("sess-a"),
            "own session → holds lock"
        );

        // Lock owned by a DIFFERENT session id → false.
        assert!(
            !this_session_holds_lock("sess-b"),
            "other session's lock → false"
        );

        // An empty session id never matches (even against an empty owner).
        assert!(!this_session_holds_lock(""), "empty session id → false");

        // A lock with no session_id field (legacy) matches nobody.
        std::fs::write(h.lock_path(), r#"{"pid":123}"#).unwrap();
        assert!(
            !this_session_holds_lock("sess-a"),
            "legacy lock (no session_id) → false"
        );

        // Garbage lock → false (never resume on garbage).
        std::fs::write(h.lock_path(), b"not json").unwrap();
        assert!(!this_session_holds_lock("sess-a"), "unparseable → false");
        drop(h);
    }
}
