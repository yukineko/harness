//! specforge — the generation-side harness (sibling to specguard, DESIGN.md).
//!
//! This binary implements the *entry gate* of the generation pipeline: ②normalize
//! a (possibly coarse) requirement into an Agent-executable Spec IR, gated by the
//! §5.3 rigor pre-flight (G1 接地 / G2 沈黙ゼロ / G3 矛盾ゼロ / G4 反証可能).
//!
//! HOTL: the judgment (can this be rigorously specified? quote the canon) lives
//! in the agent; this binary is the deterministic harness. When the rigor gate
//! is not met the harness does NOT fabricate a draft — it raises a sentinel and
//! pulls the human in (escalation). A draft becomes canon only via `ratify`
//! (human consent). The human sits at the boundaries; the middle is mechanical.

mod agent;
mod config;
mod ir;
mod parse;
mod prompt;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes — kept disjoint from anything the agent returns, mirroring
/// specguard so a scheduler/hook sees one stable contract across both tools.
const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_NO_MARKER: u8 = 3;
const EXIT_AGENT_FAILED: u8 = 4;
/// Reserved: a future `implement` refuses an unratified/changed spec (DESIGN.md
/// §5, §9-3). Defined here so the contract stays disjoint as the pipeline grows.
#[allow(dead_code)]
const EXIT_UNRATIFIED_SPEC: u8 = 6;

#[derive(Parser)]
#[command(name = "specforge", version, about = "Spec generation harness (entry gate: normalize + rigor)")]
struct Cli {
    #[arg(short, long, default_value = "specforge.toml", global = true)]
    config: PathBuf,
    /// Override the run date (YYYY-MM-DD); default today (also SPECFORGE_NOW).
    #[arg(long, global = true)]
    date: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Normalize a requirement into a draft Spec IR (rigor-gated).
    Draft {
        /// Spec id (becomes `<spec_dir>/<id>.toml`).
        #[arg(long)]
        id: String,
        /// Optional human title.
        #[arg(long, default_value = "")]
        title: String,
        /// Requirement file (omit to read the requirement from stdin).
        #[arg(long)]
        req: Option<PathBuf>,
        /// canon pointer(s) backing the requirement (repeatable).
        #[arg(long)]
        canon: Vec<String>,
    },
    /// Render the normalize prompt only (no agent call) — debug view.
    Prompt {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "")]
        title: String,
        #[arg(long)]
        req: Option<PathBuf>,
        #[arg(long)]
        canon: Vec<String>,
    },
    /// Promote a draft spec to ratified (human consent ceremony, DESIGN.md §5).
    Ratify {
        #[arg(long)]
        id: String,
        /// Why this spec is accepted (recorded in the ratification record).
        #[arg(short = 'm', long = "reason")]
        reason: String,
    },
    /// Clear the sentinel after a human has handled the escalation.
    Ack,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("specforge: error: {e:#}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

struct Loaded {
    cfg: Config,
    repo_root: PathBuf,
    config_dir: PathBuf,
    date: String,
}

fn load(cli: &Cli) -> Result<Loaded> {
    let cfg = Config::load(&cli.config)?;
    let config_dir = cli
        .config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let repo_root = canonicalize(&config_dir.join(&cfg.project.root))
        .with_context(|| "resolving project.root")?;
    let date = resolve_date(cli.date.clone());
    Ok(Loaded {
        cfg,
        repo_root,
        config_dir,
        date,
    })
}

fn run(cli: &Cli) -> Result<u8> {
    let l = load(cli)?;
    match &cli.command {
        Command::Draft {
            id,
            title,
            req,
            canon,
        } => draft(&l, id, title, req.as_deref(), canon),
        Command::Prompt {
            id,
            title,
            req,
            canon,
        } => {
            let _ = title;
            let requirement = read_requirement(req.as_deref())?;
            let template = load_template(&l)?;
            let out = prompt::render(&template, &l.cfg.project.name, id, &requirement, canon, &l.date);
            print!("{out}");
            Ok(EXIT_OK)
        }
        Command::Ratify { id, reason } => ratify(&l, id, reason),
        Command::Ack => ack(&l),
    }
}

/// ②normalize + §5.3 rigor pre-flight. The agent decides; the harness gates.
fn draft(
    l: &Loaded,
    id: &str,
    title: &str,
    req: Option<&Path>,
    canon: &[String],
) -> Result<u8> {
    let requirement = read_requirement(req)?;
    let template = load_template(l)?;
    let rendered = prompt::render(&template, &l.cfg.project.name, id, &requirement, canon, &l.date);

    let out = agent::run(&l.cfg.agent, &l.repo_root, &rendered);
    if out.code != 0 {
        eprintln!(
            "specforge: normalize agent exited with code {}\n--- agent stderr ---\n{}",
            out.code,
            out.stderr.trim_end()
        );
        return Ok(EXIT_AGENT_FAILED);
    }

    let parsed = parse::parse(&out.stdout);
    if !parsed.marker_found {
        eprintln!(
            "specforge: WARN marker '{}' missing in agent output; cannot assess. No draft, no sentinel.",
            parse::MARKER
        );
        return Ok(EXIT_NO_MARKER);
    }

    // Escalation path (insufficiency §5.2 / conflict §5.1): the agent could not
    // rigorously specify. Do NOT fabricate a draft — raise a sentinel and pull
    // the human in. This is a *successful* HOTL outcome (exit 0).
    if !parsed.rigor_pass || parsed.needs_user {
        write_sentinel(l, id, &parsed.summary, &parsed.body)?;
        println!(
            "specforge: rigor 未達 — 厳格に仕様化できない。sentinel を上げた ({}); draft は書かない。\n  summary: {}",
            sentinel_path(l).display(),
            parsed.summary
        );
        return Ok(EXIT_OK);
    }

    // Rigor claimed pass: parse the requirement TOML the agent emitted.
    let agent_draft = match ir::AgentDraft::parse(&parsed.body) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "specforge: WARN rigor:pass but body is not valid requirement TOML: {e}\n  No draft written (agent contract violation)."
            );
            return Ok(EXIT_NO_MARKER);
        }
    };

    let head = current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());
    let spec = ir::Spec {
        spec: ir::SpecMeta {
            id: id.to_string(),
            title: title.to_string(),
            status: "draft".to_string(),
            provenance_commit: head,
            date: l.date.clone(),
            canon: canon.to_vec(),
            ratification: None,
        },
        requirements: agent_draft.requirements,
    };

    // Deterministic floor: the harness re-checks the rigor contract the agent
    // claimed. If the agent over-claimed (e.g. an acceptance with no canon),
    // escalate rather than persist an ungrounded draft — the machine guards the
    // gate even when the LLM says pass.
    let violations = spec.contract_violations();
    if !violations.is_empty() {
        let body = format!(
            "agent は rigor:pass を主張したが、ハーネスの契約チェックで以下に違反:\n{}",
            violations.iter().map(|v| format!("- {v}\n")).collect::<String>()
        );
        write_sentinel(l, id, "rigor:pass の過大主張をハーネスが棄却", &body)?;
        println!(
            "specforge: agent が rigor:pass を過大主張 — 契約違反を検出し sentinel を上げた ({}); draft は書かない。",
            sentinel_path(l).display()
        );
        for v in &violations {
            println!("  - {v}");
        }
        return Ok(EXIT_OK);
    }

    let path = write_spec(l, id, &spec)?;
    println!("specforge: draft を書いた -> {}", path.display());
    println!("  内容を確認し、合意できるなら `specforge ratify --id {id} -m \"理由\"` で昇格 (HOTL)。");
    Ok(EXIT_OK)
}

/// Promote draft → ratified: contract-check, then pin consent (DESIGN.md §5).
fn ratify(l: &Loaded, id: &str, reason: &str) -> Result<u8> {
    if reason.trim().is_empty() {
        anyhow::bail!("批准には理由が必要です (-m \"...\")");
    }
    let path = spec_path(l, id);
    if !path.exists() {
        anyhow::bail!("spec が無い: {} (先に `specforge draft --id {id}`)", path.display());
    }
    let mut spec = ir::Spec::load(&path)?;

    // Machine contract floor (G1/G4): refuse to ratify an ungrounded spec. The
    // policy judgment is the human's, recorded as the reason.
    let violations = spec.contract_violations();
    if !violations.is_empty() {
        let mut msg = String::from("spec が rigor 契約に違反 — 批准を拒否:");
        for v in &violations {
            msg.push_str(&format!("\n  - {v}"));
        }
        anyhow::bail!(msg);
    }

    let head = current_head(&l.repo_root).unwrap_or_else(|_| "UNKNOWN".to_string());
    let fingerprint = spec.fingerprint();
    spec.spec.status = "ratified".to_string();
    spec.spec.ratification = Some(ir::Ratification {
        canon_commit: head.clone(),
        date: l.date.clone(),
        reason: reason.to_string(),
        fingerprint,
    });
    write_spec(l, id, &spec)?;
    println!("specforge: spec を批准した (draft -> ratified) -> {}", path.display());
    println!("  canon commit に pin (canon_commit: {head})");
    println!("  理由: {reason}");
    println!("  以後、内容を編集すると fingerprint が変わり再批准が必要 (DESIGN.md §5)。");
    Ok(EXIT_OK)
}

/// Clear the sentinel (idempotent).
fn ack(l: &Loaded) -> Result<u8> {
    let path = sentinel_path(l);
    match std::fs::remove_file(&path) {
        Ok(()) => println!("specforge: sentinel をクリアした ({})", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("specforge: sentinel は無い ({})", path.display());
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("removing sentinel {}", path.display()));
        }
    }
    Ok(EXIT_OK)
}

// --- helpers ---

fn read_requirement(req: Option<&Path>) -> Result<String> {
    match req {
        Some(p) => std::fs::read_to_string(p)
            .with_context(|| format!("reading requirement {}", p.display())),
        None => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading requirement from stdin")?;
            if s.trim().is_empty() {
                anyhow::bail!("要望が空です (--req <file> か stdin で渡してください)");
            }
            Ok(s)
        }
    }
}

fn load_template(l: &Loaded) -> Result<String> {
    if l.cfg.prompt.normalize_template.trim().is_empty() {
        return Ok(prompt::DEFAULT_TEMPLATE.to_string());
    }
    let p = l.config_dir.join(&l.cfg.prompt.normalize_template);
    let template = std::fs::read_to_string(&p)
        .with_context(|| format!("reading normalize template {}", p.display()))?;
    // The template is meta-canon: contract-check it (required placeholders) so a
    // custom template can't silently break the render/parse contract.
    let missing = prompt::missing_placeholders(&template);
    if !missing.is_empty() {
        anyhow::bail!(
            "normalize template {} は必須 placeholder を欠く: {}",
            p.display(),
            missing.join(", ")
        );
    }
    Ok(template)
}

fn spec_path(l: &Loaded, id: &str) -> PathBuf {
    l.repo_root.join(&l.cfg.output.spec_dir).join(format!("{id}.toml"))
}

fn write_spec(l: &Loaded, id: &str, spec: &ir::Spec) -> Result<PathBuf> {
    let path = spec_path(l, id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating spec dir {}", dir.display()))?;
    }
    std::fs::write(&path, spec.to_toml()?)
        .with_context(|| format!("writing spec {}", path.display()))?;
    Ok(path)
}

fn sentinel_path(l: &Loaded) -> PathBuf {
    l.repo_root.join(&l.cfg.output.sentinel)
}

/// Raise the escalation sentinel (HOTL pull). Same spirit as specguard's: a
/// date/spec/summary header plus the agent's request body, for a SessionStart
/// hook to surface.
fn write_sentinel(l: &Loaded, id: &str, summary: &str, body: &str) -> Result<()> {
    let path = sentinel_path(l);
    let content = format!(
        "date: {}\nspec: {}\nsummary: {}\n\n{}\n",
        l.date,
        id,
        summary,
        body.trim_end()
    );
    std::fs::write(&path, content).with_context(|| format!("writing sentinel {}", path.display()))?;
    Ok(())
}

fn current_head(repo_root: &Path) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("running git rev-parse")?;
    if !out.status.success() {
        anyhow::bail!("git rev-parse failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_date(cli_date: Option<String>) -> String {
    if let Some(d) = cli_date {
        return d;
    }
    if let Ok(d) = std::env::var("SPECFORGE_NOW") {
        if !d.trim().is_empty() {
            return d.trim().to_string();
        }
    }
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn canonicalize(p: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(p).or_else(|_| Ok(p.to_path_buf()))
}
