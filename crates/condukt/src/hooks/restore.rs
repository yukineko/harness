//! SessionStart hook: if this project has an in-progress condukt run or orphan
//! worktrees, remind the agent at the top of the session (stdout is injected as
//! additional context). Silent when there is nothing to resume.

use crate::config::Config;
use crate::state;
use crate::store::repo_root;
use crate::worktree;
use std::path::PathBuf;

pub fn run(cwd: &str) {
    let cfg = Config::load();
    let cwd_path = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(cwd)
    };

    let runs = state::open_runs(&cfg, &cwd_path);
    let repo = repo_root(&cwd_path);
    let orphans = worktree::orphans(&repo, &cfg.worktree_base).unwrap_or_default();

    if runs.is_empty() && orphans.is_empty() {
        return;
    }

    let mut lines = vec![String::from(
        "[condukt] Unfinished orchestration state for this project:",
    )];
    for r in &runs {
        let (done, total) = r.counts();
        lines.push(format!(
            "  - run '{}' ({}): {done}/{total} tasks verified",
            r.run_id, r.goal
        ));
    }
    for o in &orphans {
        lines.push(format!("  - orphan worktree on disk: {}", o.display()));
    }
    lines.push(String::from(
        "Resume with `/condukt` (it reads the open run) or clean up via \
         `condukt worktree cleanup` and `condukt state show --run <id>`.",
    ));
    println!("{}", lines.join("\n"));
}
