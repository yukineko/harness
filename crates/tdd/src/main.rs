//! tdd — a test-first gate for Claude Code.
//!
//! `gate` is the **Stop** hook: it blocks the stop when implementation code was
//! added with no accompanying test (you must write tests), and feeds the reason
//! back so the agent writes one and continues. `red` / `green` / `verify` make
//! test-first a *verifiable* artifact for the `/tdd` skill.
//!
//! Failure modes are split deliberately:
//!   * a *harness* error (our own bug, unreadable git) → exit 0, allow the stop.
//!     tdd must never trap a turn because it broke.
//!   * a *missing test* → block on purpose, with an actionable reason.

mod config;
mod gate;
mod git;
mod install;
mod model;
mod proof;
mod runner;
mod state;

use std::io::Read;
use std::path::Path;

use clap::{Parser, Subcommand};
use serde_json::json;

use config::Config;
use model::HookInput;

#[derive(Parser)]
#[command(
    name = "tdd",
    version,
    about = "Test-first gate for Claude Code: block stops when code lands without a test; make RED→GREEN verifiable."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stop hook: block the stop when implementation changed without a test.
    Gate,
    /// Run the tests and require them to FAIL; record the RED proof (test-first).
    Red {
        #[arg(long)]
        task: String,
        /// Test command (defaults to config `test_cmd`).
        #[arg(long)]
        cmd: Option<String>,
    },
    /// Require a RED proof, run the tests, require them to PASS; record GREEN.
    Green {
        #[arg(long)]
        task: String,
        #[arg(long)]
        cmd: Option<String>,
    },
    /// Exit 0 iff both RED and GREEN proofs exist for the task.
    Verify {
        #[arg(long)]
        task: String,
    },
    /// Write a starter ./tdd.toml.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Merge the tdd Stop hook into ~/.claude/settings.json.
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the tdd Stop hook from ~/.claude/settings.json.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Trust this project so its `tdd.toml` `test_cmd` is honored (it is executed
    /// verbatim by `tdd red`/`tdd green`; untrusted by default).
    Trust,
    /// Show the resolved config + what the gate would do for the cwd.
    Status,
}

fn read_stdin() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Gate => gate_command(),
        Command::Red { task, cmd } => proof_command(&task, &cmd, true),
        Command::Green { task, cmd } => proof_command(&task, &cmd, false),
        Command::Verify { task } => verify_command(&task),
        Command::Init { force } => exit_on_err(init(force)),
        Command::Install { dry_run } => exit_on_err(install::install(dry_run)),
        Command::Uninstall { dry_run } => exit_on_err(install::uninstall(dry_run)),
        Command::Trust => exit_on_err(trust_project()),
        Command::Status => status(),
    }
}

fn exit_on_err(r: anyhow::Result<()>) {
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// The Stop hook. Always exits 0 toward Claude (the `decision` field, not the
/// exit code, blocks a stop). Returns exit 1 only in manual CLI mode.
fn gate_command() -> ! {
    let raw = read_stdin();
    let hook = HookInput::parse(&raw);
    let interactive = hook.is_none();
    let input = hook.unwrap_or_default();
    let root = input.cwd_or_current();

    if Config::disabled_env() {
        if interactive {
            eprintln!("tdd: disabled (TDD_DISABLE)");
        }
        std::process::exit(0);
    }

    let cfg = Config::load(&root);
    if !cfg.enabled {
        if interactive {
            eprintln!("tdd: disabled in config");
        }
        std::process::exit(0);
    }

    let session = input.session_key();

    if let Some(reason) = consume_skip(&root) {
        state::reset(&cfg.state_dir, &session);
        log_event(&cfg, &session, "skip", 0);
        eprintln!("tdd: .tdd-skip consumed — allowing stop ({reason})");
        std::process::exit(0);
    }

    let verdict = gate::evaluate(&cfg, &root);

    if !verdict.blocks(&cfg) {
        state::reset(&cfg.state_dir, &session);
        log_event(&cfg, &session, "ok", 0);
        if interactive {
            println!("{}", gate::human_report(&verdict, &cfg));
        }
        std::process::exit(0);
    }

    // a blocking finding: no test for new implementation
    let attempt = state::bump(&cfg.state_dir, &session, cfg.reset_after_secs);

    if attempt > cfg.max_attempts {
        state::reset(&cfg.state_dir, &session);
        log_event(&cfg, &session, "giveup", attempt);
        eprintln!(
            "tdd: still no test after {} attempts — allowing stop. Add one or set TDD_DISABLE=1.",
            cfg.max_attempts
        );
        std::process::exit(0);
    }

    log_event(&cfg, &session, "blocked", attempt);
    let reason = gate::block_reason(&verdict, attempt, cfg.max_attempts);

    if interactive {
        eprintln!("{}", gate::human_report(&verdict, &cfg));
        eprintln!("\n{reason}");
        std::process::exit(1);
    }
    println!("{}", json!({ "decision": "block", "reason": reason }));
    std::process::exit(0);
}

/// `tdd red` / `tdd green`. Manual CLI: non-zero exit signals the skill that the
/// phase precondition (must-fail / must-pass) was not met.
fn proof_command(task: &str, cmd: &Option<String>, is_red: bool) -> ! {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let r = if is_red {
        proof::red(&root, &cfg, task, cmd)
    } else {
        proof::green(&root, &cfg, task, cmd)
    };
    match r {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("tdd: {e}");
            std::process::exit(1);
        }
    }
}

fn verify_command(task: &str) -> ! {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    if proof::verify(&root, &cfg, task) {
        println!("✓ tdd: `{task}` has both RED and GREEN proofs (test-first verified)");
        std::process::exit(0);
    }
    eprintln!(
        "✗ tdd: `{task}` is missing a RED or GREEN proof. Run `tdd red --task {task}` then \
         `tdd green --task {task}`."
    );
    std::process::exit(1);
}

/// `.tdd-skip` in the project root: consumed once, returns its reason.
fn consume_skip(root: &Path) -> Option<String> {
    let p = root.join(".tdd-skip");
    if !p.exists() {
        return None;
    }
    let reason = std::fs::read_to_string(&p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no reason given)".to_string());
    let _ = std::fs::remove_file(&p);
    Some(reason)
}

fn log_event(cfg: &Config, session: &str, verdict: &str, attempt: u32) {
    let path = cfg.state_dir.join("log.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = json!({
        "ts": chrono::Local::now().to_rfc3339(),
        "session": session,
        "verdict": verdict,
        "attempt": attempt,
    });
    if let (Ok(line), Ok(mut f)) = (
        serde_json::to_string(&entry),
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path),
    ) {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

fn status() {
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let cfg = Config::load(&root);
    let src = if Config::project_path(&root).exists() {
        Config::project_path(&root)
    } else if Config::home_path().exists() {
        Config::home_path()
    } else {
        Path::new("(defaults — no config file)").to_path_buf()
    };
    println!("config:        {}", src.display());
    println!("enabled:       {}", cfg.enabled);
    println!("max_attempts:  {}", cfg.max_attempts);
    println!("min impl lines:{}", cfg.min_added_impl_lines);
    println!("test_cmd:      {}", cfg.test_cmd);
    println!("proof_dir:     {}", cfg.proof_dir);
    println!("state_dir:     {}", cfg.state_dir.display());
    println!();
    let verdict = gate::evaluate(&cfg, &root);
    println!("{}", gate::human_report(&verdict, &cfg));
}

/// `tdd trust`: add the current project root to the shared workspace-trust list
/// so its `tdd.toml` `test_cmd` is honored by `tdd red`/`tdd green`.
fn trust_project() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let key = harness_core::trust::add(&root)?;
    println!("✓ trusted {}", key.display());
    let p = Config::project_path(&root);
    if p.exists() {
        println!("tdd will now honor test_cmd from {}", p.display());
    } else {
        println!("(no {} yet — create one with `tdd init`)", p.display());
    }
    Ok(())
}

fn init(force: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let path = Config::project_path(&root);
    if path.exists() && !force {
        anyhow::bail!("{} already exists (use --force to overwrite)", path.display());
    }
    std::fs::write(&path, STARTER_CONFIG)?;
    println!("wrote {}", path.display());
    println!("Then `tdd install` once to wire the Stop hook (or install the plugin).");
    Ok(())
}

const STARTER_CONFIG: &str = r#"# tdd.toml — test-first gate for Claude Code.
#
# On Stop, `tdd gate` blocks the turn when implementation code was ADDED without
# an accompanying test (an inline #[test]/def test_/func Test/it(...), or a file
# under tests/). The block is fed back so the agent writes the test and finishes.
#
# `tdd red`/`tdd green` (driven by the /tdd skill) make test-first verifiable:
# red REQUIRES the tests to fail first; green REQUIRES a prior red then a pass.

enabled = true
max_attempts = 3            # give up (allow stop) after N consecutive blocks
reset_after_secs = 600      # attempt counter resets after this idle gap
min_added_impl_lines = 1    # need a test once this many impl lines are added

# Default command for `tdd red` / `tdd green` (override per call with --cmd).
test_cmd = "cargo test"

# Where RED/GREEN proof artifacts are written (relative to the project root).
proof_dir = ".tdd"

# Files that count as implementation / as tests. Defaults cover Rust, Python,
# TS/JS, Go, Java, Ruby, C/C++, Kotlin, Swift — override to taste.
# impl_globs = ["**/*.rs", "**/*.py", "**/*.ts"]
# test_path_globs = ["**/tests/**", "**/test_*.py", "**/*.spec.*"]
# test_markers = ['#\[\s*test', '\bdef\s+test_', '\bfunc\s+Test\w']
"#;
