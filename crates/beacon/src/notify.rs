//! Channel dispatch. Every delivery is best-effort and side-effect only: a
//! failing channel (no `curl`, no `osascript`, network down) is swallowed so
//! the hook can still exit 0. Network calls shell out to `curl` with a hard
//! `--max-time` rather than linking an HTTP stack, keeping the binary tiny.

use std::process::{Command, Stdio};

use serde_json::json;

use crate::config::Config;

/// A ready-to-send notification.
pub struct Note {
    /// "stop" or "attention" — machine-readable event class.
    pub event: &'static str,
    pub project: String,
    pub title: String,
    pub body: String,
}

/// Fan out to every configured channel. Returns the channels actually attempted.
pub fn dispatch(cfg: &Config, note: &Note) -> Vec<&'static str> {
    let mut sent = Vec::new();
    if cfg.desktop && desktop(cfg, note) {
        sent.push("desktop");
    }
    if let Some(url) = &cfg.slack_webhook {
        if slack(url, note) {
            sent.push("slack");
        }
    }
    if let Some(url) = &cfg.webhook {
        if webhook(url, note) {
            sent.push("webhook");
        }
    }
    if let Some(cmd) = &cfg.command {
        if run_command(cmd, note) {
            sent.push("command");
        }
    }
    sent
}

/// Desktop notification via the platform tool. Returns true if the tool was
/// spawned successfully (not whether the user saw it).
fn desktop(cfg: &Config, note: &Note) -> bool {
    match std::env::consts::OS {
        "macos" => {
            let mut script = format!(
                "display notification \"{}\" with title \"{}\"",
                applescript_escape(&note.body),
                applescript_escape(&note.title),
            );
            if let Some(sound) = &cfg.sound {
                script.push_str(&format!(" sound name \"{}\"", applescript_escape(sound)));
            }
            run_quiet(Command::new("osascript").arg("-e").arg(script))
        }
        "linux" => run_quiet(Command::new("notify-send").arg(&note.title).arg(&note.body)),
        _ => false,
    }
}

fn slack(url: &str, note: &Note) -> bool {
    let payload = json!({ "text": format!("{}\n{}", note.title, note.body) });
    curl_post_json(url, &payload.to_string())
}

fn webhook(url: &str, note: &Note) -> bool {
    let payload = json!({
        "event": note.event,
        "project": note.project,
        "title": note.title,
        "body": note.body,
    });
    curl_post_json(url, &payload.to_string())
}

// The user-configured escape-hatch command receives the notification fields as
// BEACON_* environment variables. Passing them via `.env()` (not by splicing
// into the command string) means the *values* are never re-parsed by the shell —
// a body of `"; rm -rf ~"` is just an env value. The command string itself is
// user-authored and trusted; a user who writes `echo $BEACON_BODY` unquoted is
// responsible for quoting, exactly as in any shell script.
fn run_command(cmd: &str, note: &Note) -> bool {
    run_quiet(
        harness_core::shell::command(cmd)
            .env("BEACON_EVENT", note.event)
            .env("BEACON_PROJECT", &note.project)
            .env("BEACON_TITLE", &note.title)
            .env("BEACON_BODY", &note.body),
    )
}

fn curl_post_json(url: &str, body: &str) -> bool {
    run_quiet(Command::new("curl").args([
        "-sS",
        "--max-time",
        "8",
        "-X",
        "POST",
        "-H",
        "Content-Type: application/json",
        "--data-binary",
        body,
        url,
    ]))
}

/// Run a command with stdio discarded; true if it spawned and exited 0.
fn run_quiet(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Escape a string for embedding inside an AppleScript double-quoted literal.
///
/// Backslash and double-quote are escaped so a crafted title/body cannot close
/// the literal and inject trailing AppleScript. Control characters are escaped
/// too: a raw newline inside an AppleScript `"..."` literal is a syntax error
/// (and would let a multi-line body break the one-line script), so `\n`/`\r`/`\t`
/// are turned into their AppleScript escape sequences. `$` and backticks are
/// *not* special here — the script is handed to `osascript -e` as a single argv
/// element, never parsed by a shell.
fn applescript_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(applescript_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn malicious_body_cannot_break_out_of_literal() {
        // A body that tries to close the quote, inject a newline, and run a
        // command must come back fully escaped: no raw newline, and the only
        // quotes present are backslash-escaped.
        let evil = "x\" & (do shell script \"whoami\")\n$(whoami)`id`";
        let esc = applescript_escape(evil);
        assert!(!esc.contains('\n'), "raw newline survived: {esc:?}");
        assert!(!esc.contains('\r'));
        // Every double-quote in the output is preceded by a backslash (escaped).
        let bytes = esc.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'"' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\\',
                    "unescaped quote at {i}: {esc:?}"
                );
            }
        }
        // `$` and backtick are passed through verbatim — harmless to osascript.
        assert!(esc.contains('$') && esc.contains('`'));
    }
}
