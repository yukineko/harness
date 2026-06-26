mod config;
mod goal_link;
mod hypothesis;
mod install;
mod store;

mod hooks {
    pub mod session_start;
}

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "hypothesis", about = "PDO hypothesis lifecycle management")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a new hypothesis
    Add {
        text: String,
        #[arg(long)]
        goal: Option<String>,
    },
    /// Mark a hypothesis as validated
    Validate {
        id: String,
        #[arg(long)]
        evidence: Vec<String>,
    },
    /// Mark a hypothesis as rejected
    Reject {
        id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// List hypotheses
    List {
        #[arg(long)]
        status: Option<String>,
    },
    /// Install SessionStart hook
    Install {
        #[arg(long)]
        dry_run: bool,
    },
    /// Uninstall SessionStart hook
    Uninstall,
    /// Run as SessionStart hook (internal)
    SessionStart,
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::Config::load()?;

    match cli.command {
        Command::Add { text, goal } => {
            let mut st = store::Store::load(&cfg)?;
            let id = st.add(text, goal)?;
            println!("{id}");
        }
        Command::Validate { id, evidence } => {
            let mut st = store::Store::load(&cfg)?;
            st.validate(&id, evidence)?;
        }
        Command::Reject { id, reason } => {
            let mut st = store::Store::load(&cfg)?;
            st.reject(&id, reason)?;
        }
        Command::List { status } => {
            let st = store::Store::load(&cfg)?;
            for h in st.list(status.as_deref()) {
                println!("[{}] {} — {}", h.status, h.id, h.text);
            }
        }
        Command::Install { dry_run } => {
            install::install(dry_run)?;
        }
        Command::Uninstall => {
            install::uninstall()?;
        }
        Command::SessionStart => {
            if let Some(output) = hooks::session_start::run(&cfg)? {
                print!("{output}");
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hypothesis: {e:#}");
        std::process::exit(1);
    }
}
