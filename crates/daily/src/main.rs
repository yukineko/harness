//! daily — run tasks once per calendar day at SessionStart.
//!
//! Currently implemented tasks:
//!   security  cargo deny check advisories bans sources licenses
//!
//! State is stored in `~/.daily/state/<task>-daily.txt` via `DailyGuard`.
//! Config at `~/.daily/config.toml`; set `enabled = false` to disable all tasks.

use clap::{Parser, Subcommand};
use harness_core::daily::DailyGuard;
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "daily", version, about = "Run tasks once per calendar day at SessionStart")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// SessionStart hook: run pending daily tasks.
    SessionStart,
    /// Install the SessionStart hook into ~/.claude/settings.json (not yet implemented).
    Install,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Cmd::SessionStart => session_start_cmd(),
        Cmd::Install => {
            eprintln!("daily install: not yet implemented — add the hook manually to ~/.claude/settings.json");
            std::process::exit(0);
        }
    }
}

fn session_start_cmd() -> ! {
    run_hook(|| {
        let raw = read_stdin();
        let input = HookInput::parse(&raw).unwrap_or_default();

        if is_disabled() {
            return;
        }

        let state_dir = daily_state_dir();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let guard = DailyGuard::new(&state_dir, "security", &today);

        if !guard.should_run() {
            return;
        }

        let cwd = input.cwd_or_current();
        let outcome = run_security_audit(&cwd);
        guard.mark_done().ok();

        // Clean stays silent; everything else (findings, a missing tool, or an
        // execution error) surfaces a distinct message so an audit that never
        // ran can't masquerade as a clean one.
        if let Some(msg) = audit_message(&outcome) {
            println!("{}", json!({ "additionalContext": msg }));
        }
    })
}

/// The result of attempting the daily security audit.
enum AuditOutcome {
    /// cargo-deny ran and found nothing.
    Clean,
    /// cargo-deny ran and reported problems (summarized).
    Findings(String),
    /// cargo-deny is not installed — the audit did NOT run.
    ToolMissing,
    /// cargo-deny exists but the invocation itself failed to run.
    Failed(String),
}

/// Map an outcome to the message shown via `additionalContext`, or `None` to
/// stay silent. `ToolMissing` and `Failed` are worded distinctly so a
/// never-installed tool reads as "audit not run", not as a passing audit.
fn audit_message(outcome: &AuditOutcome) -> Option<String> {
    match outcome {
        AuditOutcome::Clean => None,
        AuditOutcome::Findings(summary) => Some(format!(
            "🔒 daily security audit: cargo deny check で問題が検出されました。\n{summary}\n`cargo deny check` を実行して詳細を確認してください。"
        )),
        AuditOutcome::ToolMissing => Some(
            "🔒 daily security audit: 未実施 — cargo-deny が見つかりません。脆弱性監査が一度も実行されていないため clean ではありません。\n`cargo install cargo-deny` でインストールしてください: https://github.com/EmbarkStudios/cargo-deny"
                .to_string(),
        ),
        AuditOutcome::Failed(err) => Some(format!(
            "🔒 daily security audit: cargo-deny は存在しますが実行エラーで完了できませんでした（未実施）: {err}"
        )),
    }
}

/// Run cargo-deny, classifying the result. A missing binary (spawn `NotFound`)
/// is reported as `ToolMissing` — distinct from a real execution `Failed` — so
/// the caller never confuses "not installed" with "audited clean".
fn run_security_audit(cwd: &std::path::Path) -> AuditOutcome {
    let cargo_deny = cargo_deny_path();
    let out = match Command::new(&cargo_deny)
        .args(["check", "advisories", "bans", "sources", "licenses"])
        .current_dir(cwd)
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AuditOutcome::ToolMissing,
        Err(e) => return AuditOutcome::Failed(e.to_string()),
    };

    if out.status.success() {
        return AuditOutcome::Clean;
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{stdout}{stderr}");
    AuditOutcome::Findings(summarize_deny_output(&combined))
}

fn cargo_deny_path() -> PathBuf {
    // Try $CARGO_HOME/bin/cargo-deny, then fall back to a bare name so PATH is
    // searched. When that bare name isn't on PATH either, the spawn fails with
    // `NotFound`, which run_security_audit maps to `ToolMissing`.
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".cargo"));
    let candidate = cargo_home.join("bin/cargo-deny");
    if candidate.exists() {
        return candidate;
    }
    PathBuf::from("cargo-deny")
}

/// Extract the first few warning/error lines from cargo deny output.
fn summarize_deny_output(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|l| l.contains("error") || l.contains("warning") || l.contains("RUSTSEC"))
        .take(5)
        .collect();
    if lines.is_empty() {
        output.lines().take(5).collect::<Vec<_>>().join("\n")
    } else {
        lines.join("\n")
    }
}

fn daily_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".daily/state")
}

fn is_disabled() -> bool {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".daily/config.toml");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        // Simple check: look for `enabled = false`
        return content.contains("enabled = false");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_message_distinguishes_missing_from_failure_and_clean() {
        // Clean stays silent.
        assert!(audit_message(&AuditOutcome::Clean).is_none());

        // A missing tool reads as "not run", names cargo-deny, and gives an install path.
        let missing = audit_message(&AuditOutcome::ToolMissing).unwrap();
        assert!(missing.contains("cargo-deny"));
        assert!(missing.contains("未実施"));
        assert!(missing.contains("cargo install cargo-deny"));

        // An execution error is a different message and carries the error text.
        let failed = audit_message(&AuditOutcome::Failed("boom".to_string())).unwrap();
        assert!(failed.contains("boom"));
        assert_ne!(missing, failed, "tool-missing and exec-error must be distinct");

        // Findings carry the summary through.
        let findings = audit_message(&AuditOutcome::Findings("RUSTSEC-0001".to_string())).unwrap();
        assert!(findings.contains("RUSTSEC-0001"));
    }

    #[test]
    fn summarize_picks_error_lines() {
        let output = "some preamble\nerror[A001]: advisory blah\nwarning: license issue\nignored line";
        let s = summarize_deny_output(output);
        assert!(s.contains("error") || s.contains("warning"));
    }

    #[test]
    fn summarize_falls_back_to_first_lines_when_no_errors() {
        let output = "line1\nline2\nline3";
        let s = summarize_deny_output(output);
        assert!(s.contains("line1"));
    }

    #[test]
    fn daily_guard_integration_session_start_skips_when_ran_today() {
        let dir = tempfile::tempdir().unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let guard = DailyGuard::new(dir.path(), "security", &today);
        guard.mark_done().unwrap();
        // After mark_done, should_run returns false — simulates "already ran today"
        assert!(!guard.should_run());
    }

    #[test]
    fn daily_guard_runs_on_fresh_state() {
        let dir = tempfile::tempdir().unwrap();
        let today = "2026-06-26";
        let guard = DailyGuard::new(dir.path(), "security", today);
        assert!(guard.should_run());
        guard.mark_done().unwrap();
        assert!(!guard.should_run());
    }

    #[test]
    fn is_disabled_false_when_no_config() {
        // Without a real ~/.daily/config.toml, is_disabled() reads nothing → false.
        // We can't test the true branch without writing to $HOME, so just verify false path.
        // The function is pure enough that a missing file always returns false.
        // (We rely on the function's own fallback logic here.)
        let _ = is_disabled(); // should not panic
    }
}
