//! compass — goal re-grounding + next-move derivation upstream of condukt.
//!
//! A thin, subscription-native orchestrator (DESIGN §2): the binary keeps state
//! and renders deterministic context; the LLM (skill) does the judging. This is
//! the **scaffold** — only `charter` (show/parse) and config loading have real
//! behavior. `nudge` / `breadcrumb` / `gap` / `route` are wired into the
//! dispatch but stubbed until later tasks (DESIGN §15-B implementation order).

mod breadcrumb;
mod carve;
mod charter;
mod config;
mod freshness;
mod gap;
mod gates;
mod gather;
mod outcome;
mod route;

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use harness_core::hook::{read_stdin, HookInput};
use harness_core::interrogate::{self, Answer, CarveState};

use carve::CarveView;
use charter::Charter;
use config::Config;
use gates::CompassGates;
use route::Decomposition;

#[derive(Parser)]
#[command(
    name = "compass",
    version,
    about = "Goal re-grounding + next-move derivation upstream of condukt; subscription-native."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// SessionStart hook (§14): deterministic C1/C2 freshness nudge.
    Nudge,
    /// Stop hook (§14): write the next physical step into the charter.
    Breadcrumb,
    /// Carve floor (§11): print the deterministic C1/C2 open questions as JSON.
    /// The skill ADDS its own C3–C5 on top. Initializes-or-loads the CarveState.
    Evaluate,
    /// Carve step (§11): fold one human answer into the CarveState, re-check
    /// C1/C2, persist, and print the updated `{open_questions,status,round}`.
    Apply(ApplyArgs),
    /// Clear the persisted CarveState (start a fresh carve).
    CarveReset,
    /// Derive goal - current-state gap and candidate next moves (§3).
    Gap(GapArgs),
    /// Size triage (§13): route the right-sized move / park the rest.
    Route(RouteArgs),
    /// Show the parsed charter (+ resolved config), or (--write JSON) persist a
    /// skill-composed charter.
    Charter(CharterArgs),
    /// Record a completed move's judged outcome vs the charter `measuring_stick`
    /// (§7). Requires measured evidence (build is not validation). Surfaced as
    /// `last_outcome` in `compass gap`.
    Outcome(OutcomeArgs),
}

#[derive(Args)]
struct ApplyArgs {
    /// The human [`Answer`] as JSON (`{gate, reference, value, defer}`). Omit to
    /// read the JSON from stdin.
    #[arg(long, value_name = "JSON")]
    answer: Option<String>,
}

#[derive(Args)]
struct CharterArgs {
    /// Persist a full skill-composed [`Charter`] as JSON
    /// (`{north_star, definition_of_done, measuring_stick, current_gap,
    /// next_action, parked}`). Omit to show the current parsed charter.
    #[arg(long, value_name = "JSON")]
    write: Option<String>,
}

#[derive(Args)]
struct GapArgs {
    /// Persist the skill-produced gap text into the charter `current_gap`
    /// instead of printing the assembled inputs. Deterministic write-back.
    #[arg(long, value_name = "TEXT")]
    write: Option<String>,
}

#[derive(Args)]
struct RouteArgs {
    /// Read the decomposition JSON from this file. Omit to read from stdin.
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
}

#[derive(Args)]
struct OutcomeArgs {
    /// The judged outcome vs the charter `measuring_stick`:
    /// `forward` (前進) / `unchanged` (不変) / `backward` (後退).
    #[arg(long, value_enum)]
    verdict: outcome::Verdict,
    /// Measured evidence for the verdict. Repeatable; REQUIRED to be non-empty
    /// after trimming (build is not validation).
    #[arg(long, value_name = "STRING")]
    evidence: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.command {
        Command::Nudge => nudge_command(),
        Command::Breadcrumb => breadcrumb_command(),
        Command::Evaluate => evaluate_command(),
        Command::Apply(args) => apply_command(args),
        Command::CarveReset => carve_reset_command(),
        Command::Gap(args) => gap_command(args),
        Command::Route(args) => route_command(args),
        Command::Charter(args) => charter_command(args),
        Command::Outcome(args) => outcome_command(args),
    };
    if let Err(e) = r {
        eprintln!("compass: {e}");
        std::process::exit(1);
    }
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
}

/// SessionStart hook (§14): the deterministic C1/C2 freshness nudge.
///
/// Thin path: run the deterministic floor only (NO LLM, no C3–C5) and print a
/// one-line nudge to stdout if the charter is absent/blurry (C1) or drift-
/// suspect (C2). Non-blocking by contract: this always exits 0, even on error,
/// so a re-grounding hook never breaks a session start.
fn nudge_command() -> Result<()> {
    let root = project_root();
    let cfg = Config::load(&root);
    let path = Charter::project_path(&root);

    // C1 existence (deterministic): absent file => loudest nudge.
    if !path.exists() {
        println!("compass: no .compass/charter.md yet — run `/compass` to carve a north star.");
        return Ok(());
    }

    // Parse is tolerant; a failure shouldn't break session start.
    let charter = match Charter::load(&path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // C1: blurry charter (empty north_star / DoD).
    if charter.north_star.trim().is_empty() || charter.definition_of_done.is_empty() {
        println!("compass: charter is blurry (north_star/DoD incomplete) — run `/compass`.");
        return Ok(());
    }

    // C2: deterministic freshness floor.
    let fresh = freshness::check(&root, &path, &charter, &cfg);
    if fresh.stale {
        println!(
            "compass: charter may be stale ({}) — run `/compass` to re-ground.",
            fresh.reasons.join("; ")
        );
    }
    Ok(())
}

/// Stop hook (§14 / §5⑤): write "the next physical action" into the charter so
/// the ②"blank after a checkpoint" pain doesn't recur. Reads the Stop payload
/// from stdin, finds the assistant's last message via `transcript_path`, and
/// extracts an explicit ```` ```compass-next ```` block; if found, sets
/// `charter.next_action`. No LLM, never guesses. ALWAYS exits 0 — a breadcrumb
/// must never block a turn.
fn breadcrumb_command() -> Result<()> {
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return Ok(()); // empty / invalid payload: stay silent.
    };

    // The assistant's final message lives in the JSONL transcript. Pull a
    // generous slice so a fenced block isn't truncated mid-body.
    let message = match last_assistant_message(&input.transcript_path) {
        Some(m) => m,
        None => return Ok(()), // no text to inspect: do nothing.
    };

    let Some(next_action) = breadcrumb::extract_next_action(&message) else {
        return Ok(()); // no explicit block: do NOT guess.
    };

    let root = input.cwd_or_current();
    let path = Charter::project_path(&root);
    // Best-effort write; swallow errors so the hook never breaks a turn.
    let _ = breadcrumb::write_next_action(&path, &next_action);
    Ok(())
}

/// Best-effort: the last assistant text turn from a JSONL transcript path. Each
/// line is a JSON object; an `assistant` turn carries `message.content[]` with
/// `{type:"text", text}` parts. Any read/parse error yields `None` (the hook
/// then does nothing). Returns the full joined text (untruncated) so a fenced
/// block survives intact.
fn last_assistant_message(transcript_path: &str) -> Option<String> {
    if transcript_path.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(transcript_path).ok()?;
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let Some(parts) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        let joined: String = parts
            .iter()
            .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if joined.trim().is_empty() {
            continue; // tool-only turn: keep scanning earlier turns.
        }
        return Some(joined);
    }
    None
}

/// Carve floor (§11): build the C1/C2 deterministic open questions, initialize-
/// or-load the persisted [`CarveState`] (max_rounds from `[carve]`), persist it,
/// and print `{open_questions, status, round}` as JSON. The skill reads this
/// floor and ADDS its own C3–C5 questions on top.
fn evaluate_command() -> Result<()> {
    let root = project_root();
    let cfg = Config::load(&root);
    let path = Charter::project_path(&root);
    let charter = Charter::load(&path).unwrap_or_default();

    let bundle = gather::gather(&root, &charter, &cfg)?;
    let gates = CompassGates {
        cfg: &cfg,
        repo_root: &root,
        charter_path: &path,
        charter: &charter,
    };

    // Initialize-or-load: a fresh evaluate re-seeds the bundle from gather (so
    // it reflects the current repo state); a resumed carve keeps its answers.
    let mut state = match carve::load(&root) {
        Some(mut s) => {
            // Refresh the freshly-gathered material but keep recorded answers
            // folded back in (they were appended as High fragments by `apply`).
            s.bundle = bundle.clone();
            for ans in &s.answers {
                if !ans.defer {
                    s.bundle.fragments.push(interrogate::Fragment {
                        text: ans.value.clone(),
                        source_path: format!(
                            "interrogate:answer:{}:{}",
                            ans.gate, ans.reference
                        ),
                        authority: interrogate::Authority::High,
                        score: 0,
                        anchor: None,
                    });
                }
            }
            s
        }
        None => CarveState::new(bundle, cfg.carve.max_rounds),
    };

    let open = interrogate::evaluate(&gates, &state.bundle);
    // Reflect emptiness in status without consuming a round (round bumps only on
    // `apply`): a clean floor at round 0 reads as resolved for the skill.
    if open.is_empty() && state.round == 0 {
        state.status = interrogate::CarveStatus::Resolved;
    }
    carve::save(&root, &state)?;

    println!("{}", serde_json::to_string_pretty(&CarveView::new(open, &state))?);
    Ok(())
}

/// Carve step (§11): deserialize one human [`Answer`] (from `--answer` or
/// stdin), load the persisted [`CarveState`], fold the answer in + re-check
/// C1/C2 via [`interrogate::apply`], persist, and print the updated view. C3–C5
/// answers from the skill are recorded here too (they become High-authority
/// fragments); only C1/C2 are re-evaluated by Rust — that's expected.
fn apply_command(args: ApplyArgs) -> Result<()> {
    let root = project_root();
    let cfg = Config::load(&root);
    let path = Charter::project_path(&root);
    let charter = Charter::load(&path).unwrap_or_default();

    let json = read_arg_or_stdin(args.answer.unwrap_or_default())?;
    let answer: Answer = serde_json::from_str(&json).context("parsing answer JSON")?;

    // Resume the carve; if none persisted, start one over the current gather.
    let mut state = match carve::load(&root) {
        Some(s) => s,
        None => {
            let bundle = gather::gather(&root, &charter, &cfg)?;
            CarveState::new(bundle, cfg.carve.max_rounds)
        }
    };

    let gates = CompassGates {
        cfg: &cfg,
        repo_root: &root,
        charter_path: &path,
        charter: &charter,
    };

    let open = interrogate::apply(&gates, &mut state, answer);
    carve::save(&root, &state)?;

    println!("{}", serde_json::to_string_pretty(&CarveView::new(open, &state))?);
    Ok(())
}

/// Clear the persisted [`CarveState`] (start fresh). Small helper for the skill.
fn carve_reset_command() -> Result<()> {
    let root = project_root();
    carve::reset(&root)?;
    println!("compass: carve state cleared.");
    Ok(())
}

/// Resolve a JSON-bearing arg: a non-empty value is used verbatim; an empty
/// value means "read it from stdin" (so the skill can pipe large payloads).
fn read_arg_or_stdin(value: String) -> Result<String> {
    if !value.trim().is_empty() {
        return Ok(value);
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading JSON from stdin")?;
    Ok(buf)
}

/// Show the resolved config and the parsed charter for the cwd, OR (`--write
/// <JSON>`) persist a skill-composed charter. The write path lets the skill save
/// the sharpened charter (north_star / observable DoD / measuring_stick / gap /
/// next_action / parked) after the carve resolves. Deterministic; no LLM.
fn charter_command(args: CharterArgs) -> Result<()> {
    let root = project_root();

    if let Some(json) = args.write {
        let json = read_arg_or_stdin(json)?;
        let charter: Charter =
            serde_json::from_str(&json).context("parsing charter JSON for --write")?;
        let path = Charter::project_path(&root);
        charter.save(&path)?;
        println!("compass: wrote charter to {}", path.display());
        return Ok(());
    }

    let cfg = Config::load(&root);
    let path = Charter::project_path(&root);

    println!("config:        {}", Config::project_path(&root).display());
    println!("  stale_commits  = {}", cfg.freshness.stale_commits);
    println!("  stale_days     = {}", cfg.freshness.stale_days);
    println!("  check_dod_refs = {}", cfg.freshness.check_dod_refs);
    println!("  max_rounds     = {}", cfg.carve.max_rounds);
    println!("  right_size     = {:?}", cfg.routing.right_size);
    println!();

    if !path.exists() {
        println!("charter:       {} (absent)", path.display());
        return Ok(());
    }
    let charter = Charter::load(&path)?;
    println!("charter:       {}", path.display());
    print!("{}", charter.to_markdown());
    Ok(())
}

/// gap (§3): deterministically assemble the inputs the skill reasons over, or
/// (with `--write <text>`) persist the skill-produced gap into the charter.
/// No LLM, no semantic derivation here.
fn gap_command(args: GapArgs) -> Result<()> {
    let root = project_root();
    let path = Charter::project_path(&root);
    let mut charter = Charter::load(&path)
        .with_context(|| "loading .compass/charter.md (run `/compass` to carve one)")?;

    // Write-back path: persist the skill-produced gap text.
    if let Some(text) = args.write {
        gap::persist_gap(&path, &mut charter, &text)?;
        println!("compass: wrote current_gap to {}", path.display());
        return Ok(());
    }

    // Default path: print the assembled inputs as JSON for the skill to read.
    let cfg = Config::load(&root);
    let bundle = gather::gather(&root, &charter, &cfg)?;
    let mut inputs = gap::assemble_gap_inputs(&charter, &bundle);
    // Close the measurement loop: surface the latest judged outcome (§7).
    inputs.last_outcome = outcome::latest(&root)?;
    println!("{}", serde_json::to_string_pretty(&inputs)?);
    Ok(())
}

/// outcome (§7): record a completed move's judged outcome against the charter
/// `measuring_stick`, requiring measured evidence, and append it to the
/// `.compass/outcomes.json` store. The latest record is surfaced by `gap`
/// (`last_outcome`). No LLM — the verdict/evidence are the human's judgment.
fn outcome_command(args: OutcomeArgs) -> Result<()> {
    let root = project_root();
    let path = Charter::project_path(&root);
    let charter = Charter::load(&path)
        .with_context(|| "loading .compass/charter.md (run `/compass` to carve one)")?;

    let recorded = outcome::record(&root, &charter, args.verdict, args.evidence)?;
    println!(
        "compass: recorded outcome #{} ({:?}) vs measuring_stick — {}",
        recorded.seq,
        recorded.verdict,
        outcome::store_path(&root).display()
    );
    Ok(())
}

/// route (§13): consume an already-produced condukt decomposition JSON and
/// triage it deterministically by `size`. Prints the routing as JSON, writes
/// parked tasks to taskprog, and prints the condukt handoff text. No LLM.
fn route_command(args: RouteArgs) -> Result<()> {
    let root = project_root();
    let cfg = Config::load(&root);

    // Read the decomposition from --file or stdin.
    let raw = match &args.file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("reading decomposition {}", path.display()))?,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading decomposition from stdin")?;
            buf
        }
    };
    let dec: Decomposition =
        serde_json::from_str(&raw).context("parsing decomposition JSON")?;

    let routing = route::route(&dec, &cfg);

    // Print the routing (+ edge) as JSON.
    println!("{}", serde_json::to_string_pretty(&routing)?);

    // Write parked tasks to the taskprog "残り" sink (self-feeding loop, §6).
    route::write_parked_to_taskprog(&root, &routing.parked)?;

    // Print the condukt handoff 課題 text (context for the chosen move).
    let charter = Charter::load(&Charter::project_path(&root)).unwrap_or_default();
    println!();
    print!(
        "{}",
        route::condukt_handoff(&routing.to_condukt, &charter, &dec.goal)
    );
    Ok(())
}
