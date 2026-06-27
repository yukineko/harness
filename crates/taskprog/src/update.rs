//! Stop hook: prompt the LLM to update the progress file by returning
//! additionalContext asking it to write an updated `.claude/progress.md`.

use std::path::Path;

use anyhow::Result;
use harness_core::hook::HookInput;
use serde_json::json;

use crate::config::Config;

/// Run at Stop. If no progress file exists yet, suggest creating one.
/// If it does exist, ask the LLM to keep it current.
pub fn on_stop(input: &HookInput, cfg: &Config) -> Result<()> {
    let cwd = if input.cwd.is_empty() {
        "."
    } else {
        &input.cwd
    };
    let path = cfg.resolve_progress_path(cwd);

    let (verb, current_block) = if path.exists() {
        let content = crate::progress::read_file(&path, 0).unwrap_or_else(|| "(empty)".to_string());
        (
            "update",
            format!(
                "\n\nCurrent progress file (`{}`):\n\n```markdown\n{}\n```",
                path.display(),
                content
            ),
        )
    } else {
        ("create", format!("\n\nTarget path: `{}`", path.display()))
    };

    let msg = format!(
        "Before ending this session, please {verb} the progress file with what was accomplished, \
         what is pending, and any blocking issues. Keep it concise (bullet points). \
         Write it with the Write tool.{current_block}"
    );

    let out = json!({ "additionalContext": msg });
    println!("{out}");
    Ok(())
}

/// Actually write the progress file (used by `taskprog write` command).
pub fn write_progress(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}
