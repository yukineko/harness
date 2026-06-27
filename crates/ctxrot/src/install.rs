//! Merge/remove ctxrot hooks in `~/.claude/settings.json`.
//!
//! Idempotent: existing ctxrot (and legacy `context-rot-guard.py`) entries are
//! stripped before re-adding, so running install twice is safe and it cleanly
//! replaces the Python v1. The file is backed up before any write.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// (hook event, ctxrot subcommand, optional matcher)
const EVENTS: &[(&str, &str, Option<&str>)] = &[
    ("UserPromptSubmit", "guard", None),
    ("PreCompact", "rescue", None),
    ("SessionStart", "restore", Some("startup|resume|clear")),
    ("PreToolUse", "preguard", Some("Read|Bash")),
    (
        "PostToolUse",
        "toolguard",
        Some("Read|Bash|Grep|Glob|WebFetch|BashOutput|NotebookRead"),
    ),
];

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
        .unwrap_or_else(|| "ctxrot".to_string())
}

/// Command substrings ctxrot owns: its own binary, and the legacy python guard
/// it replaces. Settings-file mechanics (load/backup/write/strip) are shared via
/// `harness_core::install`.
const MARKERS: &[&str] = &["ctxrot", "context-rot-guard"];

/// True if a hook *group* contains any command we own and should replace.
/// Only exercised by tests now; the install/uninstall paths go through
/// `strip_ours`.
#[cfg(test)]
fn is_ours(group: &Value) -> bool {
    harness_core::install::group_matches(group, MARKERS)
}

/// Strip all ctxrot/legacy groups from an event array; returns the cleaned array.
fn strip_ours(arr: &[Value]) -> Vec<Value> {
    harness_core::install::strip_matching(arr, MARKERS)
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

    for (event, sub, matcher) in EVENTS {
        let existing = hooks
            .get(*event)
            .and_then(Value::as_array)
            .map(|a| strip_ours(a))
            .unwrap_or_default();

        let mut arr = existing;
        let mut group = json!({
            "hooks": [ { "type": "command", "command": format!("{bin} {sub}"), "timeout": 10 } ]
        });
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        arr.push(group);
        hooks.insert((*event).to_string(), Value::Array(arr));
    }

    // statusLine: a live context-usage meter. Set it only when there is no
    // status line yet or the existing one is ours — never clobber a custom bar.
    if is_our_statusline(root.get("statusLine")) {
        root.insert(
            "statusLine".to_string(),
            json!({ "type": "command", "command": format!("{bin} statusline"), "padding": 0 }),
        );
    }

    if dry_run {
        println!("--- dry run (settings.json would become) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("\nInstalled hooks → {bin}");
    println!("  UserPromptSubmit=guard  PreCompact=rescue  SessionStart=restore");
    println!("  PreToolUse=preguard  PostToolUse=toolguard");
    println!("  statusLine=statusline (context-usage meter)");
    Ok(())
}

/// True if there is no status line yet, or the existing one is ctxrot's (so it
/// is safe to (re)install). A foreign custom status line returns false.
fn is_our_statusline(sl: Option<&Value>) -> bool {
    match sl {
        None => true,
        Some(v) => v
            .get("command")
            .and_then(Value::as_str)
            .map(|c| c.contains("ctxrot statusline"))
            .unwrap_or(false),
    }
}

pub fn uninstall(dry_run: bool) -> Result<()> {
    let mut settings = harness_core::install::load_settings(&settings_path())?;
    if !settings.is_object() {
        anyhow::bail!("settings.json is not a JSON object");
    }
    let root = settings.as_object_mut().unwrap();
    let mut removed = 0;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for (event, _, _) in EVENTS {
            if let Some(arr) = hooks.get(*event).and_then(Value::as_array) {
                let before = arr.len();
                let cleaned = strip_ours(arr);
                removed += before - cleaned.len();
                if cleaned.is_empty() {
                    hooks.remove(*event);
                } else {
                    hooks.insert((*event).to_string(), Value::Array(cleaned));
                }
            }
        }
    }
    // Drop our statusLine too (but leave a user's custom one untouched).
    if root.get("statusLine").is_some() && is_our_statusline(root.get("statusLine")) {
        root.remove("statusLine");
        removed += 1;
    }

    if dry_run {
        println!("--- dry run (would remove {removed} ctxrot group(s)) ---");
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    harness_core::install::write_settings(&settings_path(), &settings)?;
    println!("removed {removed} ctxrot hook group(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ours() {
        let g = json!({"hooks":[{"type":"command","command":"/x/ctxrot guard"}]});
        assert!(is_ours(&g));
        let legacy =
            json!({"hooks":[{"type":"command","command":"python3 .../context-rot-guard.py"}]});
        assert!(is_ours(&legacy));
        let other = json!({"hooks":[{"type":"command","command":"prettier --write"}]});
        assert!(!is_ours(&other));
    }

    #[test]
    fn strip_keeps_foreign() {
        let arr = vec![
            json!({"hooks":[{"type":"command","command":"ctxrot guard"}]}),
            json!({"hooks":[{"type":"command","command":"my-other-hook"}]}),
        ];
        let kept = strip_ours(&arr);
        assert_eq!(kept.len(), 1);
    }
}
