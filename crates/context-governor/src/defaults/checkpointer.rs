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
    use std::io::{Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};

    // ── In-memory BackingStore mock ───────────────────────────────────────────

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

    // ── Ledger-path helpers ───────────────────────────────────────────────────

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

    fn count_snapshotted_in_str(s: &str) -> usize {
        s.lines().filter(|l| l.contains("\"snapshotted\"")).count()
    }

    fn count_snapshotted(sink: &Path) -> usize {
        count_snapshotted_in_str(&std::fs::read_to_string(sink).unwrap_or_default())
    }

    fn preopen_ledger(sink: &Path) -> Option<std::fs::File> {
        if let Some(parent) = sink.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(sink)
            .ok()
    }

    fn count_via_fd_or_path(
        fd: &mut Option<std::fs::File>,
        sink_preopen: &Path,
        cwd: &str,
    ) -> usize {
        if let Some(ref mut f) = fd {
            let _ = f.seek(SeekFrom::Start(0));
            let mut content = String::new();
            let _ = f.read_to_string(&mut content);
            let n = count_snapshotted_in_str(&content);
            if n > 0 {
                return n;
            }
        }
        let n = count_snapshotted(sink_preopen);
        if n > 0 {
            return n;
        }
        let sink_now = current_ledger_path(cwd);
        if sink_now != sink_preopen {
            return count_snapshotted(&sink_now);
        }
        0
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Covers "above-threshold + real snapshot → one row" and
    /// "below-threshold (missing transcript) → no row, no panic".
    ///
    /// # Env-safety
    ///
    /// This test NEVER mutates `CONTEXT_GOVERNOR_STATE_DIR`.  The `MockStore`
    /// holds the snapshot in memory.  For the ledger, a fd is pre-opened before
    /// the handler is called; the handler appends to the same inode; if
    /// `backing::open_is_ok_and_creates_state_dir_for_any_cwd` deletes the
    /// parent directory between the handler write and our path-based read, the
    /// fd persists (Unix inode refcount) and we read through it instead.
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

            // Write a transcript file so last_usage_tokens returns 15 000 > 10 000.
            let tpath = td.path().join("cp-real-transcript.jsonl");
            std::fs::write(
                &tpath,
                concat!(
                    "{\"message\":{\"role\":\"user\",\"content\":\"hello world\"}}\n",
                    "{\"message\":{\"role\":\"assistant\",",
                    "\"content\":[{\"type\":\"text\",\"text\":\"done\"}],",
                    "\"usage\":{\"input_tokens\":15000,",
                    "\"cache_read_input_tokens\":0,",
                    "\"cache_creation_input_tokens\":0}}}\n",
                ),
            )
            .expect("write transcript");

            let mut store = MockStore::with_snapshot("hello world\n\ndone");
            let input = HookInput {
                transcript_path: tpath.to_str().unwrap().to_string(),
                cwd: cwd.clone(),
                hook_event_name: "Stop".to_string(),
                stop_hook_active: false,
                ..Default::default()
            };

            let sink = current_ledger_path(&cwd);
            let mut fd = preopen_ledger(&sink);
            let mut cp = DefaultCheckpointer;
            cp.checkpoint(&input, &mut store);

            let count = count_via_fd_or_path(&mut fd, &sink, &cwd);
            assert_eq!(
                count, 1,
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
            let mut fd = preopen_ledger(&sink);
            let mut cp = DefaultCheckpointer;
            cp.checkpoint(&input, &mut store);

            let count = count_via_fd_or_path(&mut fd, &sink, &cwd);
            assert_eq!(
                count, 0,
                "missing transcript (below threshold) must produce no snapshotted row"
            );
        }
    }
}
