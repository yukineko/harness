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
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TmpHome(std::path::PathBuf);
    impl TmpHome {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("autoflow-lock-{}-{n}", std::process::id()));
            std::fs::create_dir_all(p.join(".backlog")).unwrap();
            std::env::set_var("HOME", &p);
            TmpHome(p)
        }
        fn lock_path(&self) -> std::path::PathBuf {
            self.0.join(".backlog").join("run.lock")
        }
    }
    impl Drop for TmpHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
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
            format!(r#"{{"pid":{},"session_id":"x","project":"/p","acquired_at":0}}"#, std::process::id()),
        )
        .unwrap();
        assert!(backlog_driver_active(), "live owner → active");

        // A lock owned by an impossible pid (dead) reads as inactive.
        std::fs::write(h.lock_path(), r#"{"pid":2147483646}"#).unwrap();
        assert!(!backlog_driver_active(), "dead owner → inactive (not wedged)");

        // Garbage parses to inactive.
        std::fs::write(h.lock_path(), b"not json").unwrap();
        assert!(!backlog_driver_active(), "unparseable → inactive");
        drop(h);
    }
}
