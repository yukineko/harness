//! Merge/remove the budgetguard Stop hook in `~/.claude/settings.json`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const EVENT: &str = "Stop";
const SUB: &str = "gate";
const TIMEOUT_SECS: u64 = 30;

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
        .unwrap_or_else(|| "budgetguard".to_string())
}

const MARKERS: &[&str] = &["budgetguard"];

fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let root = settings.as_object_mut().unwrap();
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("hooks is not an object")?;

    let existing = hooks
        .get(EVENT)
        .and_then(Value::as_array)
        .map(|a| strip_ours(a))
        .unwrap_or_default();
    let bin = binary_path();
    let mut arr = existing;
    arr.push(json!({
        "hooks": [{ "type": "command", "command": format!("{bin} {SUB}"), "timeout": TIMEOUT_SECS }]
    }));
    hooks.insert(EVENT.to_string(), Value::Array(arr));

    if dry_run {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("Installed Stop hook → {bin} {SUB}");
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
        if let Some(arr) = hooks.get(EVENT).and_then(Value::as_array) {
            let before = arr.len();
            let cleaned = strip_ours(arr);
            removed += before - cleaned.len();
            if cleaned.is_empty() {
                hooks.remove(EVENT);
            } else {
                hooks.insert(EVENT.to_string(), Value::Array(cleaned));
            }
        }
    }
    if dry_run {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} budgetguard hook group(s)");
    Ok(())
}
