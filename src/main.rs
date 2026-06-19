//! ctxrot — a context-rot guard for Claude Code.
//!
//! One binary, one subcommand per hook. Hook subcommands read the event JSON
//! from stdin and emit the appropriate output. The cardinal rule: a hook must
//! NEVER break the user's turn — on any error we exit 0 and stay silent.

mod config;
mod hooks;
mod install;
mod model;
mod store;
mod transcript;

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
                NoteAction::Write { slug, cwd } => {
                    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
                    let body = read_stdin();
                    let safe: String = slug
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
                        .collect();
                    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                    match store.write_note(&cwd, &format!("{safe}-{stamp}"), &body) {
                        Ok(p) => println!("{}", p.display()),
                        Err(e) => {
                            eprintln!("write failed: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }
}

const SAMPLE_CONFIG: &str = r#"# ctxrot configuration
# store_dir can point at an Obsidian vault folder.
store_dir = "~/.ctxrot/store"
state_dir = "~/.ctxrot/state"

# token window used for the budget % estimate
context_window = 200000

# a local file at/above this many bytes counts as a "large reference"
large_file_bytes = 50000

# a tool output at/above this many bytes triggers the PostToolUse warning
huge_tool_output_bytes = 50000

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
    Ok(())
}
