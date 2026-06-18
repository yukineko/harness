//! specguard — project-agnostic spec/implementation drift audit harness.
//!
//! Reads a TOML config describing a project's canon (areas, canon pointers,
//! invariants), resolves a change-triggered scope from `git diff`, renders a
//! read-only audit prompt, drives an LLM agent to produce findings, and
//! persists a report plus a sentinel when something needs human review.
//!
//! The judgment lives in the agent (it reads the live canon and quotes it
//! verbatim); this binary is the deterministic harness around it.

mod agent;
mod config;
mod decision;
mod init;
mod parse;
mod prompt;
mod ratify;
mod report;
mod scope;
mod verify;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes (stable contract for schedulers/hooks). These are specguard's own
/// and must stay disjoint from anything the agent might return: an agent failure
/// always maps to [`EXIT_AGENT_FAILED`] (its real code goes to stderr) rather
/// than being propagated raw, so a caller can never confuse "agent exited 3"
/// with "no marker".
const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_NO_MARKER: u8 = 3;
const EXIT_AGENT_FAILED: u8 = 4;
/// The prompt (meta-canon) is unratified or changed since ratification, and
/// `[prompt].require_ratification` is on — a human must `accept-prompt` first.
const EXIT_UNRATIFIED: u8 = 5;

#[derive(Parser)]
#[command(name = "specguard", version, about = "Spec/implementation drift audit harness")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "specguard.toml", global = true)]
    config: PathBuf,

    /// Override the baseline ref (also via SPECGUARD_BASELINE_REF).
    #[arg(short, long, global = true)]
    baseline: Option<String>,

    /// Override the run date (YYYY-MM-DD); default is today (also SPECGUARD_NOW).
    #[arg(long, global = true)]
    date: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the full audit (default).
    Run,
    /// Print the resolved scope only (no agent call).
    Scope,
    /// Print the rendered per-shard prompts only (no agent call).
    Prompt,
    /// Clear the sentinel after a human has handled the pending findings.
    Ack,
    /// Scaffold specguard into a repo: starter config + Claude Code SessionStart
    /// hook. Idempotent; existing config kept unless `--force`.
    Init {
        /// Overwrite an existing config file.
        #[arg(long)]
        force: bool,
    },
    /// Scaffold a decision record (ADR) pinned to the current canon commit.
    Decide {
        /// Title of the decision (becomes part of the record id).
        title: String,
        /// Overwrite an existing record with the same id.
        #[arg(long)]
        force: bool,
    },
    /// Ratify the prompt templates (meta-canon) after reviewing them: contract-
    /// check, then pin the current version with a rationale. Required before a
    /// gated `run` when `[prompt].require_ratification = true`.
    AcceptPrompt {
        /// Why this prompt version is accepted (recorded in the lock).
        #[arg(short = 'm', long = "reason")]
        reason: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("specguard: error: {e:#}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

struct Loaded {
    cfg: Config,
    repo_root: PathBuf,
    template: String,
    date: String,
}

/// Load config + resolve repo root, template and date (shared by all commands).
fn load(cli: &Cli) -> Result<Loaded> {
    let cfg = Config::load(&cli.config)?;
    let config_dir = cli
        .config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let repo_root = canonicalize(&config_dir.join(&cfg.project.root))
        .with_context(|| "resolving project.root")?;
    let template = load_template(&cfg, config_dir)?;
    let date = resolve_date(cli.date.clone());
    Ok(Loaded {
        cfg,
        repo_root,
        template,
        date,
    })
}

fn run(cli: &Cli) -> Result<u8> {
    // `init` runs before config load: the config may not exist yet.
    if let Some(Command::Init { force }) = &cli.command {
        init::run(&cli.config, *force)?;
        return Ok(EXIT_OK);
    }

    let l = load(cli)?;
    let paths = report::paths(&l.cfg, &l.repo_root, &l.date);

    // `ack` only touches the sentinel; no scope/agent work needed.
    if let Some(Command::Ack) = cli.command {
        return ack(&paths);
    }

    // `decide` scaffolds a decision record pinned to the current canon commit.
    if let Some(Command::Decide { title, force }) = &cli.command {
        return decide(&l, title, *force);
    }

    // `accept-prompt` ratifies the prompt (meta-canon): contract-check + pin.
    if let Some(Command::AcceptPrompt { reason }) = &cli.command {
        return accept_prompt(&l, reason);
    }

    let last_ref = report::read_last_ref(&paths);
    let override_ref = cli
        .baseline
        .clone()
        .or_else(|| std::env::var("SPECGUARD_BASELINE_REF").ok());

    let scope = scope::resolve(&l.cfg, &l.repo_root, override_ref.as_deref(), last_ref.as_deref())?;
    let shards = prompt::shards(&l.cfg, &scope);

    match cli.command {
        Some(Command::Scope) => {
            print_scope(&l.cfg, &scope);
            return Ok(EXIT_OK);
        }
        Some(Command::Prompt) => {
            print_prompts(&l, &scope, &shards);
            return Ok(EXIT_OK);
        }
        Some(Command::Ack)
        | Some(Command::Init { .. })
        | Some(Command::Decide { .. })
        | Some(Command::AcceptPrompt { .. }) => unreachable!("handled above"),
        Some(Command::Run) | None => {}
    }

    // Ratification gate: when the prompt (meta-canon) must be ratified, refuse to
    // audit with an unratified or changed prompt until a human `accept-prompt`s.
    if l.cfg.prompt.require_ratification {
        let hashes = current_hashes(&l);
        match ratify::read_lock(&l.repo_root) {
            None => {
                eprintln!(
                    "specguard: prompt (メタ正典) が未批准です。内容を確認し、\n  `specguard accept-prompt -m \"理由\"` で批准してください。"
                );
                return Ok(EXIT_UNRATIFIED);
            }
            Some(lock) => {
                let drift = ratify::drifted(
                    &lock,
                    &hashes,
                    l.cfg.verify.enabled,
                    l.cfg.verify.completeness,
                );
                if !drift.is_empty() {
                    eprintln!(
                        "specguard: prompt (メタ正典) に未批准の変更があります: {}\n  内容を確認し、合意できるなら `specguard accept-prompt -m \"理由\"` で再批准してください。",
                        drift.join(", ")
                    );
                    return Ok(EXIT_UNRATIFIED);
                }
                // Surface which ratified policy version is in force.
                eprintln!(
                    "specguard: prompt 批准済み (date {}, canon {}) 理由: {}",
                    lock.date, lock.canon_commit, lock.reason
                );
            }
        }
    }

    let head = scope::current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());

    // Nothing in scope and no invariants: record progress without an agent call.
    if shards.is_empty() {
        let body = format!(
            "# {} 仕様↔実装 整合監査 {}\n\n## スコープ\n- baseline: {}\n- canon commit (HEAD): {}\n- in-scope 領域: なし / 不変条件: なし / 決定ログ: なし\n\n## findings\n監査対象なし。\n",
            l.cfg.project.name, l.date, scope.baseline, head
        );
        report::write_report(&paths, &body)?;
        if report::sentinel_pending(&paths) {
            println!(
                "specguard: 監査対象なし。ただし未処理 sentinel ({}) があるため baseline を据え置く (`specguard ack` で解除)",
                paths.sentinel.display()
            );
        } else {
            report::advance_baseline(&paths, &head)?;
            println!("specguard: 監査対象なし (report: {})", paths.report.display());
        }
        return Ok(EXIT_OK);
    }

    eprintln!(
        "specguard: auditing {} (baseline {}, {} shard(s): {} area(s) + {} invariant + {} decision)",
        l.cfg.project.name,
        scope.baseline,
        shards.len(),
        scope.in_scope.len(),
        if l.cfg.invariants.is_empty() { 0 } else { 1 },
        if scope.decision_files.is_empty() { 0 } else { 1 },
    );

    let shard_prompts: Vec<agent::ShardPrompt> = shards
        .iter()
        .map(|&sh| agent::ShardPrompt {
            label: prompt::shard_label(&l.cfg, &scope, sh),
            prompt: prompt::render_shard(&l.template, &l.cfg, &scope, sh, &l.date),
        })
        .collect();

    let outs = agent::run_shards(&l.cfg.agent, &l.repo_root, shard_prompts);

    // B: any shard whose agent failed -> a single EXIT_AGENT_FAILED, with each
    // real exit code on stderr. Never propagate raw (would collide with 2/3).
    let failed: Vec<&agent::ShardOutput> = outs.iter().filter(|o| o.out.code != 0).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!(
                "specguard: shard '{}' agent exited with code {}\n--- agent stderr ---\n{}",
                f.label,
                f.out.code,
                f.out.stderr.trim_end()
            );
        }
        return Ok(EXIT_AGENT_FAILED);
    }

    let parsed: Vec<(String, parse::Parsed)> = outs
        .iter()
        .map(|o| (o.label.clone(), parse::parse(&o.out.stdout)))
        .collect();

    // If any shard omitted the marker, the audit is incomplete: save the merged
    // report for inspection but do NOT advance the baseline, raise a sentinel, or
    // run verification (which needs complete audit output to refine).
    let missing: Vec<&str> = parsed
        .iter()
        .filter(|(_, p)| !p.marker_found)
        .map(|(l, _)| l.as_str())
        .collect();
    if !missing.is_empty() {
        let merged = merge_report(&l.cfg, &scope, &l.date, &head, &parsed);
        if let Some(dir) = paths.report.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&paths.report, &merged)
            .with_context(|| format!("writing report {}", paths.report.display()))?;
        eprintln!(
            "specguard: WARN marker '{}' missing in shard(s): {}; saved {} but cannot assess findings",
            parse::MARKER,
            missing.join(", "),
            paths.report.display()
        );
        return Ok(EXIT_NO_MARKER);
    }

    // Verification gates (opt-in): refute false positives (V1) and surface missed
    // rules (V2). A pure transform over the parsed shards so the merge/sentinel
    // logic below is unchanged. See DESIGN-VERIFY.md.
    let parsed = if l.cfg.verify.enabled || l.cfg.verify.completeness {
        verify::apply(&l.cfg, &l.repo_root, &scope, &shards, &l.date, parsed)
    } else {
        parsed
    };

    let merged = merge_report(&l.cfg, &scope, &l.date, &head, &parsed);
    report::write_report(&paths, &merged)?;

    let report_rel = format!("{}/{}.md", l.cfg.output.report_dir, l.date);
    let needs_user = parsed.iter().any(|(_, p)| p.needs_user);
    if needs_user {
        // Findings: raise the sentinel and HOLD the baseline. Not advancing keeps
        // the unfixed drift in the next run's diff so it is re-detected; a human
        // releases it with `specguard ack` once handled.
        let summary = merge_summary(&parsed);
        report::write_sentinel(&paths, &l.date, &report_rel, &summary)?;
        println!(
            "specguard: 修正候補あり -> {} (report: {}); baseline は据え置き (ack するまで再検出)",
            paths.sentinel.display(),
            paths.report.display()
        );
    } else if report::sentinel_pending(&paths) {
        // Clean this run, but a prior run's sentinel is still unhandled: keep the
        // baseline put so its drift stays in scope. Leave the sentinel untouched.
        println!(
            "specguard: 修正候補なし。ただし未処理 sentinel ({}) が残るため baseline を据え置く (`specguard ack` で解除)",
            paths.sentinel.display()
        );
    } else {
        // Fully clean: advance the baseline.
        report::advance_baseline(&paths, &head)?;
        println!("specguard: 修正候補なし (report: {})", paths.report.display());
    }
    Ok(EXIT_OK)
}

/// Clear the sentinel (C). Idempotent: succeeds whether or not one was present.
fn ack(paths: &report::Paths) -> Result<u8> {
    match std::fs::remove_file(&paths.sentinel) {
        Ok(()) => println!("specguard: sentinel をクリアした ({})", paths.sentinel.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("specguard: sentinel は無い ({})", paths.sentinel.display());
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("removing sentinel {}", paths.sentinel.display()));
        }
    }
    Ok(EXIT_OK)
}

/// Scaffold a decision record (ADR) pinned to the current canon commit.
fn decide(l: &Loaded, title: &str, force: bool) -> Result<u8> {
    let head = scope::current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());
    let id = format!("{}-{}", l.date, decision::slug(title));
    let path = decision::scaffold(
        &l.repo_root,
        &l.cfg.decisions.dir,
        &id,
        title,
        &l.date,
        &head,
        force,
    )?;
    println!("specguard: 決定ログを生成 -> {}", path.display());
    println!("  canon commit に pin 済み (canon_commit: {head})");
    println!("  次: `canon:` に支配する canon ポインタ、`drivers:` に反証可能な理由、`review_when:` を記入");
    Ok(EXIT_OK)
}

/// Ratify the prompt templates (meta-canon): contract-check then pin the
/// current version with a rationale. This is the consent ceremony — it confers
/// canon authority on the prompt version, recorded against the canon commit.
fn accept_prompt(l: &Loaded, reason: &str) -> Result<u8> {
    if reason.trim().is_empty() {
        anyhow::bail!("批准には理由が必要です (-m \"...\")");
    }
    // Contract judgment: the templates must not contradict the render/parse
    // contract (required placeholders present). The policy/constitution part is
    // the human's responsibility, recorded as the rationale. Verify templates are
    // contract-checked only when their gate is active (consent is scoped to the
    // live policy surface; see ratify::drifted).
    let mut violations: Vec<(&str, Vec<&'static str>)> = vec![
        ("audit-prompt", prompt::missing_placeholders(&l.template, prompt::AUDIT_PLACEHOLDERS)),
        (
            "decisions-prompt",
            prompt::missing_placeholders(prompt::DECISIONS_TEMPLATE, prompt::DECISIONS_PLACEHOLDERS),
        ),
    ];
    if l.cfg.verify.enabled {
        violations.push((
            "refute-prompt",
            prompt::missing_placeholders(prompt::REFUTE_TEMPLATE, prompt::REFUTE_PLACEHOLDERS),
        ));
    }
    if l.cfg.verify.completeness {
        violations.push((
            "completeness-prompt",
            prompt::missing_placeholders(
                prompt::COMPLETENESS_TEMPLATE,
                prompt::COMPLETENESS_PLACEHOLDERS,
            ),
        ));
    }
    if violations.iter().any(|(_, m)| !m.is_empty()) {
        let mut msg = String::from("prompt が契約に矛盾 (必須 placeholder 不足); 批准を拒否:");
        for (name, miss) in &violations {
            if !miss.is_empty() {
                msg.push_str(&format!("\n  {name}: {}", miss.join(", ")));
            }
        }
        anyhow::bail!(msg);
    }

    let head = scope::current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());
    // Pin only the live policy: a gate that is off leaves its slot empty, so
    // turning it on later registers as drift and demands a fresh ratification.
    let mut hashes = current_hashes(l);
    if !l.cfg.verify.enabled {
        hashes.refute = String::new();
    }
    if !l.cfg.verify.completeness {
        hashes.completeness = String::new();
    }
    let path = ratify::write_lock(&l.repo_root, &hashes, &head, &l.date, reason)?;
    println!("specguard: prompt (メタ正典) を批准した -> {}", path.display());
    println!("  canon commit に pin (canon_commit: {head})");
    let mut pinned = vec!["audit", "decisions"];
    if l.cfg.verify.enabled {
        pinned.push("refute");
    }
    if l.cfg.verify.completeness {
        pinned.push("completeness");
    }
    println!("  pin したポリシー: {}", pinned.join(", "));
    println!("  理由: {reason}");
    Ok(EXIT_OK)
}

/// Fingerprints of all four prompt templates (meta-canon) as they stand now.
/// Used by the ratification gate; `ratify::drifted` decides which to enforce.
fn current_hashes(l: &Loaded) -> ratify::TemplateHashes {
    ratify::TemplateHashes {
        audit: ratify::hash(&l.template),
        decisions: ratify::hash(prompt::DECISIONS_TEMPLATE),
        refute: ratify::hash(prompt::REFUTE_TEMPLATE),
        completeness: ratify::hash(prompt::COMPLETENESS_TEMPLATE),
    }
}

/// Print every shard's rendered prompt (debug view for `specguard prompt`).
fn print_prompts(l: &Loaded, scope: &scope::Scope, shards: &[prompt::Shard]) {
    if shards.is_empty() {
        println!("(in-scope 領域なし・不変条件なし — 監査対象の shard がない)");
        return;
    }
    for (i, &sh) in shards.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("===== shard: {} =====", prompt::shard_label(&l.cfg, scope, sh));
        print!("{}", prompt::render_shard(&l.template, &l.cfg, scope, sh, &l.date));
    }
}

/// Assemble the merged human-readable report: a harness-built header + overall
/// scope, then each shard's body verbatim under a divider.
fn merge_report(
    cfg: &Config,
    scope: &scope::Scope,
    date: &str,
    head: &str,
    parsed: &[(String, parse::Parsed)],
) -> String {
    let labels: Vec<&str> = parsed.iter().map(|(l, _)| l.as_str()).collect();
    let mut out = format!("# {} 仕様↔実装 整合監査 {}\n\n", cfg.project.name, date);
    out.push_str("## スコープ (全体)\n");
    out.push_str(&format!("- baseline: `{}`", scope.baseline));
    if scope.fell_back {
        out.push_str(" (fallback)");
    }
    out.push('\n');
    // Provenance: the canon commit this verdict was judged against, so a past
    // report is reproducible and B/C classification has a temporal anchor.
    out.push_str(&format!("- canon commit (HEAD): `{head}`\n"));
    out.push_str(&format!("- 変更ファイル数: {}\n", scope.changed_files.len()));
    out.push_str(&format!("- shard: {}\n", join(&labels.iter().map(|s| s.to_string()).collect::<Vec<_>>())));
    out.push_str("- 各 shard は独立した read-only エージェント (fresh context) で監査し、ここに統合した。\n");
    for (label, p) in parsed {
        out.push_str(&format!("\n---\n\n## shard: {label}\n\n"));
        out.push_str(p.report.trim_end());
        out.push('\n');
    }
    out
}

/// Merge the sentinel summary across shards that flagged `needs_user`. A single
/// flagged shard contributes its summary verbatim; multiple shards are labelled.
fn merge_summary(parsed: &[(String, parse::Parsed)]) -> String {
    let flagged: Vec<(&str, &str)> = parsed
        .iter()
        .filter(|(_, p)| p.needs_user && !p.summary.trim().is_empty())
        .map(|(l, p)| (l.as_str(), p.summary.trim()))
        .collect();
    match flagged.as_slice() {
        [] => String::new(),
        [(_, s)] => s.to_string(),
        many => many
            .iter()
            .map(|(l, s)| format!("[{l}] {s}"))
            .collect::<Vec<_>>()
            .join(" / "),
    }
}

fn print_scope(cfg: &Config, scope: &scope::Scope) {
    println!("baseline: {}", scope.baseline);
    if scope.fell_back {
        println!("(baseline fell back from configured/recorded ref)");
    }
    println!("changed files: {}", scope.changed_files.len());
    println!("in-scope areas:");
    if scope.in_scope.is_empty() {
        println!("  (none)");
    }
    for hit in &scope.in_scope {
        let canon_note = if hit.changed_canon.is_empty() {
            String::new()
        } else {
            format!(", canon changed: {}", hit.changed_canon.len())
        };
        println!(
            "  - {} ({} file(s){})",
            cfg.areas[hit.area_index].name,
            hit.matched_files.len(),
            canon_note
        );
    }
    println!("skipped areas: {}", join(&scope.skipped_areas));
    println!(
        "invariants (always): {}",
        join(&cfg.invariants.iter().map(|i| i.name.clone()).collect::<Vec<_>>())
    );
    println!("decision records (D3): {}", scope.decision_files.len());
}

fn join(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}

/// Resolve the template text: explicit `[prompt].template` path, else embedded.
fn load_template(cfg: &Config, config_dir: &Path) -> Result<String> {
    if cfg.prompt.template.trim().is_empty() {
        Ok(prompt::DEFAULT_TEMPLATE.to_string())
    } else {
        let p = config_dir.join(&cfg.prompt.template);
        std::fs::read_to_string(&p)
            .with_context(|| format!("reading prompt template {}", p.display()))
    }
}

fn resolve_date(cli_date: Option<String>) -> String {
    if let Some(d) = cli_date {
        return d;
    }
    if let Ok(d) = std::env::var("SPECGUARD_NOW") {
        if !d.trim().is_empty() {
            return d.trim().to_string();
        }
    }
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Canonicalize a path, falling back to the joined path if it doesn't exist yet.
fn canonicalize(p: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(p).or_else(|_| Ok(p.to_path_buf()))
}
