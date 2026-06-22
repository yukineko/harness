//! ctxrot — a context-rot guard for Claude Code.
//!
//! One binary, one subcommand per hook. Hook subcommands read the event JSON
//! from stdin and emit the appropriate output. The cardinal rule: a hook must
//! NEVER break the user's turn — on any error we exit 0 and stay silent.

mod config;
mod hooks;
mod install;
mod metrics;
mod model;
mod store;
mod transcript;
mod usage;

use std::io::Read;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use config::Config;
use model::HookInput;
use store::Store;

#[derive(Parser)]
#[command(
    name = "ctxrot",
    version,
    about = "Context-rot guard for Claude Code: detect, rescue, restore, distill."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// UserPromptSubmit hook: detect large refs + context-budget bands.
    Guard,
    /// PreCompact hook: rescue decisions/todos/files to a durable note.
    Rescue,
    /// SessionStart hook: inject a compact carryover from the latest note.
    Restore,
    /// PreToolUse hook: gate a Read of a pathologically large local file.
    Preguard,
    /// PostToolUse hook: warn on huge tool output.
    Toolguard,
    /// Merge ctxrot hooks into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove ctxrot hooks from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a default ~/.ctxrot/config.toml and create store/state dirs.
    Init,
    /// Inspect the note store.
    Note {
        #[command(subcommand)]
        action: NoteAction,
    },
    /// Inspect the metrics log (per-session rollup of bands/notes/gates).
    Metrics {
        #[command(subcommand)]
        action: Option<MetricsAction>,
    },
    /// statusLine command: read the status JSON on stdin, print a one-line
    /// context-usage meter (`ctxrot 52% ▮▮▯▯▯ band1 ~104k/200k`).
    Statusline,
    /// Print the current session's context usage (for usage-aware /distill).
    /// Resolves the transcript from the session id unless --transcript is given.
    Usage {
        /// Transcript path to read (default: resolve from --session).
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// Session id to resolve the transcript for (default: $CLAUDE_CODE_SESSION_ID).
        #[arg(long)]
        session: Option<String>,
    },
}

#[derive(Subcommand)]
enum MetricsAction {
    /// Per-session rollup (default).
    Summary,
    /// Print the metrics log path.
    Path,
    /// A/B compare two session groups by id prefix (e.g. guard-on vs GUARD_DISABLE).
    /// Each prefix folds all matching sessions; prints both groups and Δ(A−B).
    Compare {
        /// Session-id prefix for group A (the standard protocol: guard ON).
        a: String,
        /// Session-id prefix for group B (the standard protocol: GUARD_DISABLE).
        b: String,
    },
    /// Print the peak context usage (% and max band) for a session id prefix.
    /// For /record to stamp "how close this session got" into the session note.
    Peak {
        /// Session-id prefix (pass $CLAUDE_CODE_SESSION_ID for the current one).
        session: String,
    },
}

#[derive(Subcommand)]
enum NoteAction {
    /// List notes for a project (default: cwd), newest first.
    List {
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Print the path of the latest note for a project.
    Latest {
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Print (and create) the note directory for a project.
    Dir {
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Write a note from stdin into the store; prints the path.
    Write {
        #[arg(long, default_value = "distill")]
        slug: String,
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Session id to tag the filename with, so the originating session can
        /// reach its own note amid parallel sessions (pass $CLAUDE_CODE_SESSION_ID).
        #[arg(long)]
        session: Option<String>,
        /// Enforce the distill contract: reject (exit 1, write nothing) unless the
        /// note carries the headings `restore` depends on (決定事項/Decisions and
        /// 残課題/Open todos). Use for /distill so carryover is never silently empty.
        #[arg(long)]
        require_sections: bool,
    },
}

fn read_stdin() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

/// Run a hook handler with all errors swallowed; always exits 0.
fn run_hook<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> ! {
    let _ = std::panic::catch_unwind(f);
    std::process::exit(0);
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Guard => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let raw = read_stdin();
            if let Some(input) = HookInput::parse(&raw) {
                let cfg = Config::load();
                if let Some(text) = hooks::guard::run(&input, &cfg) {
                    println!("{text}");
                }
            }
        }),
        Command::Rescue => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let raw = read_stdin();
            if let Some(input) = HookInput::parse(&raw) {
                let cfg = Config::load();
                if let Some(path) = hooks::rescue::run(&input, &cfg) {
                    // PreCompact does not inject context; report to stderr only.
                    eprintln!("[ctxrot] rescue note saved: {}", path.display());
                }
            }
        }),
        Command::Restore => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let raw = read_stdin();
            if let Some(input) = HookInput::parse(&raw) {
                let cfg = Config::load();
                if let Some(text) = hooks::restore::run(&input, &cfg) {
                    // SessionStart: plain stdout is injected as additional context.
                    println!("{text}");
                }
            }
        }),
        Command::Preguard => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let raw = read_stdin();
            if let Some(input) = HookInput::parse(&raw) {
                let cfg = Config::load();
                if let Some(reason) = hooks::preguard::run(&input, &cfg) {
                    // PreToolUse: deny the call; the reason is the only steering
                    // channel (PreToolUse can't inject additionalContext).
                    let out = serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PreToolUse",
                            "permissionDecision": "deny",
                            "permissionDecisionReason": reason,
                        }
                    });
                    println!("{out}");
                }
            }
        }),
        Command::Toolguard => run_hook(|| {
            if Config::disabled() {
                return;
            }
            let raw = read_stdin();
            if let Some(input) = HookInput::parse(&raw) {
                let cfg = Config::load();
                if let Some(text) = hooks::toolguard::run(&input, &cfg) {
                    // PostToolUse needs JSON to inject context.
                    let out = serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PostToolUse",
                            "additionalContext": text,
                        }
                    });
                    println!("{out}");
                }
            }
        }),

        // ----- user-invoked (normal error reporting) -----
        Command::Install { dry_run } => {
            if let Err(e) = install::install(dry_run) {
                eprintln!("install failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Uninstall { dry_run } => {
            if let Err(e) = install::uninstall(dry_run) {
                eprintln!("uninstall failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Init => {
            if let Err(e) = init() {
                eprintln!("init failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Note { action } => {
            let cfg = Config::load();
            let store = Store::new(&cfg);
            match action {
                NoteAction::List { cwd } => {
                    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
                    for p in store.list_notes(&cwd) {
                        println!("{}", p.display());
                    }
                }
                NoteAction::Latest { cwd } => {
                    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
                    match store.latest_note(&cwd) {
                        Some(p) => println!("{}", p.display()),
                        None => {
                            eprintln!("(no notes for this project)");
                            std::process::exit(1);
                        }
                    }
                }
                NoteAction::Dir { cwd } => {
                    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
                    let dir = store.project_dir(&cwd);
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("could not create {}: {e}", dir.display());
                        std::process::exit(1);
                    }
                    println!("{}", dir.display());
                }
                NoteAction::Write { slug, cwd, session, require_sections } => {
                    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
                    let body = read_stdin();
                    if require_sections {
                        let missing = hooks::restore::missing_sections(&body);
                        if !missing.is_empty() {
                            eprintln!(
                                "distill contract violation — missing required section(s): {}",
                                missing.join(", ")
                            );
                            eprintln!(
                                "→ add the heading(s) (use \"_(なし / none)_\" if truly empty) and retry; nothing was written."
                            );
                            std::process::exit(1);
                        }
                        // Soft contract: the rest of the distill shape lifts carryover
                        // quality but isn't load-bearing for `restore`, so warn only.
                        let soft = hooks::restore::missing_recommended_sections(&body);
                        if !soft.is_empty() {
                            eprintln!(
                                "distill contract note — missing recommended section(s): {}",
                                soft.join(", ")
                            );
                            eprintln!(
                                "→ これらは restore には不要ですが carryover の質が上がります（任意・書き込みは継続）。"
                            );
                        }
                    }
                    let safe: String = slug
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
                        .collect();
                    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                    // Tag with the session so restore can route this session back
                    // to its own note even with parallel sessions in one project.
                    let tag = store::session_tag(session.as_deref().unwrap_or(""));
                    match store.write_note(&cwd, &format!("{safe}-{tag}-{stamp}"), &body) {
                        Ok(p) => println!("{}", p.display()),
                        Err(e) => {
                            eprintln!("write failed: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Command::Metrics { action } => {
            let cfg = Config::load();
            match action.unwrap_or(MetricsAction::Summary) {
                MetricsAction::Path => println!("{}", metrics::path(&cfg).display()),
                MetricsAction::Summary => {
                    let stats = metrics::summarize(&cfg);
                    if stats.is_empty() {
                        println!("(no metrics yet: {})", metrics::path(&cfg).display());
                    } else {
                        println!(
                            "{:<16} {:>7} {:>5} {:>4} {:>9} {:>9} {:>6} {:>4} {:>4}",
                            "session", "prompts", "cross", "band", "peak_tok", "last_tok",
                            "rescue", "gate", "dump"
                        );
                        for s in &stats {
                            let sid: String = if s.session.chars().count() > 16 {
                                let t: String = s.session.chars().take(15).collect();
                                format!("{t}…")
                            } else {
                                s.session.clone()
                            };
                            println!(
                                "{:<16} {:>7} {:>5} {:>4} {:>9} {:>9} {:>6} {:>4} {:>4}",
                                sid, s.prompts, s.crossings, s.max_band, s.peak_tokens,
                                s.last_tokens, s.rescues, s.gates, s.tooldumps
                            );
                        }
                    }
                }
                MetricsAction::Compare { a, b } => {
                    let stats = metrics::summarize(&cfg);
                    let ga = metrics::group_by_prefix(&stats, &a);
                    let gb = metrics::group_by_prefix(&stats, &b);
                    match (ga, gb) {
                        (None, _) => {
                            eprintln!("no session matches prefix '{a}' (group A)");
                            std::process::exit(1);
                        }
                        (_, None) => {
                            eprintln!("no session matches prefix '{b}' (group B)");
                            std::process::exit(1);
                        }
                        (Some((ga, na)), Some((gb, nb))) => {
                            let row = |label: &str, s: &metrics::SessionStat| {
                                println!(
                                    "{:<14} {:>7} {:>5} {:>4} {:>9} {:>6} {:>4} {:>4}",
                                    label, s.prompts, s.crossings, s.max_band, s.peak_tokens,
                                    s.rescues, s.gates, s.tooldumps
                                );
                            };
                            println!(
                                "{:<14} {:>7} {:>5} {:>4} {:>9} {:>6} {:>4} {:>4}",
                                "group", "prompts", "cross", "band", "peak_tok", "rescue",
                                "gate", "dump"
                            );
                            row(&format!("A:{a} ({na})"), &ga);
                            row(&format!("B:{b} ({nb})"), &gb);
                            // Δ(A−B): signed gaps on the figures the guard targets.
                            let d = |x: u64, y: u64| x as i64 - y as i64;
                            println!(
                                "{:<14} {:>7} {:>5} {:>4} {:>9} {:>6} {:>4} {:>4}",
                                "Δ A−B",
                                d(ga.prompts, gb.prompts),
                                d(ga.crossings, gb.crossings),
                                d(ga.max_band, gb.max_band),
                                d(ga.peak_tokens, gb.peak_tokens),
                                d(ga.rescues, gb.rescues),
                                d(ga.gates, gb.gates),
                                d(ga.tooldumps, gb.tooldumps),
                            );
                            println!(
                                "\n(標準プロトコルでは A=guard有効 / B=GUARD_DISABLE。\
                                 peak_tok と band の Δ が負ほどガードが context を抑えた証拠。)"
                            );
                        }
                    }
                }
                MetricsAction::Peak { session } => {
                    let stats = metrics::summarize(&cfg);
                    match metrics::group_by_prefix(&stats, &session) {
                        None => {
                            eprintln!("no session matches prefix '{session}'");
                            std::process::exit(1);
                        }
                        Some((g, n)) => {
                            let pct = usage::pct_from_tokens(&cfg, g.peak_tokens);
                            println!(
                                "peak ~{pct}% (band{}, ~{}/{} tokens, {n} session(s))",
                                g.max_band, g.peak_tokens, cfg.context_window
                            );
                        }
                    }
                }
            }
        }
        Command::Statusline => {
            // Never break the status bar: any failure prints nothing, exits 0.
            let cfg = Config::load();
            let raw = read_stdin();
            if let Some(line) = statusline_from(&cfg, &raw) {
                println!("{line}");
            }
        }
        Command::Usage { transcript, session } => {
            let cfg = Config::load();
            let path = transcript.map(|p| p.to_string_lossy().into_owned()).or_else(|| {
                let sid = session
                    .or_else(|| std::env::var("CLAUDE_CODE_SESSION_ID").ok())
                    .unwrap_or_default();
                usage::find_transcript_for_session(&sid).map(|p| p.to_string_lossy().into_owned())
            });
            match path.as_deref().and_then(transcript::estimate_tokens) {
                Some((tokens, _src)) => {
                    let pct = usage::pct_from_tokens(&cfg, tokens);
                    println!("{}", usage::line(&cfg, pct, Some(tokens)));
                    println!("hint: {}", usage::hint(&cfg, pct));
                }
                None => {
                    println!("context使用量は不明（transcript 未解決）。focus 指定があれば distill を続行可。");
                }
            }
        }
    }
}

/// Build the status-bar line from the statusLine stdin JSON. Prefers Claude's own
/// `context_window.used_percentage`; falls back to estimating from the transcript.
fn statusline_from(cfg: &Config, raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let cw = v.get("context_window");
    let pct = cw
        .and_then(|c| c.get("used_percentage"))
        .and_then(serde_json::Value::as_f64);
    let tokens = cw.and_then(|c| {
        let inp = c.get("total_input_tokens").and_then(serde_json::Value::as_u64);
        let out = c.get("total_output_tokens").and_then(serde_json::Value::as_u64);
        match (inp, out) {
            (Some(i), Some(o)) => Some(i + o),
            (Some(i), None) => Some(i),
            _ => None,
        }
    });
    if let Some(p) = pct {
        return Some(usage::line(cfg, p.round() as u64, tokens));
    }
    // Fallback: estimate from the transcript when Claude didn't supply a %.
    let path = v.get("transcript_path").and_then(serde_json::Value::as_str)?;
    let (t, _src) = transcript::estimate_tokens(path)?;
    Some(usage::line(cfg, usage::pct_from_tokens(cfg, t), Some(t)))
}

const SAMPLE_CONFIG: &str = r#"# ctxrot configuration
# store_dir can point at an Obsidian vault folder.
store_dir = "~/.ctxrot/store"
state_dir = "~/.ctxrot/state"

# Budget denominator for the % estimate. Set this to the EFFECTIVE CAP you want
# to stay under (the target), NOT your model's real context window. ctxrot is a
# "keep it under 200K" guard: leaving this at 200000 makes the 50/75/90% bands
# fire at ~100K/150K/180K. If you raise it to your real 1M window, the bands
# won't fire until ~950K and the whole point of the tool is lost.
context_window = 200000

# a local file at/above this many bytes counts as a "large reference"
large_file_bytes = 50000

# a tool output at/above this many bytes triggers the PostToolUse warning
huge_tool_output_bytes = 50000

# PreToolUse hard gate: an UNBOUNDED `Read` (no `limit`) of a local file at/above
# this many bytes is denied, steering the model to a sub-agent or a bounded slice.
# These are almost always logs/dumps/minified blobs. Set 0 to disable the gate.
gate_file_bytes = 1000000

# append one JSONL metrics line per hook event to <state_dir>/metrics.jsonl
# (budget trajectory, band crossings, note sizes, gate denies). Inspect with
# `ctxrot metrics`. Local only; set false (or env GUARD_METRICS=0) to disable.
metrics = true

# ascending fractions of the window that trigger escalating advice
bands = [0.50, 0.75, 0.90]
"#;

fn init() -> anyhow::Result<()> {
    let cfg = Config::load();
    std::fs::create_dir_all(&cfg.store_dir)?;
    std::fs::create_dir_all(&cfg.state_dir)?;
    let path = Config::config_path();
    if path.exists() {
        println!("config already exists: {}", path.display());
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, SAMPLE_CONFIG)?;
        println!("wrote {}", path.display());
    }
    println!("store_dir: {}", cfg.store_dir.display());
    println!("state_dir: {}", cfg.state_dir.display());
    println!(
        "context_window: {} (= effective cap / target; NOT your model's real window — \
         keep it at the limit you want to stay under, e.g. 200000)",
        cfg.context_window
    );
    Ok(())
}
