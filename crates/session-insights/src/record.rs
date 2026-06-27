//! Optional: write/update an AEGIS-style Markdown *record* note on SessionEnd.
//!
//! Opt-in (`record = true`) and only if the vault directory already exists — we
//! never create the vault. Unlike the terse `sessions/` note, this note has NO
//! YAML frontmatter and uses the AEGIS section skeleton (Japanese headings). The
//! prose sections are left as `<!-- fill: … -->` placeholders for a later
//! model-driven `/record` pass to fill.
//!
//! Append-merge, never overwrite: on first creation we emit the full skeleton;
//! on later calls we replace ONLY the auto-generated `## コスト` and
//! `## 数値サマリ` blocks (delimited by stable HTML-comment markers) and leave
//! every prose section untouched.

use std::path::PathBuf;

use harness_core::pricing::{self, PriceOverride};
use harness_core::transcript;
use harness_core::usage;

use crate::config::Config;
use crate::metrics::{short, Session};

const COST_START: &str = "<!-- si:cost:start -->";
const COST_END: &str = "<!-- si:cost:end -->";
const NUM_START: &str = "<!-- si:numeric:start -->";
const NUM_END: &str = "<!-- si:numeric:end -->";

/// Inputs the writer needs beyond the persisted rollup. `transcript_path` feeds
/// the cost (usage aggregation) and end-of-session context estimate.
pub struct RecordCtx<'a> {
    pub session_id: &'a str,
    pub project: &'a str,
    pub date: &'a str,
    pub turns: u64,
    pub tool_events: u64,
    pub files_touched: usize,
    pub transcript_path: &'a str,
    pub overrides: &'a [PriceOverride],
}

/// Build the auto `## 数値サマリ` block body (between markers, exclusive).
fn numeric_body(ctx: &RecordCtx) -> String {
    let context_line = match transcript::estimate_tokens(ctx.transcript_path) {
        Some((tokens, band)) => format!("- context: ~{tokens} tokens ({band})"),
        None => "- context: (計測データなし)".to_string(),
    };
    format!(
        "- turns: {}\n- tool events: {}\n- files touched: {}\n{}",
        ctx.turns, ctx.tool_events, ctx.files_touched, context_line
    )
}

/// Build the auto `## コスト` block body (between markers, exclusive).
fn cost_body(ctx: &RecordCtx) -> String {
    let Some(agg) = usage::aggregate(ctx.transcript_path) else {
        return "- (コストデータなし)".to_string();
    };
    let total_usd = pricing::session_cost(agg.models.iter(), ctx.overrides);
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_write = 0u64;
    let mut cache_read = 0u64;
    let mut total_tokens = 0u64;
    for u in agg.models.values() {
        input += u.input;
        output += u.output;
        cache_write += u.cache_write_5m + u.cache_write_1h;
        cache_read += u.cache_read;
        total_tokens += u.total_tokens();
    }
    let models: Vec<String> = agg.models.keys().cloned().collect();
    let models_line = if models.is_empty() {
        "(none)".to_string()
    } else {
        models.join(", ")
    };
    format!(
        "- total: ${total_usd:.2} USD\n\
         - total tokens: {total_tokens}\n\
         - input: {input}   output: {output}\n\
         - cache write: {cache_write}   cache read: {cache_read}\n\
         - models: {models_line}"
    )
}

/// The full skeleton emitted on first creation.
fn skeleton(ctx: &RecordCtx) -> String {
    format!(
        "# {date} {project} セッション記録\n\
         \n\
         ## 完了サマリ\n\
         <!-- fill: 完了サマリ -->\n\
         \n\
         ## つまずき / 学び\n\
         <!-- fill: つまずき / 学び -->\n\
         \n\
         ## 振り返り / 確立した方針\n\
         <!-- fill: 振り返り / 確立した方針 -->\n\
         \n\
         ## 注意点 / 落とし穴\n\
         <!-- fill: 注意点 / 落とし穴 -->\n\
         \n\
         ## 数値サマリ\n\
         {NUM_START}\n\
         {numeric}\n\
         {NUM_END}\n\
         \n\
         ## コスト\n\
         {COST_START}\n\
         {cost}\n\
         {COST_END}\n\
         \n\
         ## 残課題\n\
         <!-- fill: 残課題 -->\n\
         \n\
         ## 要追跡 / あとで確認\n\
         <!-- fill: 要追跡 / あとで確認 -->\n\
         \n\
         ## 関連\n\
         <!-- fill: 関連 -->\n",
        date = ctx.date,
        project = ctx.project,
        numeric = numeric_body(ctx),
        cost = cost_body(ctx),
    )
}

/// Replace the body between `start`/`end` markers (inclusive of marker lines'
/// content) with `new_body`. If the markers are absent, returns the input
/// unchanged (a model may have restructured the note; we don't force-insert).
fn replace_block(text: &str, start: &str, end: &str, new_body: &str) -> String {
    let Some(s) = text.find(start) else {
        return text.to_string();
    };
    let after_start = s + start.len();
    let Some(rel_e) = text[after_start..].find(end) else {
        return text.to_string();
    };
    let e = after_start + rel_e;
    let mut out = String::with_capacity(text.len() + new_body.len());
    out.push_str(&text[..after_start]);
    out.push('\n');
    out.push_str(new_body);
    out.push('\n');
    out.push_str(&text[e..]);
    out
}

/// Refresh both auto blocks in an existing note.
fn merge(existing: &str, ctx: &RecordCtx) -> String {
    let t = replace_block(existing, NUM_START, NUM_END, &numeric_body(ctx));
    replace_block(&t, COST_START, COST_END, &cost_body(ctx))
}

/// Write or update the record note. Returns the written path, or None if
/// skipped/failed. Fail-soft throughout.
pub fn write_record(cfg: &Config, ctx: &RecordCtx) -> Option<PathBuf> {
    if !cfg.record || !cfg.obsidian_vault.is_dir() {
        return None;
    }
    if ctx.date.len() < 10 {
        return None;
    }
    let dir = cfg.obsidian_vault.join(&cfg.record_dir);
    std::fs::create_dir_all(&dir).ok()?;
    let slug = format!("{}-{}", slugify(ctx.project), short(ctx.session_id));
    let path = dir.join(format!("{}-{}.md", &ctx.date[..10], slug));

    let body = match std::fs::read_to_string(&path) {
        Ok(existing) => merge(&existing, ctx),
        Err(_) => skeleton(ctx),
    };
    std::fs::write(&path, body).ok()?;
    Some(path)
}

/// Convenience: build a `RecordCtx` from a persisted rollup + transcript and write.
pub fn write_from_session(
    cfg: &Config,
    s: &Session,
    transcript_path: &str,
    turns_fallback: u64,
) -> Option<PathBuf> {
    let date = if s.started_at.len() >= 10 {
        s.started_at[..10].to_string()
    } else {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    };
    let turns = if s.turns > 0 { s.turns } else { turns_fallback };
    let ctx = RecordCtx {
        session_id: &s.session_id,
        project: &s.project,
        date: &date,
        turns,
        tool_events: s.tool_events,
        files_touched: s.files.len(),
        transcript_path,
        overrides: &cfg.price_overrides,
    };
    write_record(cfg, &ctx)
}

/// Lowercased kebab slug from a project name (ASCII alnum kept; runs of other
/// chars collapse to a single `-`). Non-ASCII is dropped; empty → "session".
fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "session".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_transcript(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("si-record-{}-{name}.jsonl", std::process::id()));
        let body = concat!(
            r#"{"type":"assistant","timestamp":"2026-06-22T10:00:01Z","message":{"model":"claude-opus-4-8","content":[{"type":"text","text":"x"},{"type":"tool_use","name":"Bash"}],"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":0}}}"#,
            "\n",
        );
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    fn ctx<'a>(tp: &'a str, turns: u64) -> RecordCtx<'a> {
        RecordCtx {
            session_id: "abcdef1234567890",
            project: "harness",
            date: "2026-06-22",
            turns,
            tool_events: 7,
            files_touched: 3,
            transcript_path: tp,
            overrides: &[],
        }
    }

    #[test]
    fn skeleton_has_all_sections_and_placeholders() {
        let tp = write_transcript("skel");
        let s = skeleton(&ctx(tp.to_str().unwrap(), 4));
        for h in [
            "# 2026-06-22 harness セッション記録",
            "## 完了サマリ",
            "## つまずき / 学び",
            "## 振り返り / 確立した方針",
            "## 注意点 / 落とし穴",
            "## 数値サマリ",
            "## コスト",
            "## 残課題",
            "## 要追跡 / あとで確認",
            "## 関連",
        ] {
            assert!(s.contains(h), "missing heading: {h}\n{s}");
        }
        assert!(s.contains("<!-- fill: 完了サマリ -->"));
        assert!(s.contains("<!-- fill: 注意点 / 落とし穴 -->"));
        assert!(s.contains("<!-- fill: 要追跡 / あとで確認 -->"));
        assert!(s.contains(COST_START) && s.contains(COST_END));
        assert!(s.contains(NUM_START) && s.contains(NUM_END));
        // numeric auto values
        assert!(s.contains("- turns: 4"));
        assert!(s.contains("- tool events: 7"));
        assert!(s.contains("- files touched: 3"));
        assert!(s.contains("- context: ~"));
        // cost: opus 1M in + 1M out = 5 + 25 = $30.00; total tokens 2,000,000
        assert!(s.contains("- total: $30.00 USD"), "{s}");
        assert!(s.contains("- total tokens: 2000000"));
        assert!(s.contains("claude-opus-4-8"));
        let _ = std::fs::remove_file(&tp);
    }

    #[test]
    fn merge_replaces_auto_blocks_and_preserves_prose() {
        let tp = write_transcript("merge");
        // First-create skeleton, then a model fills the prose.
        let first = skeleton(&ctx(tp.to_str().unwrap(), 4));
        let filled = first
            .replace("<!-- fill: 完了サマリ -->", "実際にやったこと: record.rs を追加")
            .replace("<!-- fill: 残課題 -->", "テストを増やす");

        // Second call: different numbers.
        let merged = merge(&filled, &ctx(tp.to_str().unwrap(), 9));

        // Prose preserved.
        assert!(merged.contains("実際にやったこと: record.rs を追加"));
        assert!(merged.contains("テストを増やす"));
        // Auto blocks refreshed: turns now 9.
        assert!(merged.contains("- turns: 9"), "{merged}");
        assert!(!merged.contains("- turns: 4"));
        // Markers still present exactly once each.
        assert_eq!(merged.matches(COST_START).count(), 1);
        assert_eq!(merged.matches(COST_END).count(), 1);
        assert_eq!(merged.matches(NUM_START).count(), 1);
        // Cost unchanged (same transcript) and still correct.
        assert!(merged.contains("- total: $30.00 USD"));
        let _ = std::fs::remove_file(&tp);
    }

    #[test]
    fn write_record_first_then_merge_on_disk() {
        let tp = write_transcript("disk");
        let vault = std::env::temp_dir().join(format!("si-vault-{}", std::process::id()));
        std::fs::create_dir_all(&vault).unwrap();
        let cfg = Config {
            record: true,
            obsidian_vault: vault.clone(),
            ..Config::default()
        };

        let mut s = Session {
            session_id: "abcdef1234567890".into(),
            project: "harness".into(),
            started_at: "2026-06-22T10:00:00Z".into(),
            turns: 4,
            tool_events: 7,
            files: vec!["a".into(), "b".into(), "c".into()],
            ..Session::default()
        };

        let p1 = write_from_session(&cfg, &s, tp.to_str().unwrap(), 0).expect("first write");
        assert!(p1.ends_with("2026-06-22-harness-abcdef12.md"));
        let on_disk = std::fs::read_to_string(&p1).unwrap();
        let edited = on_disk.replace("<!-- fill: 完了サマリ -->", "PROSE-KEEP");
        std::fs::write(&p1, &edited).unwrap();

        // Second write merges, preserves prose.
        s.turns = 12;
        let p2 = write_from_session(&cfg, &s, tp.to_str().unwrap(), 0).expect("second write");
        assert_eq!(p1, p2);
        let after = std::fs::read_to_string(&p2).unwrap();
        assert!(after.contains("PROSE-KEEP"));
        assert!(after.contains("- turns: 12"));

        let _ = std::fs::remove_file(&tp);
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn disabled_or_missing_vault_is_noop() {
        let tp = write_transcript("noop");
        let s = Session::default();
        // record disabled (the default) → no-op
        let cfg_off = Config::default();
        assert!(write_from_session(&cfg_off, &s, tp.to_str().unwrap(), 0).is_none());
        // record enabled but the vault dir does not exist → still a no-op
        let cfg_on = Config {
            record: true,
            obsidian_vault: std::env::temp_dir().join("si-nonexistent-vault-xyz"),
            ..Config::default()
        };
        assert!(write_from_session(&cfg_on, &s, tp.to_str().unwrap(), 0).is_none());
        let _ = std::fs::remove_file(&tp);
    }

    #[test]
    fn slug_basics() {
        assert_eq!(slugify("Harness Project"), "harness-project");
        assert_eq!(slugify("my_repo.v2"), "my-repo-v2");
        assert_eq!(slugify("日本語"), "session");
    }
}
