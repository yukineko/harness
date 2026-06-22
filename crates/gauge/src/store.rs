//! The session record store. Each Claude Code session is one JSON file under
//! `<state_dir>/sessions/<session_id>.json`, rewritten on every Stop with the
//! cumulative totals for the whole session so far (the hook re-reads the full
//! transcript each time, so the latest write is authoritative).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Token counts for one model within a session. `input`/`output` are the
/// uncached counts; cache writes/reads are tracked separately for pricing.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    #[serde(default)]
    pub cache_read: u64,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input + self.output + self.cache_write_5m + self.cache_write_1h + self.cache_read
    }

    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub project: String,
    pub cwd: String,
    #[serde(default)]
    pub models: BTreeMap<String, Usage>,
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub tools: BTreeMap<String, u64>,
    #[serde(default)]
    pub first_ts: Option<String>,
    #[serde(default)]
    pub last_ts: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

impl SessionRecord {
    /// Total tokens across all models.
    pub fn total_tokens(&self) -> u64 {
        self.models.values().map(|u| u.total_tokens()).sum()
    }

    /// The day (YYYY-MM-DD) this session last ran, from `last_ts`.
    pub fn day(&self) -> Option<String> {
        self.last_ts
            .as_ref()
            .and_then(|t| t.get(0..10).map(|s| s.to_string()))
    }
}

pub fn sessions_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("sessions")
}

/// Keep only filesystem-safe characters so a hostile/empty session id can't
/// escape the store directory.
fn safe_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

/// Write (overwrite) the record for its session. Fail-soft: any IO error is
/// swallowed so the hook never disturbs the turn.
pub fn upsert(state_dir: &Path, rec: &SessionRecord) {
    let dir = sessions_dir(state_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{}.json", safe_id(&rec.session_id)));
    if let Ok(text) = serde_json::to_string_pretty(rec) {
        let _ = std::fs::write(path, text);
    }
}

/// Load every session record in the store (skipping anything unparseable).
pub fn load_all(state_dir: &Path) -> Vec<SessionRecord> {
    let dir = sessions_dir(state_dir);
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(rec) = serde_json::from_str::<SessionRecord>(&text) {
                out.push(rec);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_id_strips_separators() {
        assert_eq!(safe_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_id("abc-123_DEF"), "abc-123_DEF");
        assert_eq!(safe_id(""), "unknown");
    }

    #[test]
    fn usage_totals() {
        let u = Usage {
            input: 10,
            output: 20,
            cache_write_5m: 1,
            cache_write_1h: 2,
            cache_read: 3,
        };
        assert_eq!(u.total_tokens(), 36);
    }
}
