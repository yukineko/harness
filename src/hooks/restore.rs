//! `ctxrot restore` — SessionStart hook.
//!
//! At session start, find the most recent rescue/distill note for this project
//! and inject a COMPACT carryover (decisions + open todos + a link), so the prior
//! session's conclusions survive without re-bloating context. We never inject the
//! whole note — just the durable signal plus a pointer to read more on demand.

use std::path::Path;

use crate::config::Config;
use crate::model::HookInput;
use crate::store::Store;

const READ_CAP: u64 = 256 * 1024;
const SECTION_CAP_CHARS: usize = 1500;

/// Returns the additionalContext text to inject, or None if there is no note.
pub fn run(input: &HookInput, cfg: &Config) -> Option<String> {
    // Don't re-inject right after a compaction restart — restore is for a fresh
    // session picking up prior work, not for the compact handoff.
    if input.source == "compact" {
        return None;
    }

    let cwd = input.cwd_or_current();
    let store = Store::new(cfg);
    let latest = store.latest_note(&cwd)?;

    let meta = std::fs::metadata(&latest).ok()?;
    if meta.len() > READ_CAP {
        // Unexpectedly large note: just point at it, don't inline.
        return Some(format!(
            "[ctxrot restore] 前回の退避ノートあり: {}\n→ 必要なら読み込んで続きから作業を。",
            latest.display()
        ));
    }
    let text = std::fs::read_to_string(&latest).ok()?;

    let decisions = extract_section(&text, &["決定事項", "Decisions"]);
    let todos = extract_section(&text, &["残課題", "Open todos", "todos"]);

    let mut out = String::new();
    out.push_str("[ctxrot restore] 前回セッションからの引き継ぎ（要約）:\n");
    if let Some(d) = &decisions {
        out.push_str("\n■ 決定事項:\n");
        out.push_str(d);
        out.push('\n');
    }
    if let Some(t) = &todos {
        out.push_str("\n■ 残課題:\n");
        out.push_str(t);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n→ 全文: {}\n（必要時のみ読む。本文には貼らず要約＋リンク運用を維持）",
        latest.display()
    ));

    // If both sections were empty/missing, only the pointer is useful.
    if decisions.is_none() && todos.is_none() {
        return Some(format!(
            "[ctxrot restore] 前回の退避ノートあり: {}\n→ 続きから作業する場合は読み込んで。",
            latest.display()
        ));
    }
    Some(out)
}

/// Pull the body under a `## <title>` heading (any of the aliases), bounded.
/// Skips the "_(なし / none)_" placeholder.
fn extract_section(text: &str, titles: &[&str]) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if let Some(rest) = line.strip_prefix("## ") {
            if titles.iter().any(|t| rest.contains(t)) {
                let mut body = String::new();
                i += 1;
                while i < lines.len() && !lines[i].trim_start().starts_with("## ") {
                    body.push_str(lines[i]);
                    body.push('\n');
                    i += 1;
                }
                let body = body.trim();
                if body.is_empty() || body.contains("_(なし") {
                    return None;
                }
                return Some(clip_section(body));
            }
        }
        i += 1;
    }
    None
}

fn clip_section(s: &str) -> String {
    crate::transcript::truncate_chars(s, SECTION_CAP_CHARS)
}

// Keep `Path` import used even if helpers are trimmed later.
#[allow(dead_code)]
fn _typecheck(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulls_named_section() {
        let note = "## 決定事項 / Decisions\n\n- A を採用\n- B は不採用\n\n## 残課題 / Open todos\n\n_(なし / none)_\n";
        assert!(extract_section(note, &["決定事項", "Decisions"]).unwrap().contains("A を採用"));
        assert!(extract_section(note, &["残課題", "todos"]).is_none());
    }
}
