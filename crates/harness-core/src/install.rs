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
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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
    arr.iter()
        .filter(|g| !group_matches(g, markers))
        .cloned()
        .collect()
}

/// A single-command hook group: `{"hooks":[{"type":"command","command":…,"timeout":…}]}`.
/// Callers needing a `matcher` (PostToolUse/PreToolUse) build the group inline
/// and pass it to [`push_group`].
pub fn command_group(command: &str, timeout: u64) -> Value {
    json!({ "hooks": [{ "type": "command", "command": command, "timeout": timeout }] })
}

/// Append a fully-formed hook `group` under `event`, after removing any existing
/// groups this plugin owns (matched by `markers`). This centralises the per-crate
/// `add_hook` fork — and replaces its `settings.as_object_mut().unwrap()` with a
/// typed error, so a non-object settings.json can never panic an install.
pub fn push_group(settings: &mut Value, markers: &[&str], event: &str, group: Value) -> Result<()> {
    let root = settings
        .as_object_mut()
        .context("settings.json is not a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("settings.hooks is not an object")?;
    let mut arr = hooks
        .get(event)
        .and_then(Value::as_array)
        .map(|a| strip_matching(a, markers))
        .unwrap_or_default();
    arr.push(group);
    hooks.insert(event.to_string(), Value::Array(arr));
    Ok(())
}

/// Remove every group this plugin owns (matched by `markers`) across `events`,
/// pruning now-empty event keys. Returns the number of groups removed. Replaces
/// the per-crate uninstall loop and its `as_object_mut().unwrap()` (a non-object
/// settings simply yields 0 removed).
pub fn remove_hooks_from_settings(
    settings: &mut Value,
    markers: &[&str],
    events: &[&str],
) -> usize {
    let Some(root) = settings.as_object_mut() else {
        return 0;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0usize;
    for event in events {
        if let Some(arr) = hooks.get(*event).and_then(Value::as_array) {
            let before = arr.len();
            let cleaned = strip_matching(arr, markers);
            removed += before - cleaned.len();
            if cleaned.is_empty() {
                hooks.remove(*event);
            } else {
                hooks.insert((*event).to_string(), Value::Array(cleaned));
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_matches_any_marker() {
        let g = json!({"hooks":[{"type":"command","command":"/x/ctxrot guard"}]});
        assert!(group_matches(&g, &["ctxrot", "context-rot-guard"]));
        let legacy =
            json!({"hooks":[{"type":"command","command":"python3 .../context-rot-guard.py"}]});
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
    fn push_group_replaces_own_and_keeps_foreign() {
        let mut s = json!({
            "hooks": {
                "SessionStart": [
                    {"hooks":[{"type":"command","command":"/old/backlog session-start"}]},
                    {"hooks":[{"type":"command","command":"prettier --write"}]}
                ]
            }
        });
        push_group(
            &mut s,
            &["backlog"],
            "SessionStart",
            command_group("/new/backlog session-start", 5),
        )
        .unwrap();
        let arr = s["hooks"]["SessionStart"].as_array().unwrap();
        // Foreign group kept, old ours stripped, new ours appended → 2 total.
        assert_eq!(arr.len(), 2);
        assert!(arr
            .iter()
            .any(|g| g["hooks"][0]["command"] == "prettier --write"));
        assert!(arr
            .iter()
            .any(|g| g["hooks"][0]["command"] == "/new/backlog session-start"));
    }

    #[test]
    fn push_group_supports_matcher_and_non_object_errors() {
        let mut s = json!({});
        push_group(
            &mut s,
            &["session-insights"],
            "PostToolUse",
            json!({"matcher":"Bash|Edit","hooks":[{"type":"command","command":"/x/session-insights record","timeout":10}]}),
        )
        .unwrap();
        assert_eq!(s["hooks"]["PostToolUse"][0]["matcher"], "Bash|Edit");
        // Non-object settings is a typed error, never a panic.
        let mut bad = json!("not an object");
        assert!(push_group(&mut bad, &["x"], "Stop", command_group("x", 1)).is_err());
    }

    #[test]
    fn remove_hooks_round_trips_with_push() {
        let mut s = json!({});
        push_group(&mut s, &["tdd"], "Stop", command_group("/x/tdd gate", 10)).unwrap();
        push_group(&mut s, &["tdd"], "Stop", command_group("/x/tdd gate", 10)).unwrap(); // re-install must not duplicate (strips prior ours)
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let removed = remove_hooks_from_settings(&mut s, &["tdd"], &["Stop"]);
        assert_eq!(removed, 1);
        // Empty event key pruned.
        assert!(s["hooks"].get("Stop").is_none());
        // Non-object settings → 0 removed, no panic.
        let mut bad = json!(42);
        assert_eq!(remove_hooks_from_settings(&mut bad, &["tdd"], &["Stop"]), 0);
    }

    #[test]
    fn load_settings_tolerates_missing_and_empty() {
        let missing =
            std::env::temp_dir().join(format!("harness-no-such-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(load_settings(&missing).unwrap(), json!({}));
    }
}
