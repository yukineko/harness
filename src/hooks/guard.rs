//! `ctxrot guard` — UserPromptSubmit hook (port of the Python v1).
//!
//! Output is CONDITIONAL AND MINIMAL: injecting a fixed block every turn would
//! itself accumulate and *cause* rot, so when nothing is relevant we print
//! nothing. Anything returned here on exit 0 is injected into the model context.
//!
//!   T1  large-reference detection (per-prompt): a big local file / URL / "全文"
//!       keyword -> tell the agent to read it via a sub-agent, not main ctx.
//!   T2  context-budget bands (per-session, escalate-only): when real usage
//!       crosses into a higher band, inject distill/offload advice ONCE.

use std::path::Path;

use regex::Regex;

use crate::config::Config;
use crate::model::HookInput;
use crate::transcript;

/// Returns the text to inject (already trimmed), or None to stay silent.
pub fn run(input: &HookInput, cfg: &Config) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();

    if let Some(b) = check_large_references(&input.prompt, &input.cwd_or_current(), cfg) {
        blocks.push(b);
    }
    if let Some(b) = check_context_budget(input, cfg) {
        blocks.push(b);
    }

    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join("\n\n"))
    }
}

// ----------------------------------------------------------------- T1

fn heavy_kw_re() -> Regex {
    // No lookaround needed here.
    Regex::new(
        r"(?i)(全文|全部|まるごと|丸ごと|ログ全部|一字一句|そのまま貼|paste the (?:whole|entire|full)|entire file|whole file|all (?:the )?logs|dump (?:the|all))",
    )
    .expect("static regex")
}

const CONTENT_EXTS: &[&str] = &[
    "log", "json", "jsonl", "csv", "tsv", "txt", "sql", "xml", "html", "htm", "md", "out", "dump",
    "ndjson", "parquet",
];

/// A whitespace token looks like a local path (absolute/home, or has a
/// content-ish extension). Rust's regex has no lookbehind, so we tokenize and
/// test each token instead of one big pattern.
fn looks_like_path(tok: &str) -> bool {
    if tok.starts_with('/') || tok.starts_with("~/") || tok == "~" {
        return true;
    }
    if let Some(ext) = tok.rsplit('.').next() {
        if ext != tok && CONTENT_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    false
}

fn is_url(tok: &str) -> bool {
    tok.starts_with("http://") || tok.starts_with("https://")
}

fn strip_token(tok: &str) -> &str {
    // Trim surrounding quotes/brackets but keep ':' so "path:line" survives.
    tok.trim_matches(|c: char| {
        matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';')
    })
    .trim_end_matches(['.', ',', ';'])
}

fn check_large_references(prompt: &str, cwd: &Path, cfg: &Config) -> Option<String> {
    if prompt.trim().is_empty() {
        return None;
    }

    let mut hits: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for raw_tok in prompt.split_whitespace() {
        let tok = strip_token(raw_tok);
        if tok.is_empty() {
            continue;
        }

        if is_url(tok) {
            if hits.len() < 6 && !seen.iter().any(|s| s == tok) {
                seen.push(tok.to_string());
                hits.push(format!("{tok} (URL)"));
            }
            continue;
        }

        if looks_like_path(tok) {
            if seen.iter().any(|s| s == tok) {
                continue;
            }
            seen.push(tok.to_string());
            let expanded = crate::config::expand_tilde(tok);
            let path = if expanded.is_absolute() {
                expanded
            } else {
                cwd.join(&expanded)
            };
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.is_file() && meta.len() >= cfg.large_file_bytes {
                    let kb = meta.len() as f64 / 1024.0;
                    let tok_est = meta.len() / 4;
                    hits.push(format!("{tok} (~{kb:.0}KB, 推定~{tok_est}tok)"));
                }
            }
        }
        if hits.len() >= 6 {
            break;
        }
    }

    let heavy_kw = heavy_kw_re().is_match(prompt);

    if hits.is_empty() && !heavy_kw {
        return None;
    }

    let mut lines = vec!["[context-rot guard] 大きい参照を検知:".to_string()];
    for h in hits.iter().take(6) {
        lines.push(format!("  - {h}"));
    }
    if heavy_kw && hits.is_empty() {
        lines.push("  - 「全文/まるごと」系の指示を検知".to_string());
    }
    lines.push(
        "→ 全文を main context に載せないでください。Explore（読み取り専用・該当箇所だけ抜粋） \
         または general-purpose sub-agent に読ませ、要約・該当行・結論だけを受け取って作業を。 \
         大きい生データを本文に貼らないこと。"
            .to_string(),
    );
    Some(lines.join("\n"))
}

// ----------------------------------------------------------------- T2

fn check_context_budget(input: &HookInput, cfg: &Config) -> Option<String> {
    if input.transcript_path.is_empty() {
        return None;
    }
    let (est_tokens, _src) = transcript::estimate_tokens(&input.transcript_path)?;
    let frac = est_tokens as f64 / cfg.context_window as f64;
    let band = cfg.band_for(frac);

    // Persist current band (incl. 0). Real usage drops after /compact, so when it
    // falls and later re-climbs, the same band re-fires (not a one-way ratchet).
    let _ = std::fs::create_dir_all(&cfg.state_dir);
    let safe: String = input
        .session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let state_file = cfg.state_dir.join(format!("{safe}.band"));
    let last: usize = std::fs::read_to_string(&state_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    if band != last {
        let _ = std::fs::write(&state_file, band.to_string());
    }
    if band == 0 || band <= last {
        return None;
    }

    let pct = (frac * 100.0) as i64;
    let mut body = match band {
        1 => format!(
            "context使用が推定~{pct}%。区切りの良い所で、確定した結論・決定事項だけ残し、\
             試行錯誤の経過は要約に畳む準備を。以降の重い読み込みは sub-agent 経由に。"
        ),
        2 => format!(
            "context使用が推定~{pct}%。退避を推奨: 長い成果物は外部doc（Obsidian等）へ書き出し、\
             main context は「要約＋リンク」に置換を。/distill で能動蒸留、詳細調査は sub-agent に委譲して結論だけ受け取る運用へ切替。"
        ),
        _ => format!(
            "context使用が推定~{pct}%（危険域）。今やる: (1) /distill で未保存の成果物を外部docへ退避 \
             (2) /compact もしくは会話の蒸留 (3) 以降の重い読み込み・検索は必ず sub-agent 経由。"
        ),
    };

    // Preemptive rescue (P1-1a): from band 2 (≈75%) up, write a fresh durable
    // rescue note NOW — don't wait for PreCompact, which a manual `/clear` never
    // fires. The band gate already escalates at most once per crossing, so this
    // writes a bounded number of notes per session. A failed write just means no
    // confirmation line; the advice itself is unaffected.
    if band >= 2 {
        if let Some(path) = crate::hooks::rescue::write(input, cfg, &format!("band-{pct}%")) {
            body.push_str(&format!(
                "\n先行退避ノートを書き出しました（このまま /compact・/clear しても安全）: {}",
                path.display()
            ));
        }
    }

    Some(format!("[context-rot guard] {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_detected() {
        assert!(is_url("https://example.com/x"));
        assert!(!is_url("example.com"));
    }

    #[test]
    fn path_shapes() {
        assert!(looks_like_path("/var/log/app.log"));
        assert!(looks_like_path("~/data.csv"));
        assert!(looks_like_path("notes.md"));
        assert!(!looks_like_path("hello"));
        assert!(!looks_like_path("function"));
    }

    #[test]
    fn heavy_kw() {
        assert!(heavy_kw_re().is_match("このログ全部貼って"));
        assert!(heavy_kw_re().is_match("paste the entire file"));
        assert!(!heavy_kw_re().is_match("少しだけ見せて"));
    }

    #[test]
    fn band_thresholds() {
        let cfg = Config::default();
        assert_eq!(cfg.band_for(0.10), 0);
        assert_eq!(cfg.band_for(0.50), 1);
        assert_eq!(cfg.band_for(0.80), 2);
        assert_eq!(cfg.band_for(0.95), 3);
    }

    #[test]
    fn band_crossing_writes_preemptive_rescue() {
        let base = std::env::temp_dir().join(format!("ctxrot-guard-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cwd = base.join("proj");
        std::fs::create_dir_all(&cwd).unwrap();

        let cfg = Config {
            state_dir: base.join("state"),
            store_dir: base.join("store"),
            ..Config::default()
        };
        let input = HookInput {
            session_id: "sess-guard".into(),
            // Fixture usage ≈ 184200 / 200000 ≈ 92% → band 3 (≥2).
            transcript_path: "tests/fixtures/transcript.jsonl".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            ..HookInput::default()
        };

        // First crossing: advice mentions the preemptive note, and one is on disk.
        let out = check_context_budget(&input, &cfg).expect("band advice on first crossing");
        assert!(
            out.contains("先行退避ノート"),
            "should confirm preemptive rescue: {out}"
        );

        let store = crate::store::Store::new(&cfg);
        let notes = store.list_notes(&cwd);
        assert!(
            notes.iter().any(|p| p
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with("rescue-"))),
            "expected a rescue-*.md note, got {notes:?}"
        );

        // Escalate-only: the same band does not re-fire (so it won't re-rescue every turn).
        assert!(check_context_budget(&input, &cfg).is_none());

        let _ = std::fs::remove_dir_all(&base);
    }
}
