mod config;
mod install;
mod progress;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;

#[derive(Parser)]
#[command(name = "taskprog", about = "Progress-file plugin for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// SessionStart hook: inject progress file as additionalContext
    SessionStart,
    /// Stop hook: prompt LLM to update the progress file
    Stop,
    /// Print the current progress file
    Show,
    /// Write content to the progress file (reads from stdin)
    Write {
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// Init a starter taskprog.toml in the current directory
    Init {
        #[arg(default_value = "taskprog.toml")]
        target: String,
    },
    /// Merge hooks into ~/.claude/settings.json
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove taskprog hooks from ~/.claude/settings.json
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Show resolved config
    Status,
}

fn session_start_hook(input: &HookInput) -> Result<Option<String>> {
    if config::disabled_env() {
        return Ok(None);
    }
    let cwd = if input.cwd.is_empty() { "." } else { &input.cwd };
    let cfg = config::Config::load(cwd);
    if !cfg.enabled {
        return Ok(None);
    }
    let path = cfg.resolve_progress_path(cwd);
    Ok(progress::build_context(&path, cfg.inject_limit))
}

fn stop_hook(input: &HookInput) -> Result<()> {
    if config::disabled_env() {
        return Ok(());
    }
    let cwd = if input.cwd.is_empty() { "." } else { &input.cwd };
    let cfg = config::Config::load(cwd);
    if !cfg.enabled {
        return Ok(());
    }
    update::on_stop(input, &cfg)
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::SessionStart => {
            run_hook(|| {
                let raw = read_stdin();
                let input = HookInput::parse(&raw).unwrap_or_default();
                if let Some(ctx) = session_start_hook(&input).unwrap_or(None) {
                    println!("{}", json!({ "additionalContext": ctx }));
                }
            });
        }
        Command::Stop => {
            run_hook(|| {
                let raw = read_stdin();
                let input = HookInput::parse(&raw).unwrap_or_default();
                let _ = stop_hook(&input);
            });
        }
        Command::Show => {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string());
            let cfg = config::Config::load(&cwd);
            let path = cfg.resolve_progress_path(&cwd);
            match progress::read_file(&path, 0) {
                Some(s) => print!("{s}"),
                None => eprintln!("No progress file at {}", path.display()),
            }
        }
        Command::Write { cwd } => {
            let cfg = config::Config::load(&cwd);
            let path = cfg.resolve_progress_path(&cwd);
            let mut content = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)
                .expect("failed to read stdin");
            update::write_progress(&path, &content).expect("write failed");
            println!("wrote {}", path.display());
        }
        Command::Init { target } => {
            config::init_config(&target).unwrap_or_else(|e| eprintln!("Error: {e}"));
        }
        Command::Install { dry_run } => {
            install::install(dry_run).unwrap_or_else(|e| eprintln!("Error: {e}"));
        }
        Command::Uninstall { dry_run } => {
            install::uninstall(dry_run).unwrap_or_else(|e| eprintln!("Error: {e}"));
        }
        Command::Status => {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string());
            let cfg = config::Config::load(&cwd);
            let path = cfg.resolve_progress_path(&cwd);
            println!("enabled:        {}", cfg.enabled);
            println!("progress_file:  {}", path.display());
            println!("inject_limit:   {}", cfg.inject_limit);
            println!("file_exists:    {}", path.exists());
        }
    }
}
