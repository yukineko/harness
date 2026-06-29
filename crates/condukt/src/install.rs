//! Manual install path: merge condukt's hook into `~/.claude/settings.json`.
//!
//! The plugin (hooks/hooks.json) is the recommended distribution; this exists
//! for users who build from source without installing the plugin. Idempotent:
//! it strips any prior condukt entries (matched by the `condukt ` command
//! prefix) before adding the current one, and backs up settings.json first.
//! Settings-file mechanics (load/backup/write/strip) are shared via
//! `harness_core::install`.

use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;

/// (event, matcher, subcommand) — single SessionStart restore hook.
const EVENTS: &[(&str, Option<&str>, &str)] =
    &[("SessionStart", Some("startup|resume|clear"), "restore")];

/// Command substrings condukt owns (the `condukt ` command prefix).
const MARKERS: &[&str] = &["condukt "];

fn settings_path() -> PathBuf {
    harness_core::config::home()
        .join(".claude")
        .join("settings.json")
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
    let mut settings = harness_core::install::load_settings(&path)?;

    if !settings.is_object() {
        settings = json!({});
    }

    for (event, matcher, sub) in EVENTS {
        let mut group = harness_core::install::command_group(&format!("condukt {sub}"), 10);
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        harness_core::install::push_group(&mut settings, MARKERS, event, group)?;
    }

    harness_core::install::write_settings(&path, &settings)
        .with_context(|| format!("writing {}", path.display()))?;
    eprintln!("installed condukt hooks into {}", path.display());
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = settings_path();
    if !path.exists() {
        eprintln!("no settings.json at {}", path.display());
        return Ok(());
    }
    let mut settings = harness_core::install::load_settings(&path)?;
    let events: Vec<&str> = EVENTS.iter().map(|(e, _, _)| *e).collect();
    harness_core::install::remove_hooks_from_settings(&mut settings, MARKERS, &events);
    harness_core::install::write_settings(&path, &settings)
        .with_context(|| format!("writing {}", path.display()))?;
    eprintln!("removed condukt hooks from {}", path.display());
    Ok(())
}
