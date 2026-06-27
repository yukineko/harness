//! Read-only budget-pressure check.
//!
//! budgetguard is a post-hoc Stop gate: it only reacts *after* a turn has
//! already spent. fugu-router picks models *before* the spend. When the day's
//! budget is under pressure (spend has reached the daily warn threshold), the
//! router should bias cheaper — shaving a tier and suppressing opus escalation —
//! so the remaining budget isn't burned on an opus×N fan-out.
//!
//! This asks budgetguard for its deterministic pressure verdict
//! (`budgetguard status --json`). Soft dependency: if budgetguard isn't
//! installed (or errors / emits garbage) we return `false` and routing is
//! unchanged. Read-only — never writes.

use std::path::PathBuf;
use std::process::Command;

use harness_core::config::home;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Status {
    #[serde(default)]
    pressure: bool,
}

/// True iff budgetguard reports the day's spend has reached the warn threshold.
/// `false` when budgetguard is absent/errors (soft dep → routing unchanged).
pub fn under_pressure() -> bool {
    let Some(binary) = find_budgetguard_binary() else {
        return false;
    };
    let Ok(out) = Command::new(&binary).args(["status", "--json"]).output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    parse_pressure(&out.stdout)
}

/// Parse `budgetguard status --json` stdout → pressure bit. Split out so the
/// decode is unit-testable without a real budgetguard binary.
fn parse_pressure(stdout: &[u8]) -> bool {
    serde_json::from_slice::<Status>(stdout)
        .map(|s| s.pressure)
        .unwrap_or(false)
}

/// Locate budgetguard: PATH first, then the plugin cache (newest version).
fn find_budgetguard_binary() -> Option<PathBuf> {
    if Command::new("budgetguard")
        .arg("--version")
        .output()
        .is_ok()
    {
        return Some(PathBuf::from("budgetguard"));
    }
    let base = home()
        .join(".claude")
        .join("plugins")
        .join("cache")
        .join("yukineko")
        .join("budgetguard");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("bin").join("budgetguard"))
        .filter(|p| p.exists())
        .collect();
    candidates.sort();
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pressure_true() {
        assert!(parse_pressure(
            br#"{"day_usd":9.0,"daily_warn_usd":5.0,"daily_block_usd":0.0,"pressure":true}"#
        ));
    }

    #[test]
    fn parses_pressure_false() {
        assert!(!parse_pressure(br#"{"pressure":false}"#));
        // Missing field defaults to no pressure.
        assert!(!parse_pressure(br#"{"day_usd":1.0}"#));
    }

    #[test]
    fn unparseable_is_no_pressure() {
        assert!(!parse_pressure(b"budgetguard: human line"));
        assert!(!parse_pressure(b""));
    }
}
