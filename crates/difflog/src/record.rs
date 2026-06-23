//! SessionStart + SessionEnd hooks.
//!
//! SessionStart: record the HEAD SHA at session start.
//! SessionEnd:   generate the diff-log markdown and write it to `log_dir`.

use std::path::Path;

use harness_core::hook::HookInput;

use crate::config::Config;
use crate::git;
use crate::state::{self, SessionState};

/// SessionStart: snapshot HEAD so we know where the session began.
pub fn on_session_start(input: &HookInput, cfg: &Config) {
    let cwd = input.cwd_or_current();
    let Some(sha) = git::head_sha(&cwd) else { return };

    let started_at = {
        use chrono::Utc;
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    };

    let st = SessionState {
        session_id: input.session_id.clone(),
        start_sha: sha,
        project: project_name(&cwd),
        started_at,
    };
    let _ = state::save(&cfg.log_dir, &st);
}

/// SessionEnd: generate and write the diff-log.
pub fn on_session_end(input: &HookInput, cfg: &Config) {
    let cwd = input.cwd_or_current();
    let Some(st) = state::load(&cfg.log_dir, &input.session_id) else { return };

    let stat = git::diff_stat(&cwd, &st.start_sha);
    if stat.trim().is_empty() {
        // Nothing changed — skip writing a log.
        return;
    }

    let name_status = git::diff_name_status(&cwd, &st.start_sha);
    let commits = git::log_oneline(&cwd, &st.start_sha);
    let body = git::diff_body(&cwd, &st.start_sha, cfg.diff_body_limit);

    let head_sha = git::head_sha(&cwd).unwrap_or_else(|| "HEAD".into());
    let ended_at = {
        use chrono::Utc;
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    };
    let date = &ended_at[..10];

    let markdown = render_log(DiffLogCtx {
        session_id: &input.session_id,
        project: &st.project,
        started_at: &st.started_at,
        ended_at: &ended_at,
        start_sha: &st.start_sha,
        head_sha: &head_sha,
        stat: &stat,
        name_status: &name_status,
        commits: &commits,
        diff_body: &body,
    });

    let _ = write_log(&cfg.log_dir, date, &input.session_id, &markdown);
}

struct DiffLogCtx<'a> {
    session_id: &'a str,
    project: &'a str,
    started_at: &'a str,
    ended_at: &'a str,
    start_sha: &'a str,
    head_sha: &'a str,
    stat: &'a str,
    name_status: &'a [(char, String)],
    commits: &'a str,
    diff_body: &'a str,
}

fn render_log(ctx: DiffLogCtx) -> String {
    let files_added: Vec<_> = ctx.name_status.iter()
        .filter(|(s, _)| *s == 'A').map(|(_, p)| p.as_str()).collect();
    let files_modified: Vec<_> = ctx.name_status.iter()
        .filter(|(s, _)| *s == 'M').map(|(_, p)| p.as_str()).collect();
    let files_deleted: Vec<_> = ctx.name_status.iter()
        .filter(|(s, _)| *s == 'D').map(|(_, p)| p.as_str()).collect();

    let mut out = String::new();
    out.push_str(&format!("# difflog — {}\n\n", ctx.project));
    out.push_str(&format!("- **session**: `{}`\n", ctx.session_id));
    out.push_str(&format!("- **started**: {}\n", ctx.started_at));
    out.push_str(&format!("- **ended**:   {}\n", ctx.ended_at));
    out.push_str(&format!("- **range**:   `{}..{}`\n\n", &ctx.start_sha[..8.min(ctx.start_sha.len())], &ctx.head_sha[..8.min(ctx.head_sha.len())]));

    if !ctx.commits.trim().is_empty() {
        out.push_str("## Commits\n\n```\n");
        out.push_str(ctx.commits.trim_end());
        out.push_str("\n```\n\n");
    }

    out.push_str("## Files changed\n\n");
    if !files_added.is_empty() {
        out.push_str(&format!("**Added** ({})\n", files_added.len()));
        for f in &files_added { out.push_str(&format!("- `{f}`\n")); }
        out.push('\n');
    }
    if !files_modified.is_empty() {
        out.push_str(&format!("**Modified** ({})\n", files_modified.len()));
        for f in &files_modified { out.push_str(&format!("- `{f}`\n")); }
        out.push('\n');
    }
    if !files_deleted.is_empty() {
        out.push_str(&format!("**Deleted** ({})\n", files_deleted.len()));
        for f in &files_deleted { out.push_str(&format!("- `{f}`\n")); }
        out.push('\n');
    }

    out.push_str("## Stat\n\n```\n");
    out.push_str(ctx.stat.trim_end());
    out.push_str("\n```\n");

    if !ctx.diff_body.is_empty() {
        out.push_str("\n## Diff\n\n```diff\n");
        out.push_str(ctx.diff_body.trim_end());
        out.push_str("\n```\n");
    }

    out
}

fn write_log(log_dir: &Path, date: &str, session_id: &str, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let safe_id: String = session_id.chars()
        .take(8)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let name = format!("{date}-{safe_id}.md");
    std::fs::write(log_dir.join(&name), content)?;
    eprintln!("difflog: wrote {name}");
    Ok(())
}

fn project_name(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string()
}
