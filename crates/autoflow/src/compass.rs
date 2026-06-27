//! Read-only compass freshness check.
//!
//! flow's invariant (`flow` SKILL.md §"盲目実行しない"): never auto-drive the
//! queue unless the compass charter is sharp. autoflow's auto-loop drove the
//! backlog (`block: /backlog`) without ever consulting compass — bypassing that
//! gate. This module asks compass for its deterministic C1/C2 freshness verdict
//! (`compass nudge --json`) so autoflow can stand down and nudge the user toward
//! `/compass` instead of blind-driving a stale charter.
//!
//! Soft dependency: if compass isn't installed (or errors / emits garbage) we
//! return `None`, and the caller preserves today's behavior — a repo that
//! doesn't use compass keeps auto-driving as before. This module only READS
//! (shells out to a hook subcommand that always exits 0); it never writes.

use std::path::{Path, PathBuf};
use std::process::Command;

use harness_core::config::home;
use serde::Deserialize;

/// compass's machine-readable freshness verdict (`compass nudge --json`).
#[derive(Debug, Deserialize)]
pub struct Verdict {
    /// `true` iff the charter is sharp enough to auto-act on (C1 present +
    /// non-blurry, C2 not drift-suspect).
    pub fresh: bool,
    /// Human-readable nudge text; present iff `!fresh`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Ask compass whether the charter for the repo containing `cwd` is fresh.
///
/// `None` means "can't tell" — compass absent, errored, or emitted unparseable
/// output — and the caller should preserve its prior behavior (proceed). A
/// `Some(verdict)` carries compass's deterministic answer.
pub fn charter_freshness(cwd: &Path) -> Option<Verdict> {
    let binary = find_compass_binary()?;
    let out = Command::new(&binary)
        .args(["nudge", "--json"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_verdict(&out.stdout)
}

/// Parse `compass nudge --json` stdout into a [`Verdict`]. Split out for unit
/// testing without a real compass binary on PATH.
fn parse_verdict(stdout: &[u8]) -> Option<Verdict> {
    serde_json::from_slice(stdout).ok()
}

/// Locate the compass binary: PATH first, then the plugin cache (newest version).
fn find_compass_binary() -> Option<PathBuf> {
    if Command::new("compass").arg("--version").output().is_ok() {
        return Some(PathBuf::from("compass"));
    }

    // ~/.claude/plugins/cache/yukineko/compass/<version>/bin/compass
    let base = home()
        .join(".claude")
        .join("plugins")
        .join("cache")
        .join("yukineko")
        .join("compass");

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("bin").join("compass"))
        .filter(|p| p.exists())
        .collect();

    candidates.sort();
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stale_verdict() {
        let v =
            parse_verdict(br#"{"fresh":false,"reason":"charter may be stale (x) - run /compass"}"#)
                .expect("parse");
        assert!(!v.fresh);
        assert!(v.reason.as_deref().unwrap_or("").contains("stale"));
    }

    #[test]
    fn parses_fresh_verdict_with_null_reason() {
        let v = parse_verdict(br#"{"fresh":true,"reason":null}"#).expect("parse");
        assert!(v.fresh);
        assert!(v.reason.is_none());
    }

    #[test]
    fn unparseable_output_is_none() {
        assert!(parse_verdict(b"compass: some human line, not json").is_none());
        assert!(parse_verdict(b"").is_none());
    }
}
