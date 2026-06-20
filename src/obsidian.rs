//! Optional: write a dated session note into an Obsidian vault. Opt-in
//! (`obsidian_log = true`) and only if the vault directory already exists — we
//! never create the vault. The note is overwritten on each Stop, so it always
//! reflects the latest rollup for that session.

use std::path::PathBuf;

use crate::config::Config;
use crate::metrics::{short, Session};

/// Returns the written path on success, or None if skipped/failed.
pub fn write_note(cfg: &Config, s: &Session) -> Option<PathBuf> {
    if !cfg.obsidian_log || !cfg.obsidian_vault.is_dir() {
        return None;
    }
    let date = if s.started_at.len() >= 10 {
        &s.started_at[..10]
    } else {
        return None;
    };
    let dir = cfg.obsidian_vault.join("sessions");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{date}-{}.md", short(&s.session_id)));

    let files_block = if s.files.is_empty() {
        "_(none)_".to_string()
    } else {
        s.files
            .iter()
            .take(30)
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tools_block = s
        .top_tools(10)
        .iter()
        .map(|(t, c)| format!("- {t}: {c}"))
        .collect::<Vec<_>>()
        .join("\n");

    let body = format!(
        "---\ntype: session\ndate: {date}\nproject: {}\nsize: {}\ncategory: {}\nturns: {}\ntool_events: {}\n---\n\n\
         # session {} — {}\n\n\
         - started: {}\n- last: {}\n- size: **{}**   category: **{}**\n- turns: {}   tool events: {}   files: {}\n\n\
         ## tools\n{}\n\n## files touched\n{}\n",
        s.project,
        s.size(&cfg.size_thresholds),
        s.category(),
        s.turns,
        s.tool_events,
        short(&s.session_id),
        s.project,
        s.started_at,
        s.last_at,
        s.size(&cfg.size_thresholds),
        s.category(),
        s.turns,
        s.tool_events,
        s.files.len(),
        if tools_block.is_empty() { "_(none)_".to_string() } else { tools_block },
        files_block,
    );
    std::fs::write(&path, body).ok()?;
    Some(path)
}
