//! Manual install path: merge condukt's hook into `~/.claude/settings.json`.
//!
//! The plugin (hooks/hooks.json) is the recommended distribution; this exists
//! for users who build from source without installing the plugin. Idempotent:
//! it strips any prior condukt entries (matched by the `condukt ` command
//! prefix) before adding the current one, and backs up settings.json first.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

/// (event, matcher, subcommand) — single SessionStart restore hook.
const EVENTS: &[(&str, Option<&str>, &str)] =
    &[("SessionStart", Some("startup|resume|clear"), "restore")];

const CMD_PREFIX: &str = "condukt ";

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

fn is_condukt_entry(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.contains(CMD_PREFIX))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Remove every condukt-owned hook group from each event array.
fn strip(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };
    for (_event, groups) in hooks.iter_mut() {
        if let Some(arr) = groups.as_array_mut() {
            arr.retain(|g| !is_condukt_entry(g));
        }
    }
}

fn load(path: &PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// Print what install would do without writing.
pub fn dry_run() -> Result<()> {
    let path = settings_path();
    eprintln!("would update {}", path.display());
    for (event, matcher, sub) in EVENTS {
        eprintln!("  + {event} ({}) -> condukt {sub}", matcher.unwrap_or("*"));
    }
    Ok(())
}

pub fn install() -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut settings = load(&path);

    if path.exists() {
        let backup = path.with_extension("json.condukt-bak");
        std::fs::copy(&path, &backup)
            .with_context(|| format!("backing up to {}", backup.display()))?;
    }

    strip(&mut settings);

    if !settings.is_object() {
        settings = json!({});
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));

    for (event, matcher, sub) in EVENTS {
        let mut group = json!({
            "hooks": [{ "type": "command", "command": format!("condukt {sub}"), "timeout": 10 }]
        });
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        let arr = hooks
            .as_object_mut()
            .unwrap()
            .entry(*event)
            .or_insert_with(|| json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(group);
        }
    }

    let out = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&path, out).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("installed condukt hooks into {}", path.display());
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = settings_path();
    if !path.exists() {
        eprintln!("no settings.json at {}", path.display());
        return Ok(());
    }
    let mut settings = load(&path);
    strip(&mut settings);
    let out = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&path, out).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("removed condukt hooks from {}", path.display());
    Ok(())
}
