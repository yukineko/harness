//! Prompt ratification — the prompt templates are *meta-canon* (the audit policy
//! that decides what counts as drift). Treating them as canon means a change
//! must be (1) contract-checked, (2) consented to with a rationale, and (3)
//! pinned. This module records that consent (a lock file holding the template
//! fingerprints + the canon commit + the reason) and verifies it before a gated
//! `run`. Human consent is what confers canon authority and terminates the
//! "who audits the auditor" regress; the lock is the pinned record of it.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Non-cryptographic content fingerprint (FNV-1a, 64-bit). We only need to
/// detect that a template changed since ratification — not resist adversarial
/// collisions — so a fast, dependency-free hash suffices.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Hex fingerprint of a template's bytes.
pub fn hash(s: &str) -> String {
    format!("{:016x}", fnv1a(s.as_bytes()))
}

/// The ratification lock lives at the repo root and SHOULD be committed: it is
/// the pinned record of human consent to a prompt version.
pub fn lock_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".specguard-prompt.lock")
}

#[derive(Debug, Deserialize)]
pub struct Lock {
    pub audit_hash: String,
    pub decisions_hash: String,
    #[serde(default)]
    pub canon_commit: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub reason: String,
}

/// Read the ratification lock, if present and parseable.
pub fn read_lock(repo_root: &Path) -> Option<Lock> {
    let text = std::fs::read_to_string(lock_path(repo_root)).ok()?;
    toml::from_str(&text).ok()
}

/// Write (or overwrite) the lock — the act of ratification.
pub fn write_lock(
    repo_root: &Path,
    audit_hash: &str,
    decisions_hash: &str,
    canon_commit: &str,
    date: &str,
    reason: &str,
) -> Result<PathBuf> {
    let path = lock_path(repo_root);
    let reason_esc = reason.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        "# specguard prompt ratification lock.\n\
         # The prompt templates are meta-canon (the audit policy). This file pins\n\
         # the version a human ratified, with the reason. Regenerate via\n\
         # `specguard accept-prompt`. Commit this file.\n\
         audit_hash = \"{audit_hash}\"\n\
         decisions_hash = \"{decisions_hash}\"\n\
         canon_commit = \"{canon_commit}\"\n\
         date = \"{date}\"\n\
         reason = \"{reason_esc}\"\n"
    );
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Which templates changed vs the lock (empty = still ratified).
pub fn drifted(lock: &Lock, audit_hash: &str, decisions_hash: &str) -> Vec<&'static str> {
    let mut v = Vec::new();
    if lock.audit_hash != audit_hash {
        v.push("audit-prompt");
    }
    if lock.decisions_hash != decisions_hash {
        v.push("decisions-prompt");
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_sensitive() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
        assert_eq!(hash("abc").len(), 16);
    }

    #[test]
    fn drifted_flags_changed_templates() {
        let lock = Lock {
            audit_hash: "a".into(),
            decisions_hash: "d".into(),
            canon_commit: String::new(),
            date: String::new(),
            reason: String::new(),
        };
        assert!(drifted(&lock, "a", "d").is_empty());
        assert_eq!(drifted(&lock, "x", "d"), vec!["audit-prompt"]);
        assert_eq!(drifted(&lock, "a", "y"), vec!["decisions-prompt"]);
        assert_eq!(drifted(&lock, "x", "y"), vec!["audit-prompt", "decisions-prompt"]);
    }
}
