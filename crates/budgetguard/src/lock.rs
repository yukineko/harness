//! A tiny cross-platform advisory lock for serializing the ledger
//! read-modify-write across concurrent sessions.
//!
//! The daily ledger is a single shared file updated on every Stop. Without
//! serialization, two sessions that Stop at the same moment each load → record →
//! save and the last writer clobbers the other's entry (lost update), silently
//! under-counting the day total and letting the daily block fail open.
//!
//! We use an `O_EXCL` lock file (`OpenOptions::create_new`) — an atomic
//! exclusive create that works the same on Unix and Windows without any external
//! crate. Acquisition spins with a short backoff up to a bounded timeout; a lock
//! left behind by a crashed process is stolen once it is older than
//! `STALE_AFTER`. Release removes the file (also on `Drop`, so a panic in the
//! critical section can't strand the lock).
//!
//! This is best-effort by design: if the lock can't be acquired within the
//! timeout we proceed anyway rather than ever blocking a turn — correctness under
//! contention is improved, and the hook never hangs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A held lock; releases (removes the lock file) on drop.
pub struct LedgerLock {
    path: PathBuf,
    /// False when we proceeded without truly holding the lock (timed out). Drop
    /// then must not remove a file it doesn't own.
    held: bool,
}

/// Steal a lock file older than this (its owner is assumed dead).
const STALE_AFTER: Duration = Duration::from_secs(30);
/// Give up trying to acquire after this and proceed best-effort.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);
/// Backoff between acquisition attempts.
const BACKOFF: Duration = Duration::from_millis(25);

impl LedgerLock {
    fn lock_path(state_dir: &Path) -> PathBuf {
        state_dir.join("ledger.lock")
    }

    /// Acquire the ledger lock for `state_dir`. Always returns a guard: if the
    /// lock can't be taken within the timeout the guard is "unheld" (we proceed
    /// best-effort), so callers never block a turn.
    pub fn acquire(state_dir: &Path) -> Self {
        let path = Self::lock_path(state_dir);
        let _ = std::fs::create_dir_all(state_dir);
        let start = Instant::now();

        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return LedgerLock { path, held: true },
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Owner looks dead AND we managed to remove its lock: retry
                    // create immediately. If the removal fails (e.g. a races/perm
                    // issue) we fall through to the timeout-bounded backoff below
                    // rather than hot-spinning forever on a file we can't delete.
                    if Self::is_stale(&path) && std::fs::remove_file(&path).is_ok() {
                        continue;
                    }
                    if start.elapsed() >= ACQUIRE_TIMEOUT {
                        // Best-effort: proceed without the lock rather than hang.
                        return LedgerLock { path, held: false };
                    }
                    std::thread::sleep(BACKOFF);
                }
                // Any other error (e.g. permissions): don't block the turn.
                Err(_) => return LedgerLock { path, held: false },
            }
        }
    }

    fn is_stale(path: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            // Vanished between attempts — treat as acquirable.
            return true;
        };
        match meta.modified() {
            Ok(mtime) => mtime
                .elapsed()
                .map(|age| age >= STALE_AFTER)
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        if self.held {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
