//! compass-side helpers over [`harness_core::discovery`] — the machine-scope
//! shared discovery store that lets concurrent compass/scout sessions avoid
//! re-surfacing a task another session already discovered.
//!
//! This module is the thin compass layer: session-id resolution, the
//! `record`/`select`/`list` command bodies, the opportunity-add discovery hook,
//! and the cross-session dedup filter. The persistence (append-only JSONL,
//! fail-soft) lives entirely in `harness_core::discovery`; we never reinvent it.
//!
//! Every path here is fail-soft and exits 0 on any internal error — these are
//! called from skills/hooks and must NEVER break a caller's turn.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use harness_core::discovery::{self, DiscoveryRecord, Status};

use crate::opportunity::Opportunity;

/// The env var carrying the active Claude Code session id. Empty/unset resolves
/// to a stable default so single-session use still records coherent rows.
const SESSION_ENV: &str = "CLAUDE_CODE_SESSION_ID";

/// Stable default session id when none is supplied (and the env is empty/unset).
const DEFAULT_SESSION: &str = "local";

/// Resolve the session id from the env (`CLAUDE_CODE_SESSION_ID`), falling back
/// to a stable default ([`DEFAULT_SESSION`]) when unset/blank. Fail-soft: a
/// missing or non-UTF8 env var degrades to the default rather than erroring.
pub fn session_from_env() -> String {
    std::env::var(SESSION_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

/// Resolve a session id: an explicit non-blank `arg` wins, else fall back to the
/// env-or-default ([`session_from_env`]).
fn resolve_session(arg: Option<String>) -> String {
    match arg {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => session_from_env(),
    }
}

/// Current wall-clock as unix seconds; 0 on a pre-epoch clock (fail-soft, never
/// panics).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append a `Discovered` row for `title` under `session`. Fail-soft — the
/// underlying [`discovery::append`] swallows any IO/serialization error. Shared
/// by the `record` command AND the opportunity-add hook.
pub fn append_discovered(cwd: &Path, title: &str, session: &str) {
    let rec = DiscoveryRecord {
        fingerprint: discovery::fingerprint(title),
        session_id: session.to_string(),
        status: Status::Discovered,
        created_at: now_unix(),
        title: title.to_string(),
    };
    discovery::append(cwd, &rec);
}

/// `compass discovery record`: append a `Discovered` row for `title`. Session
/// from `--session-id` else env-or-default. Fail-soft (store errors are
/// swallowed); the caller exits 0 regardless.
pub fn record_command(cwd: &Path, title: String, session_id: Option<String>) {
    let session = resolve_session(session_id);
    append_discovered(cwd, &title, &session);
}

/// `compass discovery select`: resolve a fingerprint (from `--fingerprint` else
/// `fingerprint(--title)`) and mark every matching row `Selected`. If neither is
/// given, print a short usage note to stderr and return — NEVER break the
/// caller's turn. Fail-soft.
pub fn select_command(cwd: &Path, fingerprint: Option<String>, title: Option<String>) {
    let Some(fp) = resolve_fingerprint(fingerprint, title) else {
        eprintln!("compass: discovery select needs --fingerprint or --title (no-op)");
        return;
    };
    discovery::mark_selected(cwd, &fp);
}

/// Resolve a fingerprint from `--fingerprint` (used verbatim if non-blank) else
/// from `fingerprint(--title)`. `None` iff neither yields a usable value.
fn resolve_fingerprint(fingerprint: Option<String>, title: Option<String>) -> Option<String> {
    if let Some(fp) = fingerprint {
        let fp = fp.trim();
        if !fp.is_empty() {
            return Some(fp.to_string());
        }
    }
    if let Some(title) = title {
        let title = title.trim();
        if !title.is_empty() {
            return Some(discovery::fingerprint(title));
        }
    }
    None
}

/// `compass discovery list`: load all rows (fail-soft; absent/corrupt store =>
/// empty). With `--json` print a JSON array (`[]` when empty); else human lines.
/// Always exits 0.
pub fn list_command(cwd: &Path, json: bool) -> Result<()> {
    let records = discovery::load(cwd);
    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
    } else if records.is_empty() {
        println!("(no discovery records)");
    } else {
        for r in &records {
            let status = match r.status {
                Status::Discovered => "discovered",
                Status::Selected => "selected",
            };
            println!(
                "- [{}] {} \"{}\" (session {})",
                r.fingerprint, status, r.title, r.session_id
            );
        }
    }
    Ok(())
}

/// Drop opportunities whose title was already `Discovered` by ANOTHER session
/// (cross-session dedup, DoD#2). The single canonical place this dedup lives,
/// called at both surface points (`gap` and the `route` handoff).
///
/// Fail-soft / byte-equivalent fallback (DoD#4): an absent/empty/corrupt store
/// makes [`discovery::already_discovered_by_other`] return false for everything,
/// so the filter drops nothing and the surfaced set/order is unchanged.
pub fn filter_undiscovered_by_others(
    cwd: &Path,
    my_session: &str,
    opportunities: Vec<Opportunity>,
) -> Vec<Opportunity> {
    opportunities
        .into_iter()
        .filter(|o| {
            let fp = discovery::fingerprint(&o.title);
            !discovery::already_discovered_by_other(cwd, &fp, my_session)
        })
        .collect()
}
