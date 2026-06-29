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

/// Hex fingerprint of a template's bytes. Non-cryptographic (FNV-1a, 64-bit via
/// the shared `harness_core::hash`): we only need to detect that a template
/// changed since ratification, not resist adversarial collisions.
pub fn hash(s: &str) -> String {
    format!("{:016x}", harness_core::hash::fnv1a64(s.as_bytes()))
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
    /// V1 refute template fingerprint. `default` for backward compatibility with
    /// locks written before the verification gates existed: an old lock simply
    /// has no refute policy pinned, so it only re-blocks once the refute gate is
    /// turned on (see [`drifted`]).
    #[serde(default)]
    pub refute_hash: String,
    /// V2 completeness template fingerprint (same backward-compat note).
    #[serde(default)]
    pub completeness_hash: String,
    #[serde(default)]
    pub canon_commit: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub reason: String,
}

/// The fingerprints of the four prompt templates (meta-canon) at ratification or
/// check time. The verify hashes are empty when their gate is inactive — consent
/// is scoped to the policy surface that is actually live.
pub struct TemplateHashes {
    pub audit: String,
    pub decisions: String,
    pub refute: String,
    pub completeness: String,
}

/// Read the ratification lock, if present and parseable.
pub fn read_lock(repo_root: &Path) -> Option<Lock> {
    let text = std::fs::read_to_string(lock_path(repo_root)).ok()?;
    toml::from_str(&text).ok()
}

/// Write (or overwrite) the lock — the act of ratification. The verify hashes in
/// `h` are empty when their gate is inactive, so consent is pinned only for the
/// live policy surface; activating a gate later leaves its slot empty and forces
/// a fresh ratification (see [`drifted`]).
pub fn write_lock(
    repo_root: &Path,
    h: &TemplateHashes,
    canon_commit: &str,
    date: &str,
    reason: &str,
) -> Result<PathBuf> {
    let path = lock_path(repo_root);
    let reason_esc = reason.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        "# specguard prompt ratification lock.\n\
         # The prompt templates are meta-canon (the audit + verification policy).\n\
         # This file pins the version a human ratified, with the reason. The\n\
         # refute/completeness hashes are pinned only when their [verify] gate is\n\
         # on. Regenerate via `specguard accept-prompt`. Commit this file.\n\
         audit_hash = \"{}\"\n\
         decisions_hash = \"{}\"\n\
         refute_hash = \"{}\"\n\
         completeness_hash = \"{}\"\n\
         canon_commit = \"{canon_commit}\"\n\
         date = \"{date}\"\n\
         reason = \"{reason_esc}\"\n",
        h.audit, h.decisions, h.refute, h.completeness
    );
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Which templates changed vs the lock (empty = still ratified). The audit and
/// decisions policies are always checked; the verify policies are checked only
/// when their gate is active, so a project that never enables a gate is never
/// asked to ratify inert policy — and turning a gate on (its slot empty in the
/// lock) registers as drift, demanding a fresh, meaningful consent.
pub fn drifted(
    lock: &Lock,
    h: &TemplateHashes,
    refute_active: bool,
    completeness_active: bool,
) -> Vec<&'static str> {
    let mut v = Vec::new();
    if lock.audit_hash != h.audit {
        v.push("audit-prompt");
    }
    if lock.decisions_hash != h.decisions {
        v.push("decisions-prompt");
    }
    if refute_active && lock.refute_hash != h.refute {
        v.push("refute-prompt");
    }
    if completeness_active && lock.completeness_hash != h.completeness {
        v.push("completeness-prompt");
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

    fn lock_with(audit: &str, decisions: &str, refute: &str, completeness: &str) -> Lock {
        Lock {
            audit_hash: audit.into(),
            decisions_hash: decisions.into(),
            refute_hash: refute.into(),
            completeness_hash: completeness.into(),
            canon_commit: String::new(),
            date: String::new(),
            reason: String::new(),
        }
    }

    fn hashes(audit: &str, decisions: &str, refute: &str, completeness: &str) -> TemplateHashes {
        TemplateHashes {
            audit: audit.into(),
            decisions: decisions.into(),
            refute: refute.into(),
            completeness: completeness.into(),
        }
    }

    #[test]
    fn drifted_flags_changed_audit_and_decisions() {
        let lock = lock_with("a", "d", "", "");
        // Verify gates inactive: only audit + decisions are checked.
        assert!(drifted(&lock, &hashes("a", "d", "z", "z"), false, false).is_empty());
        assert_eq!(
            drifted(&lock, &hashes("x", "d", "", ""), false, false),
            vec!["audit-prompt"]
        );
        assert_eq!(
            drifted(&lock, &hashes("a", "y", "", ""), false, false),
            vec!["decisions-prompt"]
        );
    }

    #[test]
    fn verify_policy_checked_only_when_gate_active() {
        let lock = lock_with("a", "d", "r", "c");
        // Active + matching -> no drift.
        assert!(drifted(&lock, &hashes("a", "d", "r", "c"), true, true).is_empty());
        // Active + changed refute -> flagged; completeness inactive so ignored.
        assert_eq!(
            drifted(&lock, &hashes("a", "d", "X", "Y"), true, false),
            vec!["refute-prompt"]
        );
        // Both active + both changed.
        assert_eq!(
            drifted(&lock, &hashes("a", "d", "X", "Y"), true, true),
            vec!["refute-prompt", "completeness-prompt"]
        );
    }

    #[test]
    fn enabling_gate_against_unpinned_lock_is_drift() {
        // An old/audit-only lock has no refute policy pinned (empty). Turning the
        // refute gate on registers as drift -> forces a fresh ratification.
        let lock = lock_with("a", "d", "", "");
        assert_eq!(
            drifted(&lock, &hashes("a", "d", "r", ""), true, false),
            vec!["refute-prompt"]
        );
    }
}
