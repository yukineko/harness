use crate::config::Config;
use crate::goal_link::check_goal_link;
use crate::store::Store;
use harness_core::hook::{read_stdin, HookInput};
use std::path::Path;

/// SessionStart hook entry point. Loads config and stdin itself so main can
/// hand off to `run_hook` without capturing any state.
pub fn run() -> Option<String> {
    if Config::disabled_env() {
        return None;
    }
    let cfg = Config::load().ok()?;
    if !cfg.enabled {
        return None;
    }
    let raw = read_stdin();
    let input = HookInput::parse(&raw).unwrap_or_default();
    let repo_root = input.cwd_or_current();
    run_with(&cfg, &repo_root)
}

/// Testable core: takes an explicit config and repo root.
pub(crate) fn run_with(cfg: &Config, repo_root: &Path) -> Option<String> {
    let store = Store::load(cfg).ok()?;
    let all = store.all();

    let open: Vec<_> = all.iter().filter(|h| h.status.is_open()).collect();
    if open.is_empty() {
        return None;
    }

    let unlinked_ids = check_goal_link(all, repo_root);

    let mut out = String::from("## Hypothesis \u{2014} open hypotheses for this project\n\n");

    for h in &open {
        let link_marker = if unlinked_ids.contains(&h.id) {
            " [unlinked]"
        } else {
            ""
        };
        out.push_str(&format!("- **[{}]**{} {}\n", h.id, link_marker, h.text));
        if let Some(goal) = &h.linked_goal {
            out.push_str(&format!("  linked_goal: {}\n", goal));
        }
    }

    out.push_str("\n---\n\n");
    out.push_str("To validate: `hypothesis validate <id> --evidence \"...\"`\n");
    out.push_str("To reject:   `hypothesis reject <id> --reason \"...\"`\n");

    let unlinked_open = open.iter().filter(|h| unlinked_ids.contains(&h.id)).count();
    if unlinked_open > 0 {
        out.push_str(&format!(
            "\n\u{26a0}\u{fe0f} {} 件の仮説が compass charter とリンクしていません。\
             `hypothesis add ... --goal \"<keyword>\"` でリンクしてください。\n",
            unlinked_open
        ));
    }

    if out.len() > cfg.inject_limit {
        let truncated = truncate_to_byte_boundary(&out, cfg.inject_limit);
        out = format!("{}\n*(truncated)*", truncated);
    }

    Some(out)
}

fn truncate_to_byte_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::store::Store;
    use tempfile::TempDir;

    fn test_cfg(dir: &TempDir) -> Config {
        Config {
            enabled: true,
            store_dir: dir.path().to_path_buf(),
            inject_limit: 2000,
        }
    }

    #[test]
    fn session_hook_no_hypotheses_returns_none() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);
        let result = run_with(&cfg, dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn session_hook_open_hypothesis_appears_in_output() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        st.add("users want faster onboarding".to_string(), None).unwrap();

        let out = run_with(&cfg, dir.path()).expect("should produce output");
        assert!(out.contains("users want faster onboarding"));
        assert!(out.contains("## Hypothesis"));
    }

    #[test]
    fn session_hook_validated_hypothesis_not_shown() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("already proven".to_string(), None).unwrap();
        st.validate(&id, vec!["measured".to_string()], None).unwrap();

        let result = run_with(&cfg, dir.path());
        // All open hypotheses gone → None
        assert!(result.is_none());
    }

    #[test]
    fn session_hook_unlinked_marker_shown_when_no_charter() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        st.add("no charter present".to_string(), None).unwrap();

        // No .compass/charter.md → hypothesis treated as unlinked
        let out = run_with(&cfg, dir.path()).expect("output");
        assert!(out.contains("[unlinked]"));
    }

    #[test]
    fn session_hook_linked_hypothesis_no_unlinked_marker() {
        use std::fs;
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        // Write charter so goal link can match
        let compass = dir.path().join(".compass");
        fs::create_dir_all(&compass).unwrap();
        fs::write(
            compass.join("charter.md"),
            "## north_star\nfaster user onboarding\n\n## definition_of_done\n- all tests pass\n",
        )
        .unwrap();

        let mut st = Store::load(&cfg).unwrap();
        st.add(
            "users want faster onboarding".to_string(),
            Some("faster user onboarding".to_string()),
        )
        .unwrap();

        let out = run_with(&cfg, dir.path()).expect("output");
        assert!(!out.contains("[unlinked]"));
    }

    #[test]
    fn session_hook_truncates_at_inject_limit() {
        let dir = TempDir::new().unwrap();
        let mut cfg = test_cfg(&dir);
        cfg.inject_limit = 60;

        let mut st = Store::load(&cfg).unwrap();
        st.add(
            "a".repeat(200),
            None,
        )
        .unwrap();

        let out = run_with(&cfg, dir.path()).expect("output");
        assert!(out.contains("*(truncated)*"));
    }
}
