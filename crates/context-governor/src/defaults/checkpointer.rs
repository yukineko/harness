//! Default [`Checkpointer`] — Stop / SubagentStop externalization. Writes
//! completed work to the backing store under a threshold gate. **Side effects
//! only**: the bin discards any result and exits 0, so this must never block
//! (the per-session block cap short-circuits the session after repeated blocks).

use crate::handlers::{BackingStore, Checkpointer};
use crate::ledger::{Action, Ledger, LedgerNode};
use crate::types::{ItemBody, ItemId};
use harness_core::hook::HookInput;
use harness_core::transcript::last_usage_tokens;

const DEFAULT_CHECKPOINT_THRESHOLD: u64 = 10_000;

pub struct DefaultCheckpointer;

impl Checkpointer for DefaultCheckpointer {
    fn checkpoint(&mut self, i: &HookInput, s: &mut dyn BackingStore) {
        // Re-entrant Stop (continuation of a prior Stop hook): do nothing.
        if i.stop_hook_active {
            return;
        }
        let threshold = std::env::var("CONTEXT_GOVERNOR_CHECKPOINT_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CHECKPOINT_THRESHOLD);
        // Only externalize once the conversation is heavy enough to be worth it.
        if last_usage_tokens(&i.transcript_path).unwrap_or(0) >= threshold {
            let key = s.snapshot_transcript(&i.transcript_path);
            // Emit a ledger row ONLY when a real (non-empty) snapshot was secured.
            // SNAPSHOT_KEY is returned even for empty/missing transcripts, so we
            // check recall to detect whether anything was actually stored.
            if let Some(item) = s.recall(&key) {
                if let ItemBody::Inline(text) = &item.body {
                    if !text.is_empty() {
                        let resident = (text.chars().count().div_ceil(4).max(1)) as u32;
                        let node = LedgerNode {
                            session: i.session_id.clone(),
                            hook: i.hook_event_name.clone(),
                            item: Some(ItemId(key.0)),
                            action: Action::Snapshotted { to: key },
                            reason: "stop-checkpoint",
                        };
                        Ledger::open(&i.cwd).append(&node, resident);
                    }
                }
            }
        }
    }
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

    /// In-memory [`BackingStore`] — mirrors the one in `guard::tests`.
    struct MockStore {
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
            SNAPSHOT_KEY
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

    /// Single test covering "above-threshold + real snapshot → one row" and
    /// "below-threshold (missing transcript) → no row, no panic".
    ///
    /// Env-safety: this test NEVER sets `CONTEXT_GOVERNOR_STATE_DIR`, so it
    /// cannot interfere with `backing::open_is_ok_and_creates_state_dir_for_any_cwd`.
    ///
    /// The `MockStore` holds the snapshot in memory, so no file is written to any
    /// env-var-derived directory.  For Scenario A, a real transcript file with a
    /// `message.usage.input_tokens` of 15 000 is written so `last_usage_tokens`
    /// exceeds the default 10 000 threshold.  Scenario B uses a missing transcript
    /// path so `last_usage_tokens` returns `None` → 0 < threshold → no snapshot.
    ///
    /// The shared `crate::defaults::guard::acquire_env_lock` serialises this test
    /// with the guard's test so they do not run concurrently.
    #[test]
    fn checkpointer_emits_snapshotted_row_iff_real_snapshot() {
        let _lock = crate::defaults::guard::acquire_env_lock();
        let td = tempfile::tempdir().expect("tempdir");

        // ── Scenario A: above-threshold + real snapshot → one row ─────────────
        {
            let cwd = td
                .path()
                .join("checkpointer-real")
                .to_str()
                .unwrap()
                .to_string();

            // Transcript carries usage so last_usage_tokens returns 15 000 > threshold.
            let tpath = td.path().join("cp-real-transcript.jsonl");
            std::fs::write(
                &tpath,
                concat!(
                    "{\"message\":{\"role\":\"user\",\"content\":\"hello world please do the task\"}}\n",
                    "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"task done\"}],",
                    "\"usage\":{\"input_tokens\":15000,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n",
                ),
            )
            .expect("write transcript");

            let mut store = MockStore::with_snapshot("hello world please do the task\n\ntask done");
            let input = HookInput {
                transcript_path: tpath.to_str().unwrap().to_string(),
                cwd: cwd.clone(),
                hook_event_name: "Stop".to_string(),
                stop_hook_active: false,
                ..Default::default()
            };

            let sink = current_ledger_path(&cwd);
            let mut cp = DefaultCheckpointer;
            cp.checkpoint(&input, &mut store);

            assert_eq!(
                count_snapshotted(&sink),
                1,
                "above-threshold + real snapshot must produce one snapshotted row; sink={sink:?}"
            );
        }

        // ── Scenario B: missing transcript → below threshold → no row ─────────
        {
            let cwd = td
                .path()
                .join("checkpointer-missing")
                .to_str()
                .unwrap()
                .to_string();
            let mut store = MockStore::without_snapshot();
            let input = HookInput {
                transcript_path: "/no/such/cp-test-transcript.jsonl".to_string(),
                cwd: cwd.clone(),
                hook_event_name: "Stop".to_string(),
                stop_hook_active: false,
                ..Default::default()
            };

            let sink = current_ledger_path(&cwd);
            let mut cp = DefaultCheckpointer;
            cp.checkpoint(&input, &mut store); // must not panic

            assert_eq!(
                count_snapshotted(&sink),
                0,
                "missing transcript (below threshold) must produce no snapshotted row"
            );
        }
    }
}
