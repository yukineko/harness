//! Merge/remove the gauge Stop hook in `~/.claude/settings.json`. Idempotent;
//! backs up before any write; preserves foreign hook groups. This is the
//! standalone `cargo install` path — the plugin path uses `hooks/hooks.json`.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;

const EVENTS: [(&str, &str); 1] = [("Stop", "record")];
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
        .unwrap_or_else(|| "gauge".to_string())
}

/// Command substrings gauge owns. Settings-file mechanics
/// (load/backup/write/strip) are shared via `harness_core::install`.
const MARKERS: &[&str] = &["gauge"];

#[cfg(test)]
fn is_ours(group: &Value) -> bool {
    harness_core::install::group_matches(group, MARKERS)
}

fn load_settings() -> Result<Value> {
    harness_core::install::load_settings(&settings_path())
}

fn write_settings(value: &Value) -> Result<()> {
    harness_core::install::write_settings(&settings_path(), value)
}

#[cfg(test)]
fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = load_settings()?;
    let bin = binary_path();
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    for (event, sub) in EVENTS {
        harness_core::install::push_group(
            &mut settings,
            MARKERS,
            event,
            harness_core::install::command_group(&format!("{bin} {sub}"), TIMEOUT_SECS),
        )?;
    }

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings)?;
    println!("\nInstalled Stop hook → {bin} record");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = load_settings()?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let events: Vec<&str> = EVENTS.iter().map(|(e, _)| *e).collect();
    let removed =
        harness_core::install::remove_hooks_from_settings(&mut settings, MARKERS, &events);

    if dry_run {
        println!("--- dry run (would remove {removed} gauge group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings)?;
    println!("removed {removed} gauge hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/gauge record"}]});
        assert!(is_ours(&g));
        let other = json!({"hooks":[{"type":"command","command":"beacon notify"}]});
        assert!(!is_ours(&other));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"gauge record"}]}),
            json!({"hooks":[{"type":"command","command":"beacon notify"}]}),
        ];
        assert_eq!(strip_ours(&arr).len(), 1);
    }
}
