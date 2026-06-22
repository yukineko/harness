//! Generic `~/.claude/settings.json` mechanics shared by every plugin's
//! install/uninstall: load (tolerating absent/empty files), timestamped backup,
//! pretty write, and ownership detection by command-substring markers.
//!
//! Each plugin keeps its own event table, ownership markers, and status-line
//! command (which differ per plugin) and drives these helpers.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// Load a settings JSON object, treating a missing or empty file as `{}`.
pub fn load_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Back up `path` to `<path>.bak-<YYYYmmdd-HHMMSS>` before a write. No-op if the
/// file does not exist. Prints the backup location (matches existing UX).
pub fn backup(path: &Path) -> Result<()> {
    if path.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let bak = path.with_extension(format!("json.bak-{stamp}"));
        std::fs::copy(path, &bak)?;
        println!("backup: {}", bak.display());
    }
    Ok(())
}

/// Backup, then pretty-write `value` to `path` (creating the parent dir). Prints
/// the updated path.
pub fn write_settings(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    backup(path)?;
    let text = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(path, text)?;
    println!("updated: {}", path.display());
    Ok(())
}

/// True if a hook *group* contains any command whose string contains one of
/// `markers` — i.e. something this plugin owns and should replace.
pub fn group_matches(group: &Value, markers: &[&str]) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| markers.iter().any(|m| c.contains(m)))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Drop every group matching `markers` from an event array; returns the cleaned
/// array (foreign groups preserved).
pub fn strip_matching(arr: &[Value], markers: &[&str]) -> Vec<Value> {
    arr.iter().filter(|g| !group_matches(g, markers)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_matches_any_marker() {
        let g = json!({"hooks":[{"type":"command","command":"/x/ctxrot guard"}]});
        assert!(group_matches(&g, &["ctxrot", "context-rot-guard"]));
        let legacy = json!({"hooks":[{"type":"command","command":"python3 .../context-rot-guard.py"}]});
        assert!(group_matches(&legacy, &["ctxrot", "context-rot-guard"]));
        let other = json!({"hooks":[{"type":"command","command":"prettier --write"}]});
        assert!(!group_matches(&other, &["ctxrot", "context-rot-guard"]));
    }

    #[test]
    fn strip_keeps_foreign_groups() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"ctxrot guard"}]}),
            json!({"hooks":[{"type":"command","command":"my-other-hook"}]}),
        ];
        let kept = strip_matching(&arr, &["ctxrot"]);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn load_settings_tolerates_missing_and_empty() {
        let missing = std::env::temp_dir().join(format!("harness-no-such-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(load_settings(&missing).unwrap(), json!({}));
    }
}
