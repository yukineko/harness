//! Per-session ring buffer of recent events + per-pattern nudge bookkeeping,
//! persisted as one small JSON file so detection survives across the many
//! separate hook process invocations within a session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sig::Event;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Nudge {
    pub count: u32,
    pub last_seq: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub seq: u64,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub nudges: HashMap<String, Nudge>,
}

fn safe(session: &str) -> String {
    session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn path(state_dir: &Path, session: &str) -> PathBuf {
    state_dir
        .join("sessions")
        .join(format!("{}.json", safe(session)))
}

pub fn load(state_dir: &Path, session: &str) -> SessionState {
    harness_core::store::load_json(&path(state_dir, session))
}

pub fn save(state_dir: &Path, session: &str, st: &SessionState) {
    harness_core::store::save_json(&path(state_dir, session), st);
}

impl SessionState {
    /// Append an event, assigning the next seq, and prune to `window`.
    pub fn push(&mut self, mut e: Event, window: usize) -> u64 {
        self.seq += 1;
        e.seq = self.seq;
        self.events.push(e);
        let len = self.events.len();
        if len > window {
            self.events.drain(0..len - window);
        }
        self.seq
    }

    /// Record a nudge for `key`; return the resulting (1-based) nudge count.
    pub fn record_nudge(&mut self, key: &str, seq: u64) -> u32 {
        let n = self.nudges.entry(key.to_string()).or_default();
        n.count += 1;
        n.last_seq = seq;
        n.count
    }

    /// True if this pattern was nudged within `cooldown` events of `seq`.
    pub fn in_cooldown(&self, key: &str, seq: u64, cooldown: u64) -> bool {
        self.nudges
            .get(key)
            .map(|n| seq.saturating_sub(n.last_seq) < cooldown)
            .unwrap_or(false)
    }
}
