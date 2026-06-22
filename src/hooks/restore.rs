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
    // Prefer this session's own note (resume/compact keep the same session_id),
    // so parallel sessions don't grab each other's carryover. Else fall back to a
    // SAFE cross-session note: the latest when the stream is unambiguous, but
    // never a sibling session's tagged note when parallel usage is detected.
    let latest = store
        .latest_note_for_session(&cwd, &input.session_id)
        .or_else(|| store.latest_fallback_note(&cwd))?;

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
        let msg = format!(
            "[ctxrot restore] 前回の退避ノートあり: {}\n→ 続きから作業する場合は読み込んで。",
            latest.display()
        );
        emit_restore(cfg, input, &latest, msg.len(), false, false);
        return Some(msg);
    }
    emit_restore(
        cfg,
        input,
        &latest,
        out.len(),
        decisions.is_some(),
        todos.is_some(),
    );
    Some(out)
}

fn emit_restore(
    cfg: &Config,
    input: &HookInput,
    note: &Path,
    bytes: usize,
    had_decisions: bool,
    had_todos: bool,
) {
    crate::metrics::emit(
        cfg,
        &input.session_id,
        "restore",
        serde_json::json!({
            "note": note.to_string_lossy(),
            "bytes": bytes,
            "had_decisions": had_decisions,
            "had_todos": had_todos,
        }),
    );
}

/// The section headings `restore` depends on for carryover. This is the single
/// source of truth for the distill *contract*: a distilled note that omits one
/// of these silently produces an empty carryover, so `note write
/// --require-sections` rejects it. Each entry is `(human label, heading aliases)`.
pub const REQUIRED_SECTIONS: &[(&str, &[&str])] = &[
    ("決定事項 / Decisions", &["決定事項", "Decisions"]),
    ("残課題 / Open todos", &["残課題", "Open todos", "todos"]),
];

/// True if `text` has a `## …` heading matching any of `titles` (presence only —
/// an empty "_(なし)_" section still counts, since `restore` handles that).
/// Matches exactly what `extract_section` keys on, so the contract guarantees
/// `restore` can find the section.
pub fn has_section(text: &str, titles: &[&str]) -> bool {
    text.lines().any(|l| {
        l.trim()
            .strip_prefix("## ")
            .map(|rest| titles.iter().any(|t| rest.contains(t)))
            .unwrap_or(false)
    })
}

/// Human labels of any `REQUIRED_SECTIONS` whose heading is missing from `text`.
/// Empty vec means the note satisfies the contract.
pub fn missing_sections(text: &str) -> Vec<&'static str> {
    REQUIRED_SECTIONS
        .iter()
        .filter(|(_, aliases)| !has_section(text, aliases))
        .map(|(label, _)| *label)
        .collect()
}

/// Sections that materially improve carryover quality but aren't load-bearing
/// for `restore` (which only consumes Decisions/todos). These mirror the rest of
/// the distill template's shape. `note write --require-sections` only *warns* on
/// these — it never rejects — so the full structure is encouraged without making
/// a thin-but-valid note impossible. Each entry is `(human label, heading aliases)`.
pub const RECOMMENDED_SECTIONS: &[(&str, &[&str])] = &[
    ("触ったファイル / Files", &["触ったファイル", "Files"]),
    ("重要な事実 / Key facts", &["重要な事実", "重要事実", "Key facts"]),
    ("現在地 / Where we are", &["現在地", "Where we are"]),
];

/// Human labels of any `RECOMMENDED_SECTIONS` whose heading is missing from
/// `text`. Empty vec means the note carries the full distill shape.
pub fn missing_recommended_sections(text: &str) -> Vec<&'static str> {
    RECOMMENDED_SECTIONS
        .iter()
        .filter(|(_, aliases)| !has_section(text, aliases))
        .map(|(label, _)| *label)
        .collect()
}

/// Pull the body under a `## <title>` heading (any of the aliases), bounded.
/// Skips the "_(なし / none)_" placeholder. Public so the re-anchor check
/// (`guard::check_reanchor`) reuses the exact same extraction `restore` does.
pub fn extract_section(text: &str, titles: &[&str]) -> Option<String> {
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

    #[test]
    fn contract_accepts_required_headings_even_when_empty() {
        // Both headings present (todos is the "none" placeholder) → conformant.
        let note = "## 決定事項 / Decisions\n\n- A\n\n## 残課題 / Open todos\n\n_(なし / none)_\n";
        assert!(missing_sections(note).is_empty());
    }

    #[test]
    fn contract_flags_omitted_section() {
        // The distiller dropped the empty Open-todos heading entirely → violation,
        // exactly the silent-failure restore can't recover from.
        let note = "## 決定事項 / Decisions\n\n- A\n\n## 触ったファイル / Files\n\n- x.rs\n";
        let missing = missing_sections(note);
        assert_eq!(missing, vec!["残課題 / Open todos"]);
    }

    #[test]
    fn contract_flags_both_when_renamed() {
        // Decisions hidden under a non-canonical heading → restore would miss it.
        let note = "## まとめ\n\n- A を採用\n- 次は B\n";
        assert_eq!(missing_sections(note).len(), 2);
    }

    #[test]
    fn recommended_flags_only_soft_sections() {
        // Required headings present, all recommended ones absent → the soft check
        // names the three template extras and the hard check stays clean.
        let note = "## 決定事項 / Decisions\n\n- A\n\n## 残課題 / Open todos\n\n_(なし / none)_\n";
        assert!(missing_sections(note).is_empty());
        assert_eq!(missing_recommended_sections(note).len(), 3);
    }

    #[test]
    fn recommended_satisfied_by_full_shape() {
        let note = "## 決定事項 / Decisions\n\n- A\n\n## 残課題 / Open todos\n\n- B\n\n\
                    ## 触ったファイル / Files\n\n- x.rs\n\n## 重要な事実 / Key facts\n\n- k\n\n\
                    ## 現在地 / Where we are\n\n- here\n";
        assert!(missing_recommended_sections(note).is_empty());
    }
}
