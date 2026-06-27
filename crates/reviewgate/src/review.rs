//! The gate proper: gather the reviewable diff, decide whether to block the
//! stop, and produce the model-facing reason. Two modes:
//!
//!   * `inject` — block once per new diff state and inject a review rubric; the
//!     running subscription agent reviews its own changes.
//!   * `subprocess` — run an independent reviewer over the diff and block only
//!     when it reports issues, injecting just those findings.
//!
//! Convergence: we hash the reviewable diff. A stop whose diff matches the last
//! one we forced a review of is allowed (already reviewed). A changed diff costs
//! one more round, capped by `max_attempts`. Harness errors always allow.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use globset::{Glob, GlobSetBuilder};
use wait_timeout::ChildExt;

use crate::config::{Config, Mode};

/// What the gate decided. `tag` is a short label for the JSONL log.
pub enum Decision {
    /// Let the stop through. Carries the per-session state to persist and a log tag.
    Allow {
        tag: &'static str,
        attempts: u32,
        last_hash: String,
    },
    /// Block the stop; inject `reason`. Carries state to persist.
    Block {
        reason: String,
        tag: &'static str,
        files: Vec<String>,
        attempts: u32,
        last_hash: String,
    },
}

/// Files that changed *and* are worth reviewing (match include, not exclude).
pub fn reviewable_files(cfg: &Config, changed: &[String]) -> Vec<String> {
    let inc = build_set(&cfg.include);
    let exc = build_set(&cfg.exclude);
    changed
        .iter()
        .filter(|f| {
            inc.as_ref().map(|s| s.is_match(f)).unwrap_or(true)
                && !exc.as_ref().map(|s| s.is_match(f)).unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn build_set(globs: &[String]) -> Option<globset::GlobSet> {
    let mut b = GlobSetBuilder::new();
    let mut any = false;
    for g in globs {
        if let Ok(glob) = Glob::new(g) {
            b.add(glob);
            any = true;
        }
    }
    if !any {
        return None;
    }
    b.build().ok()
}

fn hash_diff(diff: &str) -> String {
    let mut h = DefaultHasher::new();
    diff.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

/// Core decision. `st` is the loaded prior session state.
pub fn evaluate(cfg: &Config, root: &Path, st: &crate::state::SessionState) -> Decision {
    let Some(changed) = crate::git::changed_files(root) else {
        return allow("no-git", st);
    };
    let files = reviewable_files(cfg, &changed);
    if files.len() < cfg.min_changed_files {
        return allow("no-reviewable-changes", st);
    }

    let diff = crate::git::diff_text(root, &files, cfg.max_diff_bytes);
    if diff.trim().is_empty() {
        return allow("empty-diff", st);
    }
    let hash = hash_diff(&diff);

    // attempt counter resets after an idle gap (a fresh turn).
    let prior_attempts = if now() - st.last_ts > cfg.reset_after_secs {
        0
    } else {
        st.attempts
    };

    // Same diff we already forced a review of → the agent reviewed exactly this.
    if !st.last_hash.is_empty() && st.last_hash == hash {
        return allow("already-reviewed", st);
    }

    match cfg.mode {
        Mode::Inject => {
            let attempts = prior_attempts + 1;
            if attempts > cfg.max_attempts {
                return Decision::Allow {
                    tag: "giveup",
                    attempts: 0,
                    last_hash: String::new(),
                };
            }
            let reason = inject_reason(cfg, &files, attempts);
            Decision::Block {
                reason,
                tag: "blocked-inject",
                files,
                attempts,
                last_hash: hash,
            }
        }
        Mode::Subprocess => match run_reviewer(cfg, &diff) {
            ReviewerResult::Error(e) => {
                eprintln!("reviewgate: reviewer command failed ({e}) — allowing stop");
                // Don't record the hash: retry the review on the next stop.
                allow("reviewer-error", st)
            }
            ReviewerResult::Clean => Decision::Allow {
                tag: "clean",
                attempts: 0,
                last_hash: hash,
            },
            ReviewerResult::Issues(findings) => {
                let attempts = prior_attempts + 1;
                if attempts > cfg.max_attempts {
                    return Decision::Allow {
                        tag: "giveup",
                        attempts: 0,
                        last_hash: String::new(),
                    };
                }
                let reason = subprocess_reason(&files, &findings, attempts, cfg.max_attempts);
                Decision::Block {
                    reason,
                    tag: "blocked-review",
                    files,
                    attempts,
                    last_hash: hash,
                }
            }
        },
    }
}

fn allow(tag: &'static str, st: &crate::state::SessionState) -> Decision {
    Decision::Allow {
        tag,
        attempts: 0,
        last_hash: st.last_hash.clone(),
    }
}

fn file_list(files: &[String]) -> String {
    let mut s = String::new();
    for f in files.iter().take(40) {
        s.push_str("  ");
        s.push_str(f);
        s.push('\n');
    }
    if files.len() > 40 {
        s.push_str(&format!("  … (+{} more)\n", files.len() - 40));
    }
    s
}

/// inject mode: ask the running agent to review its own diff.
fn inject_reason(cfg: &Config, files: &[String], attempt: u32) -> String {
    format!(
        "🔍 reviewgate: 完了前に、自分の変更をコードレビューしてください (round {attempt}/{max}).\n\n\
         レビュー対象 ({n} files):\n{list}\n\
         `git diff` で差分を確認し、次の観点でレビューしてください:\n{rubric}\n\n\
         実在する問題が見つかれば修正してから完了してください。\
         レビューの結果と対応を簡潔に報告すること。\
         修正不要・対応済みなら、そのまま完了して構いません（同じ差分での次の停止は許可されます）。\n\n\
         このレビューを1回だけスキップ: project root に `.reviewgate-skip` を作成（理由を1行）。\
         完全に無効化: 環境変数 REVIEWGATE_DISABLE=1。",
        attempt = attempt,
        max = cfg.max_attempts,
        n = files.len(),
        list = file_list(files),
        rubric = cfg.rubric,
    )
}

/// subprocess mode: inject the independent reviewer's findings.
fn subprocess_reason(files: &[String], findings: &str, attempt: u32, max: u32) -> String {
    format!(
        "🔍 reviewgate: 独立レビュアーが変更に問題を指摘しました (round {attempt}/{max}).\n\n\
         レビュー対象 ({n} files):\n{list}\n\
         --- 指摘 ---\n{findings}\n\
         ------------\n\n\
         妥当な指摘を修正してから完了してください。誤検知だと判断した指摘は、理由を述べてスキップして構いません。\n\n\
         このレビューを1回だけスキップ: `.reviewgate-skip` を作成。完全に無効化: REVIEWGATE_DISABLE=1。",
        attempt = attempt,
        max = max,
        n = files.len(),
        list = file_list(files),
        findings = findings.trim(),
    )
}

enum ReviewerResult {
    Clean,
    Issues(String),
    Error(String),
}

/// Run `reviewer_cmd`, feeding it the review prompt on stdin and reading
/// findings from stdout. Output that is empty or starts with "LGTM" = clean.
fn run_reviewer(cfg: &Config, diff: &str) -> ReviewerResult {
    let prompt = format!(
        "あなたは独立した辛口のコードレビュアーです。以下の git diff をレビューしてください。\n\n\
         観点:\n{rubric}\n\n\
         実在し根拠のある問題だけを、深刻度(high/med/low)付きで簡潔に箇条書きしてください。\
         該当ファイルと行が分かるよう示すこと。問題が無ければ `LGTM` とだけ出力してください。\n\n\
         --- diff ---\n{diff}\n",
        rubric = cfg.rubric,
        diff = diff,
    );

    let mut cmd = build_command(&cfg.reviewer_cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ReviewerResult::Error(format!("spawn: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
        // drop closes stdin so the reviewer sees EOF
    }

    let timeout = Duration::from_secs(cfg.reviewer_timeout_secs);
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let mut out = String::new();
            if let Some(mut so) = child.stdout.take() {
                use std::io::Read;
                let _ = so.read_to_string(&mut out);
            }
            if !status.success() && out.trim().is_empty() {
                return ReviewerResult::Error(format!("exit {:?}", status.code()));
            }
            classify(&out)
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            ReviewerResult::Error("timed out".to_string())
        }
        Err(e) => ReviewerResult::Error(format!("wait: {e}")),
    }
}

fn classify(out: &str) -> ReviewerResult {
    let t = out.trim();
    if t.is_empty() {
        return ReviewerResult::Clean;
    }
    let first = t.lines().next().unwrap_or("").trim();
    if first.eq_ignore_ascii_case("lgtm") || first.to_ascii_lowercase().starts_with("lgtm") {
        return ReviewerResult::Clean;
    }
    ReviewerResult::Issues(t.to_string())
}

/// Split a command line into program + args for `sh -c`-free direct spawn,
/// falling back to the shell for anything with shell metacharacters.
fn build_command(cmdline: &str) -> Command {
    let needs_shell = cmdline.contains(|c| "|&;<>(){}$`\\\"'*?".contains(c));
    if needs_shell {
        harness_core::shell::command(cmdline)
    } else {
        let mut parts = cmdline.split_whitespace();
        let prog = parts.next().unwrap_or("claude");
        let mut c = Command::new(prog);
        c.args(parts);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(include: &[&str], exclude: &[&str]) -> Config {
        Config {
            include: include.iter().map(|s| s.to_string()).collect(),
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn include_exclude_filtering() {
        let cfg = cfg_with(&["**/*.rs"], &["**/target/**"]);
        let changed = vec![
            "src/main.rs".to_string(),
            "README.md".to_string(),
            "target/x.rs".to_string(),
        ];
        let r = reviewable_files(&cfg, &changed);
        assert_eq!(r, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn lockfile_excluded_by_default() {
        let cfg = Config::default();
        let changed = vec!["Cargo.lock".to_string(), "package-lock.json".to_string()];
        assert!(reviewable_files(&cfg, &changed).is_empty());
    }

    #[test]
    fn classify_lgtm_is_clean() {
        assert!(matches!(classify("LGTM"), ReviewerResult::Clean));
        assert!(matches!(classify("  lgtm \n"), ReviewerResult::Clean));
        assert!(matches!(classify(""), ReviewerResult::Clean));
        assert!(matches!(
            classify("- high: bug in foo.rs:10"),
            ReviewerResult::Issues(_)
        ));
    }

    #[test]
    fn hash_is_stable_and_distinct() {
        assert_eq!(hash_diff("abc"), hash_diff("abc"));
        assert_ne!(hash_diff("abc"), hash_diff("abd"));
    }
}
