//! `ctxrot preguard` — PreToolUse hook (preventive gate).
//!
//! Where `toolguard` (PostToolUse) nudges *after* a huge payload already landed
//! in context, this fires *before* the load and can actually stop it. It targets
//! the one rot vector we can measure ahead of time: a `Read` of a pathologically
//! large LOCAL file with no `limit`. Those are almost always logs / data dumps /
//! minified blobs — exactly what should never enter main context whole.
//!
//! Policy (deliberately narrow to avoid false positives on normal source files):
//!   * Only `Read`.                     (URLs can't be sized before fetch.)
//!   * Only when `limit` is absent.     (An explicit slice = the model is careful.)
//!   * Only at/above `gate_file_bytes`  (default 1MB; 0 disables the gate).
//!
//! Everything else defers to the normal permission flow (we stay silent).
//!
//! PreToolUse cannot inject additionalContext, so the deny *reason* is the only
//! steering channel — we make it actionable (route via sub-agent, or re-Read a
//! bounded slice).

use crate::config::Config;
use crate::model::HookInput;

/// Returns a deny reason (the model is told to reroute), or None to stay silent
/// and let the normal permission flow proceed.
pub fn run(input: &HookInput, cfg: &Config) -> Option<String> {
    if cfg.gate_file_bytes == 0 || input.tool_name != "Read" {
        return None;
    }
    let ti = input.tool_input.as_ref()?;

    // An explicit `limit` means the model is already bounding the read — never
    // gate those (reading 50 lines out of a 2MB file is fine).
    if ti.get("limit").is_some() {
        return None;
    }
    let raw_path = ti.get("file_path").and_then(|v| v.as_str())?;

    let expanded = crate::config::expand_tilde(raw_path);
    let path = if expanded.is_absolute() {
        expanded
    } else {
        input.cwd_or_current().join(&expanded)
    };

    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() < cfg.gate_file_bytes {
        return None;
    }

    let mb = meta.len() as f64 / 1_048_576.0;
    let tok = meta.len() / 4;
    Some(format!(
        "[context-rot guard] このファイルは ~{mb:.1}MB（推定~{tok}tok）で、全文を main context に \
         載せると rot の原因になります。次のいずれかで読み直してください: \
         (1) Explore か general-purpose sub-agent に読ませ、該当行・要約・結論だけ受け取る \
         (2) 本当に直接読むなら Read に offset/limit を付けて必要な範囲だけに絞る。 \
         （巨大な生データを丸ごと本文に入れない方針です）"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn big_temp_file(name: &str, bytes: usize) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ctxrot-preguard-{}-{}", std::process::id(), name));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(&vec![b'x'; bytes]).unwrap();
        p
    }

    fn read_input(ti: serde_json::Value) -> HookInput {
        HookInput {
            tool_name: "Read".into(),
            tool_input: Some(ti),
            ..Default::default()
        }
    }

    #[test]
    fn denies_huge_unbounded_read() {
        let cfg = Config::default();
        let p = big_temp_file("huge.log", 1_200_000);
        let out = run(&read_input(json!({ "file_path": p.to_string_lossy() })), &cfg);
        assert!(out.unwrap().contains("sub-agent"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn allows_when_limit_is_set() {
        let cfg = Config::default();
        let p = big_temp_file("huge2.log", 1_200_000);
        let out = run(
            &read_input(json!({ "file_path": p.to_string_lossy(), "limit": 50 })),
            &cfg,
        );
        assert!(out.is_none(), "an explicit limit must bypass the gate");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn allows_normal_sized_file() {
        let cfg = Config::default();
        let p = big_temp_file("small.rs", 60_000); // > large_file_bytes but << gate
        let out = run(&read_input(json!({ "file_path": p.to_string_lossy() })), &cfg);
        assert!(out.is_none(), "a 60KB source file must not be gated");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn gate_disabled_when_zero() {
        let cfg = Config {
            gate_file_bytes: 0,
            ..Config::default()
        };
        let p = big_temp_file("huge3.log", 1_200_000);
        let out = run(&read_input(json!({ "file_path": p.to_string_lossy() })), &cfg);
        assert!(out.is_none(), "gate_file_bytes=0 disables the gate");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ignores_other_tools() {
        let cfg = Config::default();
        let mut input = read_input(json!({ "file_path": "/etc/hosts" }));
        input.tool_name = "Bash".into();
        assert!(run(&input, &cfg).is_none());
    }
}
