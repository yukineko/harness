//! Append-only episode store: each line is one routing outcome (JSONL).
//!
//! Fail-soft throughout — a malformed line is skipped, a missing file reads as
//! empty, so a corrupt store never breaks routing or a turn.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One routing outcome: a task's features, the model that ran it, and whether it
/// passed verification (plus cost). The k-NN policy learns from these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Unix seconds when recorded (0 if unknown).
    #[serde(default)]
    pub ts: u64,
    pub title: String,
    #[serde(default)]
    pub touched_files: Vec<String>,
    #[serde(default)]
    pub class: String,
    pub model: String,
    #[serde(default = "default_role")]
    pub role: String,
    pub pass: bool,
    #[serde(default)]
    pub cost_usd: f64,
}

fn default_role() -> String {
    "worker".to_string()
}

/// Load all episodes, skipping any malformed line.
pub fn load(path: &Path) -> Vec<Episode> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Episode>(l).ok())
        .collect()
}

/// Append one episode as a JSON line, creating parent dirs as needed.
pub fn append(path: &Path, ep: &Episode) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(ep).unwrap_or_default();
    writeln!(f, "{line}")
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A verified task's procedure record — stored separately from Episodes so
/// routing statistics stay unaffected by the larger procedure text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    #[serde(default)]
    pub ts: u64,
    pub title: String,
    #[serde(default)]
    pub touched_files: Vec<String>,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub done_criteria: String,
    #[serde(default)]
    pub notes: String,
}

/// Load all playbook entries, skipping any malformed line.
pub fn load_playbooks(path: &Path) -> Vec<Playbook> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Playbook>(l).ok())
        .collect()
}

/// Append one playbook entry as a JSON line, creating parent dirs as needed.
pub fn append_playbook(path: &Path, pb: &Playbook) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(pb).unwrap_or_default();
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playbook_roundtrip() {
        let dir = std::env::temp_dir().join("fugu-router-playbook-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("playbooks.jsonl");
        let _ = std::fs::remove_file(&path);
        let pb = Playbook {
            ts: 42,
            title: "add auth endpoint".into(),
            touched_files: vec!["src/auth.rs".into()],
            class: "serial".into(),
            done_criteria: "cargo test passes".into(),
            notes: "use bcrypt".into(),
        };
        append_playbook(&path, &pb).unwrap();
        // malformed line must be skipped
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"garbage\n")
            .unwrap();
        append_playbook(&path, &pb).unwrap();
        let loaded = load_playbooks(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "add auth endpoint");
        assert_eq!(loaded[0].done_criteria, "cargo test passes");
    }

    #[test]
    fn load_playbooks_missing_file_returns_empty() {
        let path = std::path::PathBuf::from("/tmp/nonexistent_playbooks_12345.jsonl");
        assert!(load_playbooks(&path).is_empty());
    }

    #[test]
    fn skips_malformed_lines() {
        let dir = std::env::temp_dir().join("fugu-router-store-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("episodes.jsonl");
        let _ = std::fs::remove_file(&path);
        let ep = Episode {
            ts: 1,
            title: "add login endpoint".into(),
            touched_files: vec!["src/auth/login.ts".into()],
            class: "parallel".into(),
            model: "sonnet".into(),
            role: "worker".into(),
            pass: true,
            cost_usd: 0.12,
        };
        append(&path, &ep).unwrap();
        // a junk line must not break the load
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        append(&path, &ep).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].model, "sonnet");
    }
}
