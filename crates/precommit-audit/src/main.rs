//! precommit-audit — a config-driven, cross-platform pre-commit static audit.
//!
//! Operates in two modes (like the PowerShell original it replaces):
//!   stop      — Claude Code Stop hook. Honors the subagent review contract.
//!               Exits 2 to feed findings back to the agent.
//!   precommit — pre-commit-framework / git hook on a human commit. Skips the
//!               review contract. Exits 1 on failure (pre-commit convention).
//!
//! Generic checks are built in; project-specific policy lives in a TOML config
//! (default `.precommit-audit.toml` at the repo root). No project name, path, or
//! rule is hard-coded into the binary.

mod checks;
mod classify;
mod config;
mod git;
mod hookio;
mod model;

use checks::Ctx;
use classify::Classifier;
use config::{Config, Severity};
use std::path::{Path, PathBuf};
use std::process::exit;

struct Args {
    config: Option<PathBuf>,
    mode: Option<String>,
    root: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args {
        config: None,
        mode: None,
        root: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => a.config = it.next().map(PathBuf::from),
            "--mode" => a.mode = it.next(),
            "--root" => a.root = it.next().map(PathBuf::from),
            "--version" | "-V" => {
                println!("precommit-audit {}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            "--help" | "-h" => {
                print_help();
                exit(0);
            }
            other => {
                eprintln!("precommit-audit: unknown argument '{other}' (try --help)");
                exit(64);
            }
        }
    }
    a
}

fn print_help() {
    println!(
        "precommit-audit {ver}\n\
Config-driven pre-commit static audit (cross-platform).\n\n\
USAGE:\n  precommit-audit [--mode stop|precommit] [--config <file>] [--root <dir>]\n\n\
OPTIONS:\n  --mode <m>     stop (default) or precommit\n  --config <f>   config file (default: <root>/.precommit-audit.toml)\n  --root <d>     repo root (default: $CLAUDE_PROJECT_DIR, else git toplevel)\n  -V, --version  print version\n  -h, --help     this help\n\n\
EXIT: 0 clean | 1 blocked (precommit) | 2 blocked (stop)",
        ver = env!("CARGO_PKG_VERSION")
    );
}

fn resolve_root(arg: Option<PathBuf>) -> PathBuf {
    if let Some(r) = arg {
        return r;
    }
    if let Ok(p) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(top) = git::toplevel(&cwd) {
        return PathBuf::from(top);
    }
    cwd
}

fn resolve_mode(arg: Option<String>) -> String {
    arg.or_else(|| std::env::var("AUDIT_MODE").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "stop".to_string())
}

fn main() {
    let args = parse_args();

    // Recursion guard: a Stop hook that re-fires within the same stop cycle.
    let hook = hookio::read_stdin();
    if hook.stop_hook_active {
        exit(0);
    }

    let root = resolve_root(args.root);
    let mode = resolve_mode(args.mode);
    // Exit code that signals "blocking issues found". On the Stop hook, 2 blocks
    // the stop; on a git pre-commit invocation, 1 aborts the commit. But under
    // SessionEnd (where this hook now runs as a side effect) a non-zero exit
    // CANNOT block — Claude Code reports it as a *failed hook*. So when invoked
    // from SessionEnd we still run the audit (markers + log are useful), but
    // never surface a blocking exit code.
    let fail_exit = if hook.event == "SessionEnd" {
        0
    } else if mode == "precommit" {
        1
    } else {
        2
    };

    let config_path = args
        .config
        .unwrap_or_else(|| root.join(".precommit-audit.toml"));
    let cfg = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("precommit-audit: {e}");
            exit(64);
        }
    };

    // One-shot skip escape hatch.
    if let Some(reason) = hookio::consume_skip(&root, &cfg.audit_dir) {
        eprintln!("pre-commit audit SKIPPED (one-shot) -- reason: {reason}");
        exit(0);
    }

    let mut files = git::changed_and_untracked(&root);
    // Never audit our own config file: it literally contains the rule patterns,
    // which would otherwise self-trigger on the commit that introduces it.
    if let Ok(rel) = config_path.strip_prefix(&root) {
        let cfg_rel = classify::norm(&rel.to_string_lossy());
        files.retain(|f| classify::norm(f) != cfg_rel);
    }
    if files.is_empty() {
        exit(0);
    }

    let classifier = Classifier::new(&cfg.classify);
    let ctx = Ctx {
        root: &root,
        cfg: &cfg,
        cls: &classifier,
        files: &files,
    };

    let mut issues: Vec<model::Issue> = Vec::new();
    checks::run_static_checks(&ctx, &mut issues);
    if cfg.checks.linters {
        checks::linters::run(&ctx, &mut issues);
    }
    if mode == "stop" {
        if let Some(i) = checks::review::check(&ctx) {
            issues.push(i);
        }
    }

    emit_and_exit(&root, &cfg, &mode, fail_exit, &files, issues);
}

fn emit_and_exit(
    root: &Path,
    cfg: &Config,
    mode: &str,
    fail_exit: i32,
    files: &[String],
    issues: Vec<model::Issue>,
) {
    let (blocking, warnings): (Vec<_>, Vec<_>) =
        issues.into_iter().partition(|i| i.severity == Severity::Block);

    // Advisory warnings always print, never affect the exit code.
    if !warnings.is_empty() {
        let body = warnings
            .iter()
            .map(|w| w.message.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        eprintln!("=== pre-commit audit WARNING (non-blocking) ===\n\n{body}");
    }

    let ts = now_iso8601();
    if blocking.is_empty() {
        hookio::clear_block_marker(root, &cfg.audit_dir);
        hookio::write_audit_log(
            root,
            &cfg.audit_dir,
            mode,
            "pass",
            0,
            &[],
            warnings.len(),
            files.len(),
            &ts,
        );
        exit(0);
    }

    hookio::set_block_marker(root, &cfg.audit_dir);
    let mut cats: Vec<String> = blocking.iter().map(|i| i.category.clone()).collect();
    cats.sort();
    cats.dedup();
    hookio::write_audit_log(
        root,
        &cfg.audit_dir,
        mode,
        "block",
        blocking.len(),
        &cats,
        warnings.len(),
        files.len(),
        &ts,
    );

    let body = blocking
        .iter()
        .map(|i| i.message.clone())
        .collect::<Vec<_>>()
        .join("\n\n");
    let header = if fail_exit == 0 {
        // SessionEnd: advisory only — a non-zero exit can't block here.
        "=== pre-commit audit found blocking issues (advisory; session ended) ==="
    } else if mode == "precommit" {
        "=== pre-commit audit BLOCKED the git commit ==="
    } else {
        "=== pre-commit audit BLOCKED the auto-commit ==="
    };
    eprintln!(
        "{header}\n\n{body}\n\n\
Required actions:\n  1. Fix the code or add the missing test for each issue above.\n  2. Or suppress with a reasoned marker (reason REQUIRED):\n       Per-line:  append  '# audit-ignore: <reason>'  (use // for JS/TS)\n       Per-file:  add     'audit-ignore-file: <reason>'  in the first 20 lines\n  3. Re-run to re-check."
    );
    exit(fail_exit);
}

/// UTC ISO-8601 timestamp (e.g. 2026-06-12T10:30:00Z) without pulling in a date
/// crate. Uses Howard Hinnant's civil-from-days algorithm.
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}
