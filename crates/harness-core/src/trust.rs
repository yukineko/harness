//! Workspace trust: a shared gate for honoring *command strings* that come from
//! a project-local config file.
//!
//! Several plugins read a project-local config (`<root>/donegate.toml`,
//! `reviewgate.toml`, `beacon.toml`, `tdd.toml`, …) that "wins outright" over the
//! home config, and some of those configs carry shell command strings that the
//! plugin later runs (via `sh -c`) from a Stop / SessionStart hook. That means
//! merely opening (or cloning) a repository that ships such a file would run
//! attacker-controlled commands with the user's privileges.
//!
//! This module is the safe-by-default boundary: project-sourced commands are only
//! honored once the user has explicitly **trusted** that project root, mirroring
//! VS Code Workspace Trust / git's `safe.directory`. Until then plugins fall back
//! to the (trusted) home config or built-in defaults.
//!
//! The trust list lives in `~/.harness/trust.toml`:
//! ```toml
//! trusted = ["/abs/path/to/project", "/another/repo"]
//! ```
//! Paths are stored canonicalized (absolute) so a relative or `..`-laden cwd can
//! never spoof a trusted entry. The escape hatch `HARNESS_TRUST_ALL=1` trusts
//! every project (for CI / single-tenant machines that accept the risk).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{base_dir, env_bool};

/// On-disk form of the trust list.
#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustFile {
    #[serde(default)]
    trusted: Vec<String>,
}

/// Path to the shared trust list (`~/.harness/trust.toml`).
pub fn trust_path() -> PathBuf {
    base_dir("harness").join("trust.toml")
}

/// Env escape hatch: `HARNESS_TRUST_ALL` set truthy trusts every project.
pub fn trust_all() -> bool {
    env_bool("HARNESS_TRUST_ALL").unwrap_or(false)
}

/// Normalize a project root to a canonical absolute key. Falls back to the path
/// as given when it can't be canonicalized (e.g. it doesn't exist yet), so the
/// stored key and a later lookup of the same path still agree.
fn normalize(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

fn load_file() -> TrustFile {
    let path = trust_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str::<TrustFile>(&s).ok())
        .unwrap_or_default()
}

/// Every trusted project root, canonicalized.
pub fn list() -> Vec<PathBuf> {
    load_file()
        .trusted
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Is this project root trusted to run commands sourced from its project-local
/// config? `HARNESS_TRUST_ALL` short-circuits to `true`; otherwise the root must
/// be present (canonicalized) in `~/.harness/trust.toml`. Default: `false`.
pub fn is_trusted(root: &Path) -> bool {
    if trust_all() {
        return true;
    }
    let key = normalize(root);
    load_file().trusted.iter().any(|t| Path::new(t) == key)
}

/// Add a project root to the trust list (idempotent). Returns the canonical key
/// that was recorded. Writes atomically (tmp + rename) so a concurrent reader
/// never sees a truncated file.
pub fn add(root: &Path) -> std::io::Result<PathBuf> {
    let key = normalize(root);
    let key_str = key.to_string_lossy().into_owned();

    let mut file = load_file();
    if !file.trusted.iter().any(|t| t == &key_str) {
        file.trusted.push(key_str);
        write_file(&file)?;
    }
    Ok(key)
}

/// Remove a project root from the trust list (idempotent). Returns `true` if an
/// entry was actually removed.
pub fn remove(root: &Path) -> std::io::Result<bool> {
    let key = normalize(root);
    let mut file = load_file();
    let before = file.trusted.len();
    file.trusted.retain(|t| Path::new(t) != key);
    let removed = file.trusted.len() != before;
    if removed {
        write_file(&file)?;
    }
    Ok(removed)
}

fn write_file(file: &TrustFile) -> std::io::Result<()> {
    let path = trust_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = toml::to_string(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests mutate the process-global HOME and HARNESS_TRUST_ALL env, so they
    // must not run concurrently. A single #[test] drives the whole sequence.
    #[test]
    fn trust_roundtrip_and_env_override() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        std::env::remove_var("HARNESS_TRUST_ALL");

        let root = proj.path();

        // Default: an unregistered project is untrusted.
        assert!(!is_trusted(root), "fresh project must be untrusted");
        assert!(list().is_empty());

        // After add(): trusted, and listed (canonicalized).
        let key = add(root).unwrap();
        assert!(is_trusted(root), "added project must be trusted");
        assert_eq!(list(), vec![key.clone()]);

        // add() is idempotent — no duplicate entries.
        add(root).unwrap();
        assert_eq!(list().len(), 1, "add must be idempotent");

        // The trust file actually exists on disk where we expect it.
        assert!(trust_path().exists());

        // remove() reverses it.
        assert!(remove(root).unwrap());
        assert!(!is_trusted(root), "removed project must be untrusted");
        assert!(!remove(root).unwrap(), "remove is idempotent");

        // HARNESS_TRUST_ALL trusts everything regardless of the list.
        std::env::set_var("HARNESS_TRUST_ALL", "1");
        assert!(is_trusted(root), "HARNESS_TRUST_ALL must override");
        std::env::set_var("HARNESS_TRUST_ALL", "0");
        assert!(!is_trusted(root), "HARNESS_TRUST_ALL=0 must not trust");
        std::env::remove_var("HARNESS_TRUST_ALL");
    }
}
