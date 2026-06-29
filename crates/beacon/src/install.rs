//! Merge/remove the beacon Stop and Notification hooks in
//! `~/.claude/settings.json`. Idempotent; backs up before any write; preserves
//! foreign hook groups. This is the standalone `cargo install` path — the
//! plugin path uses `hooks/hooks.json` instead.

use std::path::PathBuf;

use anyhow::Result;
#[cfg(test)]
use serde_json::{json, Value};

use harness_core::install::{
    command_group, load_settings, push_group, remove_hooks_from_settings, write_settings,
};

const EVENTS: [(&str, &str); 2] = [("Stop", "notify"), ("Notification", "notify")];
const TIMEOUT_SECS: u64 = 10;

/// Command-substring markers identifying hook groups this plugin owns.
const MARKERS: [&str; 1] = ["beacon"];

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
        .unwrap_or_else(|| "beacon".to_string())
}

#[cfg(test)]
fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, &MARKERS)
}

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = load_settings(&settings_path())?;
    let bin = binary_path();
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    for (event, sub) in EVENTS {
        push_group(
            &mut settings,
            &MARKERS,
            event,
            command_group(&format!("{bin} {sub}"), TIMEOUT_SECS),
        )?;
    }

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings_path(), &settings)?;
    println!("\nInstalled Stop + Notification hooks → {bin} notify");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let events: Vec<&str> = EVENTS.iter().map(|(e, _)| *e).collect();
    let removed = remove_hooks_from_settings(&mut settings, &MARKERS, &events);

    if dry_run {
        println!("--- dry run (would remove {removed} beacon group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    write_settings(&settings_path(), &settings)?;
    println!("removed {removed} beacon hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::install::group_matches;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/beacon notify"}]});
        assert!(group_matches(&g, &MARKERS));
        let other = json!({"hooks":[{"type":"command","command":"stuckguard watch"}]});
        assert!(!group_matches(&other, &MARKERS));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"beacon notify"}]}),
            json!({"hooks":[{"type":"command","command":"stuckguard watch"}]}),
        ];
        assert_eq!(strip_ours(&arr).len(), 1);
    }
}
