//! Merge/remove the stuckguard PostToolUse hook in `~/.claude/settings.json`.
//! Idempotent; backs up before any write.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const EVENT: &str = "PostToolUse";
const SUB: &str = "watch";
const MATCHER: &str = "Bash|Edit|MultiEdit|Write|Read|Grep|Glob";
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
        .unwrap_or_else(|| "stuckguard".to_string())
}

fn is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains("stuckguard"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn load_settings() -> Result<Value> {
    let path = settings_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn backup(path: &PathBuf) -> Result<()> {
    if path.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let bak = path.with_extension(format!("json.bak-{stamp}"));
        std::fs::copy(path, &bak)?;
        println!("backup: {}", bak.display());
    }
    Ok(())
}

fn write_settings(value: &Value) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    backup(&path)?;
    let text = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(&path, text)?;
    println!("updated: {}", path.display());
    Ok(())
}

fn strip_ours(arr: &[Value]) -> Vec<Value> {
    arr.iter().filter(|g| !is_ours(g)).cloned().collect()
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = load_settings()?;
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

    let existing = hooks
        .get(EVENT)
        .and_then(Value::as_array)
        .map(|a| strip_ours(a))
        .unwrap_or_default();
    let mut arr = existing;
    arr.push(json!({
        "matcher": MATCHER,
        "hooks": [ { "type": "command", "command": format!("{bin} {SUB}"), "timeout": TIMEOUT_SECS } ]
    }));
    hooks.insert(EVENT.to_string(), Value::Array(arr));

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings)?;
    println!("\nInstalled PostToolUse hook → {bin} {SUB}  (matcher: {MATCHER})");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = load_settings()?;
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
        println!("--- dry run (would remove {removed} stuckguard group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings)?;
    println!("removed {removed} stuckguard hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/stuckguard watch"}]});
        assert!(is_ours(&g));
        let other = json!({"hooks":[{"type":"command","command":"ctxrot toolguard"}]});
        assert!(!is_ours(&other));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"stuckguard watch"}]}),
            json!({"hooks":[{"type":"command","command":"ctxrot toolguard"}]}),
        ];
        assert_eq!(strip_ours(&arr).len(), 1);
    }
}
