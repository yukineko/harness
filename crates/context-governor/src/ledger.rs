//! Append-only action ledger (§13), with size metrics.
//!
//! Distinct from `harness_core::ledger` (that one is budgetguard's *daily spend*
//! ledger). This records, for every hook decision, a single node — satisfying
//! I6 (observability): each hook judgement leaves exactly one
//! injected / groomed{saved} / snapshotted / pinned / recalled trace. Per turn
//! the governor records `resident_tokens`, `groom saved_tokens`, and the growth
//! slope, then ships them to the metrics sink (beacon/Langfuse) via
//! `harness_core::metrics::emit`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::types::{ItemId, StoreKey};
use harness_core::metrics;
use harness_core::store::{project_key, safe_session};

/// What a hook did, with the size delta where one applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Reference body / pins injected beside the prompt (size: retrieval).
    Injected,
    /// A tool result trimmed — `saved_tokens` is the size reclaimed (I4).
    Groomed { saved_tokens: u32 },
    /// Transcript/verbatim externalized to the backing store (correctness).
    Snapshotted { to: StoreKey },
    /// A pin re-asserted into the final context (I1).
    Pinned,
    /// An externalized item pulled back in (lossless round-trip, I2).
    Recalled { from: StoreKey },
}

/// One append-only ledger node. `reason` is a `&'static str` so the cause is a
/// fixed vocabulary, not free text — the ledger stays queryable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerNode {
    pub session: String,
    pub hook: String,
    pub item: Option<ItemId>,
    pub action: Action,
    pub reason: &'static str,
}

/// Resolve the session-scoped `state_dir` and the `safe_session` string the same
/// way [`crate::backing::TranscriptBackingStore::open`] does, so the ledger lands
/// beside the backing store for the same project+session. Returns
/// `(state_dir, session)`.
fn resolve_state(cwd: &str) -> (PathBuf, String) {
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

    let state_dir = base.join(project_key(Path::new(cwd))).join(session.clone());
    (state_dir, session)
}

/// Durable JSONL sink for ledger nodes. Wraps [`harness_core::metrics::emit`]
/// appending to `<state_dir>/ledger.jsonl` — the same parallel-session-safe
/// substrate the backing store uses. Every method is best-effort and never
/// panics: a hook decision must never break a turn over its own bookkeeping.
pub struct Ledger {
    sink: PathBuf,
    session: String,
}

impl Ledger {
    /// Open (or lazily create) the ledger under `cwd`. Best-effort: directory
    /// creation failures degrade rather than propagate, so this returns `Self`
    /// directly — call sites stay panic-free.
    pub fn open(cwd: &str) -> Self {
        let (state_dir, session) = resolve_state(cwd);
        let _ = std::fs::create_dir_all(&state_dir);
        let sink = state_dir.join("ledger.jsonl");
        Self { sink, session }
    }

    /// Append one node as a single JSONL line via [`metrics::emit`]. `resident_tokens`
    /// is the caller's per-action estimate of post-action window occupancy; the
    /// ledger records whatever it is given. Best-effort — never panics or unwraps.
    pub fn append(&self, node: &LedgerNode, resident_tokens: u32) {
        let (event, saved): (&str, u32) = match node.action {
            Action::Injected => ("injected", 0),
            Action::Groomed { saved_tokens } => ("groomed", saved_tokens),
            Action::Snapshotted { .. } => ("snapshotted", 0),
            Action::Pinned => ("pinned", 0),
            Action::Recalled { .. } => ("recalled", 0),
        };

        let extra = json!({
            "saved_tokens": saved,
            "resident_tokens": resident_tokens,
            "hook": node.hook,
            "reason": node.reason,
            "item": node.item.map(|i| i.0),
        });

        // Use the resolved, env-derived session (the one that scopes `sink`), so
        // every line in this file shares one canonical session id.
        metrics::emit(&self.sink, &self.session, event, extra);
    }
}

/// Deterministic rollup of a session's ledger. `per_event` is a `BTreeMap` so the
/// summary serializes in a stable key order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerSummary {
    /// Sum of `saved_tokens` across every recorded row.
    pub total_saved_tokens: u64,
    /// Total number of rows parsed.
    pub rows: u64,
    /// Count of rows per `event` name, in deterministic key order.
    pub per_event: BTreeMap<String, u64>,
}

/// Read `<state_dir>/ledger.jsonl` for `cwd` and aggregate it. Fail-soft: a
/// missing file yields an empty summary, and corrupt/partial lines are skipped
/// rather than panicking.
pub fn rollup(cwd: &str) -> LedgerSummary {
    let (state_dir, _session) = resolve_state(cwd);
    summarize_jsonl(&state_dir.join("ledger.jsonl"))
}

/// Aggregate a ledger JSONL file at `sink`. Pure function of the path — no env —
/// so callers (and tests) get a deterministic result. Fail-soft as above.
fn summarize_jsonl(sink: &Path) -> LedgerSummary {
    let mut summary = LedgerSummary::default();
    let Ok(contents) = std::fs::read_to_string(sink) else {
        return summary;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        summary.rows += 1;
        summary.total_saved_tokens += value
            .get("saved_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(event) = value.get("event").and_then(|v| v.as_str()) {
            *summary.per_event.entry(event.to_string()).or_insert(0) += 1;
        }
    }

    summary
}

/// Fail-soft seen-state query for the injector's dedup (I6 observe→act, read
/// half): returns `true` iff this session's ledger already records an `injected`
/// row whose `item` equals `item_id`. A missing/corrupt file or absent match
/// yields `false`, so the caller treats "unknown" as "not yet injected" and
/// proceeds — the ledger becomes the control input without ever breaking a turn.
pub fn was_injected(cwd: &str, item_id: u64) -> bool {
    let (state_dir, _session) = resolve_state(cwd);
    seen_injected_in(&state_dir.join("ledger.jsonl"), item_id)
}

/// Path-pinned core of [`was_injected`]: pure function of `sink` (no env), so it
/// is deterministic and unit-testable the same way [`summarize_jsonl`] is.
/// Fail-soft: a missing/corrupt file or absent match yields `false`.
fn seen_injected_in(sink: &Path, item_id: u64) -> bool {
    let Ok(contents) = std::fs::read_to_string(sink) else {
        return false;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_injected = value.get("event").and_then(|v| v.as_str()) == Some("injected");
        let item_matches = value.get("item").and_then(|v| v.as_u64()) == Some(item_id);
        if is_injected && item_matches {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberately env-FREE. `CONTEXT_GOVERNOR_STATE_DIR` / `CLAUDE_CODE_SESSION_ID`
    // are process-global; a sibling unit test (`backing::open_is_ok…`) already
    // asserts on a base it sets in that var, so mutating it here would race and
    // flake *that* frozen test. Instead we construct `Ledger` directly against a
    // private tempdir (the same discipline `backing::tests::temp_store` uses) and
    // assert on the path-pinned summary core `summarize_jsonl`, which is the exact
    // logic `rollup` runs. `Ledger::open` + `rollup` are read-only over the env, so
    // they are smoke-called (no panic) without mutating anything global.
    #[test]
    fn append_and_rollup_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // Missing file → empty summary (nonexistent path, no env).
        assert_eq!(
            summarize_jsonl(Path::new("/tmp/ledger-sink-absent-xyz/ledger.jsonl")),
            LedgerSummary::default()
        );

        // Construct directly: env-immune, never touches a real $HOME or the shared
        // state-dir var. Mirrors `backing::tests::temp_store`.
        let ledger = Ledger {
            sink: tmp.path().join("ledger.jsonl"),
            session: "S1".to_string(),
        };

        let groomed = LedgerNode {
            session: "S1".to_string(),
            hook: "PostToolUse".to_string(),
            item: Some(ItemId(7)),
            action: Action::Groomed { saved_tokens: 50 },
            reason: "oversized-tool-result",
        };
        let injected = LedgerNode {
            session: "S1".to_string(),
            hook: "UserPromptSubmit".to_string(),
            item: None,
            action: Action::Injected,
            reason: "pin-reinjection",
        };

        ledger.append(&groomed, 1200);
        ledger.append(&injected, 1180);

        let contents = std::fs::read_to_string(&ledger.sink).expect("ledger file");
        let lines: Vec<&str> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "one JSONL line per appended node");
        for line in &lines {
            assert!(
                line.contains("saved_tokens"),
                "line carries saved_tokens: {line}"
            );
            assert!(
                line.contains("resident_tokens"),
                "line carries resident_tokens: {line}"
            );
        }

        // Numeric assertions against the path-pinned core — the exact logic rollup runs.
        let summary = summarize_jsonl(&ledger.sink);
        assert_eq!(summary.total_saved_tokens, 50);
        assert_eq!(summary.rows, 2);
        assert_eq!(summary.per_event.get("groomed"), Some(&1));
        assert_eq!(summary.per_event.get("injected"), Some(&1));

        // Smoke-exercise the public env-derived entry points: they read the env
        // (never mutate it) and must never panic, even on a never-written cwd.
        let _ = Ledger::open("/tmp/ledger-sink-smoke-cwd");
        let _ = rollup("/tmp/ledger-sink-smoke-cwd");
    }

    /// `seen_injected_in` (the env-free core of `was_injected`) detects a prior
    /// `injected` row by its `item` id, ignores non-injected rows with the same
    /// id, and fail-softs on an absent file / no match.
    #[test]
    fn seen_injected_in_matches_injected_row_by_item() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sink = tmp.path().join("ledger.jsonl");

        // Missing file → false (fail-soft).
        assert!(!seen_injected_in(&sink, 42));

        let ledger = Ledger {
            sink: sink.clone(),
            session: "S1".to_string(),
        };

        // A groomed row carrying item=42 must NOT match (event != injected).
        ledger.append(
            &LedgerNode {
                session: "S1".to_string(),
                hook: "PostToolUse".to_string(),
                item: Some(ItemId(42)),
                action: Action::Groomed { saved_tokens: 10 },
                reason: "oversized-tool-result",
            },
            100,
        );
        assert!(
            !seen_injected_in(&sink, 42),
            "a groomed row must not count as injected"
        );

        // An injected row with item=42 → matches; a different id does not.
        ledger.append(
            &LedgerNode {
                session: "S1".to_string(),
                hook: "UserPromptSubmit".to_string(),
                item: Some(ItemId(42)),
                action: Action::Injected,
                reason: "reference-injection",
            },
            80,
        );
        assert!(
            seen_injected_in(&sink, 42),
            "injected row with matching item must be seen"
        );
        assert!(
            !seen_injected_in(&sink, 7),
            "a different item id must not match"
        );
    }
}
