//! Hook plumbing: stdin protocol, state files, audit log. These conventions
//! follow Claude Code's hook contract but are inert when the tool is run as a
//! plain pre-commit-framework hook (no stdin JSON, no review artifact).

use std::io::Write;
use std::path::{Path, PathBuf};

/// Parsed subset of the Claude Code Stop-hook stdin payload.
pub struct HookInput {
    pub stop_hook_active: bool,
    /// `hook_event_name` from the payload (e.g. "Stop", "SessionEnd"). Empty
    /// when absent (e.g. a plain pre-commit-framework invocation with no JSON).
    pub event: String,
}

/// Read and parse stdin JSON. Returns a default (non-recursive) input when
/// stdin is empty or unparseable, so non-Claude invocations Just Work.
pub fn read_stdin() -> HookInput {
    let raw = harness_core::hook::read_stdin();
    if raw.trim().is_empty() {
        return HookInput {
            stop_hook_active: false,
            event: String::new(),
        };
    }
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => HookInput {
            stop_hook_active: v
                .get("stop_hook_active")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            event: v
                .get("hook_event_name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        },
        Err(_) => HookInput {
            stop_hook_active: false,
            event: String::new(),
        },
    }
}

/// One-shot escape hatch: `<audit_dir>/.audit-skip`. If present, consume it
/// (delete), clear the block marker, and return the reason string.
pub fn consume_skip(root: &Path, audit_dir: &str) -> Option<String> {
    let skip = root.join(audit_dir).join(".audit-skip");
    if !skip.exists() {
        return None;
    }
    let reason = std::fs::read_to_string(&skip)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no reason given)".to_string());
    let _ = std::fs::remove_file(&skip);
    let _ = std::fs::remove_file(block_marker(root, audit_dir));
    Some(reason)
}

pub fn block_marker(root: &Path, audit_dir: &str) -> PathBuf {
    root.join(audit_dir).join(".audit-blocked")
}

/// Create the block marker (best effort; parent dir must already exist).
pub fn set_block_marker(root: &Path, audit_dir: &str) {
    let p = block_marker(root, audit_dir);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&p, b"");
}

pub fn clear_block_marker(root: &Path, audit_dir: &str) {
    let _ = std::fs::remove_file(block_marker(root, audit_dir));
}

/// Append a JSONL entry to `<audit_dir>/audit-log.jsonl`. Best effort.
pub fn write_audit_log(
    root: &Path,
    audit_dir: &str,
    mode: &str,
    verdict: &str,
    issue_count: usize,
    categories: &[String],
    warning_count: usize,
    changed_count: usize,
    timestamp: &str,
) {
    let entry = serde_json::json!({
        "ts": timestamp,
        "event": "pre-commit-audit",
        "mode": mode,
        "verdict": verdict,
        "issueCount": issue_count,
        "issueCategories": categories,
        "warningCount": warning_count,
        "changedCount": changed_count,
    });
    let path = root.join(audit_dir).join("audit-log.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(&entry) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}
