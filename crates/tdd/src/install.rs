//! Merge/remove the tdd Stop hook in `~/.claude/settings.json`.
//!
//! Idempotent: existing tdd groups are stripped before re-adding, so running
//! install twice is safe. Settings-file mechanics are shared via
//! `harness_core::install`. (Installing via the plugin marketplace wires the
//! same hook through `hooks/hooks.json`; this is for non-plugin use.)

use std::path::PathBuf;

use anyhow::Result;
#[cfg(test)]
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
        .unwrap_or_else(|| "tdd".to_string())
}

const MARKERS: &[&str] = &["tdd gate", "/tdd "];

#[cfg(test)]
fn is_ours(group: &Value) -> bool {
    harness_core::install::group_matches(group, MARKERS)
}

#[cfg(test)]
fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    let bin = binary_path();
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    harness_core::install::push_group(
        &mut settings,
        MARKERS,
        EVENT,
        harness_core::install::command_group(&format!("{bin} {SUB}"), TIMEOUT_SECS),
    )?;

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("\nInstalled Stop hook → {bin} {SUB}");
    println!("Tune ./tdd.toml (or ~/.tdd/config.toml) if the language defaults don't fit.");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let removed =
        harness_core::install::remove_hooks_from_settings(&mut settings, MARKERS, &[EVENT]);

    if dry_run {
        println!("--- dry run (would remove {removed} tdd group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} tdd hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/tdd gate"}]});
        assert!(is_ours(&g));
        let other = json!({"hooks":[{"type":"command","command":"prettier --write"}]});
        assert!(!is_ours(&other));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"tdd gate"}]}),
            json!({"hooks":[{"type":"command","command":"my-other-hook"}]}),
        ];
        assert_eq!(strip_ours(&arr).len(), 1);
    }
}
