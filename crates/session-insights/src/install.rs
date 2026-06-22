//! Merge/remove the session-insights hooks (PostToolUse + Stop) in
//! `~/.claude/settings.json`. Idempotent; backs up before any write; preserves
//! foreign hook groups. Standalone `cargo install` path; the plugin path uses
//! `hooks/hooks.json`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const MATCHER: &str = "Bash|Edit|MultiEdit|Write|Read|Grep|Glob|WebFetch|WebSearch|NotebookEdit";
const TIMEOUT_SECS: u64 = 10;

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

fn binary_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "session-insights".to_string())
}

/// Command substring session-insights owns: its own binary. Settings-file
/// mechanics (load/backup/write/strip) are shared via `harness_core::install`.
const MARKERS: &[&str] = &["session-insights"];

/// True if a hook *group* contains any command we own and should replace.
/// Only exercised by tests now; the install/uninstall paths go through
/// `strip_ours`.
#[cfg(test)]
fn is_ours(group: &Value) -> bool {
    harness_core::install::group_matches(group, MARKERS)
}

/// Strip all session-insights groups from an event array; returns the cleaned array.
fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
}

fn upsert(hooks: &mut serde_json::Map<String, Value>, event: &str, group: Value) {
    let existing = hooks
        .get(event)
        .and_then(Value::as_array)
        .map(|a| strip_ours(a))
        .unwrap_or_default();
    let mut arr = existing;
    arr.push(group);
    hooks.insert(event.to_string(), Value::Array(arr));
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    let bin = binary_path();
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let root = settings.as_object_mut().unwrap();
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("hooks is not an object")?;

    upsert(
        hooks,
        "PostToolUse",
        json!({
            "matcher": MATCHER,
            "hooks": [ { "type": "command", "command": format!("{bin} record"), "timeout": TIMEOUT_SECS } ]
        }),
    );
    upsert(
        hooks,
        "Stop",
        json!({
            "hooks": [ { "type": "command", "command": format!("{bin} stop"), "timeout": TIMEOUT_SECS } ]
        }),
    );

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("\nInstalled PostToolUse + Stop hooks → {bin} record / stop");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let root = settings.as_object_mut().unwrap();
    let mut removed = 0;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for event in ["PostToolUse", "Stop"] {
            if let Some(arr) = hooks.get(event).and_then(Value::as_array) {
                let before = arr.len();
                let cleaned = strip_ours(arr);
                removed += before - cleaned.len();
                if cleaned.is_empty() {
                    hooks.remove(event);
                } else {
                    hooks.insert(event.to_string(), Value::Array(cleaned));
                }
            }
        }
    }

    if dry_run {
        println!("--- dry run (would remove {removed} session-insights group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} session-insights hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/session-insights record"}]});
        assert!(is_ours(&g));
        let other = json!({"hooks":[{"type":"command","command":"stuckguard watch"}]});
        assert!(!is_ours(&other));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"session-insights record"}]}),
            json!({"hooks":[{"type":"command","command":"stuckguard watch"}]}),
        ];
        assert_eq!(strip_ours(&arr).len(), 1);
    }
}
