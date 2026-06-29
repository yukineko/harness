//! Merge/remove the difflog SessionStart + SessionEnd hooks.

use std::path::PathBuf;

use anyhow::Result;

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
        .unwrap_or_else(|| "difflog".to_string())
}

const MARKERS: &[&str] = &["difflog"];

pub fn install(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let bin = binary_path();
    harness_core::install::push_group(
        &mut settings,
        MARKERS,
        "SessionStart",
        harness_core::install::command_group(&format!("{bin} session-start"), 10),
    )?;
    harness_core::install::push_group(
        &mut settings,
        MARKERS,
        "SessionEnd",
        harness_core::install::command_group(&format!("{bin} session-end"), 30),
    )?;
    if dry_run {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("Installed SessionStart + SessionEnd hooks for difflog");
    Ok(())
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let removed = harness_core::install::remove_hooks_from_settings(
        &mut settings,
        MARKERS,
        &["SessionStart", "SessionEnd"],
    );
    if dry_run {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} difflog hook group(s)");
    Ok(())
}
