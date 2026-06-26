use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

fn settings_path() -> PathBuf {
    harness_core::config::home()
        .join(".claude")
        .join("settings.json")
}

fn binary_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "hypothesis".to_string())
}

const MARKERS: &[&str] = &["hypothesis"];

fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
}

fn add_hook(settings: &mut Value, event: &str, sub: &str, timeout: u64) -> Result<()> {
    let bin = binary_path();
    let root = settings.as_object_mut().unwrap();
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("hooks is not an object")?;
    let existing = hooks
        .get(event)
        .and_then(Value::as_array)
        .map(|a| strip_ours(a))
        .unwrap_or_default();
    let mut arr = existing;
    arr.push(json!({
        "hooks": [{
            "type": "command",
            "command": format!("{bin} {sub}"),
            "timeout": timeout
        }]
    }));
    hooks.insert(event.to_string(), Value::Array(arr));
    Ok(())
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    add_hook(&mut settings, "SessionStart", "session-start", 5)?;
    if dry_run {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("Installed SessionStart hook for hypothesis");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let root = settings.as_object_mut().unwrap();
    let mut removed = 0usize;
    for event in &["SessionStart"] {
        if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
            if let Some(arr) = hooks.get(*event).and_then(Value::as_array) {
                let before = arr.len();
                let cleaned = strip_ours(arr);
                removed += before - cleaned.len();
                if cleaned.is_empty() {
                    hooks.remove(*event);
                } else {
                    hooks.insert(event.to_string(), Value::Array(cleaned));
                }
            }
        }
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} hypothesis hook group(s)");
    Ok(())
}
