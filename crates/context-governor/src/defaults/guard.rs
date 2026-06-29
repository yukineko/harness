//! Default [`CompactionGuard`] — PreCompact backstop. Snapshots the transcript +
//! records verbatim spans, then **proceeds** (compression is delegated to
//! built-in compaction; no self-summarization). Block is reserved for the rare
//! case where the snapshot itself could not be secured.

use crate::handlers::{BackingStore, CompactDecision, CompactionGuard};
use crate::ledger::{Action, Ledger, LedgerNode};
use crate::types::{ItemBody, ItemId};
use harness_core::hook::HookInput;

pub struct DefaultGuard;

impl CompactionGuard for DefaultGuard {
    fn on_pre_compact(&mut self, i: &HookInput, s: &mut dyn BackingStore) -> CompactDecision {
        // Secure a transcript snapshot before built-in compaction runs (I1).
        // snapshot_transcript is fail-soft (empty/missing transcript → no-op), so
        // the backstop proceeds unconditionally; Block stays reserved for a future
        // case where the snapshot genuinely could not be secured.
        let key = s.snapshot_transcript(&i.transcript_path);
        // Emit a ledger row ONLY when a real (non-empty) snapshot was secured.
        // SNAPSHOT_KEY is returned even for empty/missing transcripts, so we check
        // recall to detect whether anything was actually stored.
        if let Some(item) = s.recall(&key) {
            if let ItemBody::Inline(text) = &item.body {
                if !text.is_empty() {
                    let resident = (text.chars().count().div_ceil(4).max(1)) as u32;
                    let node = LedgerNode {
                        session: i.session_id.clone(),
                        hook: i.hook_event_name.clone(),
                        item: Some(ItemId(key.0)),
                        action: Action::Snapshotted { to: key },
                        reason: "precompact-snapshot",
                    };
                    Ledger::open(&i.cwd).append(&node, resident);
                }
            }
        }
        CompactDecision::Proceed
    }
}

// ── Test helpers (test builds only) ──────────────────────────────────────────

/// Process-wide mutex used by the env-adjacent tests in `guard` and
/// `checkpointer` to prevent them from running concurrently with each other.
/// Exposed at module level (not inside `mod tests`) so the checkpointer module's
/// tests can call `crate::defaults::guard::acquire_env_lock`.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn acquire_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) // recover from prior panic poison
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backing::SNAPSHOT_KEY;
    use crate::handlers::BackingStore;
    use crate::types::{ContextItem, ItemBody, ItemId, Lane, StoreKey};
    use harness_core::hook::HookInput;
    use harness_core::store::{project_key, safe_session};
    use std::path::{Path, PathBuf};

    /// In-memory [`BackingStore`] for tests: avoids all env var / filesystem
    /// dependencies for the store itself, so the store never uses a directory
    /// owned by another test (e.g. `backing::open_is_ok_and_creates_state_dir`).
    /// The recall response is pre-configured at construction time.
    struct MockStore {
        /// The `Inline` body text returned for `SNAPSHOT_KEY`, or `None` to
        /// simulate an empty/missing transcript (recall → `None`).
        snapshot_text: Option<String>,
    }

    impl MockStore {
        fn with_snapshot(text: &str) -> Self {
            Self {
                snapshot_text: Some(text.to_string()),
            }
        }
        fn without_snapshot() -> Self {
            Self {
                snapshot_text: None,
            }
        }
    }

    impl BackingStore for MockStore {
        fn snapshot_transcript(&mut self, _transcript_path: &str) -> StoreKey {
            SNAPSHOT_KEY // always return the fixed key; snapshot is in `snapshot_text`
        }
        fn put(&mut self, item: &ContextItem) -> StoreKey {
            StoreKey(item.id.0)
        }
        fn recall(&self, key: &StoreKey) -> Option<ContextItem> {
            if *key != SNAPSHOT_KEY {
                return None;
            }
            self.snapshot_text.as_ref().map(|text| ContextItem {
                id: ItemId(SNAPSHOT_KEY.0),
                lane: Lane::Verbatim,
                tokens: (text.chars().count().div_ceil(4)).max(1) as u32,
                body: ItemBody::Inline(text.clone()),
            })
        }
    }

    /// Replicate `ledger::resolve_state` from the CURRENT env snapshot so the
    /// path computed here and the path that `Ledger::open` computes inside the
    /// handler are always identical (both happen in the same nanosecond window,
    /// before any other thread can mutate the env vars).
    fn current_ledger_path(cwd: &str) -> PathBuf {
        let base = std::env::var("CONTEXT_GOVERNOR_STATE_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| ".".to_string());
                PathBuf::from(home).join(".context-governor")
            });
        let session = std::env::var("CLAUDE_CODE_SESSION_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| safe_session(&s))
            .unwrap_or_else(|| safe_session("default"));
        base.join(project_key(Path::new(cwd)))
            .join(session)
            .join("ledger.jsonl")
    }

    fn count_snapshotted(sink: &Path) -> usize {
        std::fs::read_to_string(sink)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"snapshotted\""))
            .count()
    }

    /// Single test covering "real snapshot → one row" and
    /// "empty/missing → no row, no panic".
    ///
    /// Env-safety: this test NEVER sets `CONTEXT_GOVERNOR_STATE_DIR`, so it
    /// cannot override the value that
    /// `backing::open_is_ok_and_creates_state_dir_for_any_cwd` sets, keeping
    /// the two tests safe to run in parallel.
    ///
    /// Store isolation: a `MockStore` holds the snapshot entirely in memory, so
    /// no snapshot file is written to any env-var-derived directory.  Only the
    /// `Ledger::open` call (inside the handler) and `current_ledger_path` (called
    /// immediately before) read the env — and they execute nanoseconds apart, so
    /// they always agree on the path.  `project_key(unique_cwd)` isolates each
    /// scenario's ledger file from every other run and scenario.
    #[test]
    fn guard_emits_snapshotted_row_iff_real_snapshot() {
        let _lock = acquire_env_lock();

        // Use unique cwd values so project_key(cwd) gives a unique ledger path
        // per scenario and per process invocation.  A tempdir path suffix
        // guarantees uniqueness without touching STATE_DIR.
        let td = tempfile::tempdir().expect("tempdir");

        // ── Scenario A: real (non-empty) snapshot → exactly one row ──────────
        {
            let cwd = td.path().join("guard-real").to_str().unwrap().to_string();
            let mut store = MockStore::with_snapshot("hello world from user\n\nhello back from assistant");

            let input = HookInput {
                transcript_path: "/unused-by-mock".to_string(),
                cwd: cwd.clone(),
                hook_event_name: "PreCompact".to_string(),
                ..Default::default()
            };

            let sink = current_ledger_path(&cwd);
            let mut guard = DefaultGuard;
            let decision = guard.on_pre_compact(&input, &mut store);

            assert!(
                matches!(decision, CompactDecision::Proceed),
                "guard must proceed after securing a snapshot"
            );
            assert_eq!(
                count_snapshotted(&sink),
                1,
                "real snapshot must produce exactly one snapshotted row; sink={sink:?}"
            );
        }

        // ── Scenario B: no snapshot → no row, no panic ────────────────────────
        {
            let cwd = td
                .path()
                .join("guard-missing")
                .to_str()
                .unwrap()
                .to_string();
            let mut store = MockStore::without_snapshot();

            let input = HookInput {
                transcript_path: "/no/such/transcript.jsonl".to_string(),
                cwd: cwd.clone(),
                hook_event_name: "PreCompact".to_string(),
                ..Default::default()
            };

            let sink = current_ledger_path(&cwd);
            let mut guard = DefaultGuard;
            let decision = guard.on_pre_compact(&input, &mut store);

            assert!(
                matches!(decision, CompactDecision::Proceed),
                "guard must proceed even when transcript is missing"
            );
            assert_eq!(
                count_snapshotted(&sink),
                0,
                "missing/empty transcript must produce no snapshotted row"
            );
        }
    }
}
