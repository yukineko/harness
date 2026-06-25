mod config;
mod hooks;
mod install;
mod lock;
mod store;
mod task;

use anyhow::Result;
use clap::{Parser, Subcommand};
use harness_core::hook::{read_stdin, run_hook, HookInput};
use serde_json::json;

#[derive(Parser)]
#[command(name = "backlog", about = "Cross-project task queue for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a new task to the backlog
    Add {
        /// Task title
        #[arg(long)]
        title: String,

        /// Project path
        #[arg(long)]
        project: String,

        /// Tags (can be specified multiple times)
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,

        /// Priority shortcut: p0, p1, or p2 (added as a tag)
        #[arg(long)]
        priority: Option<String>,

        /// Notes
        #[arg(long, default_value = "")]
        notes: String,
    },

    /// List tasks
    List {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,

        /// Filter by project path
        #[arg(long)]
        project: Option<String>,

        /// Filter by status
        #[arg(long)]
        status: Option<String>,
    },

    /// Show the next highest-priority pending task
    Next {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,

        /// Filter by project path
        #[arg(long)]
        project: Option<String>,
    },

    /// Mark a task as done
    Done {
        /// Task ID
        id: String,
    },

    /// Mark a task as failed
    Fail {
        /// Task ID
        id: String,

        /// Failure reason
        #[arg(long)]
        reason: Option<String>,
    },

    /// Edit a task's fields
    Edit {
        /// Task ID
        id: String,

        /// New title
        #[arg(long)]
        title: Option<String>,

        /// New tags (replaces existing tags)
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,

        /// New notes
        #[arg(long)]
        notes: Option<String>,

        /// New status
        #[arg(long)]
        status: Option<String>,
    },

    /// SessionStart hook: reads stdin JSON and injects pending tasks as context
    SessionStart,

    /// Install hooks into ~/.claude/settings.json
    Install {
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove hooks from ~/.claude/settings.json
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },

    /// Manage the ~/.backlog/run.lock exclusive lock
    Lock {
        #[command(subcommand)]
        action: LockAction,
    },
}

#[derive(Subcommand)]
enum LockAction {
    /// Acquire the lock (errors if already active)
    Acquire {
        /// Session ID
        #[arg(long)]
        session_id: String,

        /// Project path
        #[arg(long)]
        project: String,
    },

    /// Release the lock (no-op if none)
    Release,

    /// Print lock status as JSON, or "none"
    Status,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let cfg = config::Config::load();
    let tasks_path = cfg.tasks_path();

    match cli.command {
        Command::Add {
            title,
            project,
            mut tags,
            priority,
            notes,
        } => {
            // priority is a shortcut for adding a priority tag
            if let Some(p) = priority {
                if !tags.contains(&p) {
                    tags.push(p);
                }
            }
            let now = now_unix();
            let id = store::add(&tasks_path, &title, &project, tags, &notes, now)?;
            println!("added: {id}");
        }

        Command::List {
            tag,
            project,
            status,
        } => {
            let tasks = store::list(
                &tasks_path,
                tag.as_deref(),
                project.as_deref(),
                status.as_deref(),
            )?;

            if tasks.is_empty() {
                println!("no tasks");
            } else {
                println!("{:<10} {:<10} {:<10} {}", "ID", "PRIORITY", "STATUS", "TITLE");
                for t in &tasks {
                    let priority_str = match t.priority() {
                        0 => "p0",
                        1 => "p1",
                        2 => "p2",
                        _ => "-",
                    };
                    println!(
                        "{:<10} {:<10} {:<10} {}",
                        t.id, priority_str, t.status, t.title
                    );
                }
            }
        }

        Command::Next { tag, project } => {
            let task = store::next(&tasks_path, tag.as_deref(), project.as_deref())?;
            match task {
                Some(t) => {
                    println!("{}", serde_json::to_string_pretty(&t)?);
                }
                None => {
                    println!("no pending tasks");
                }
            }
        }

        Command::Done { id } => {
            store::mark_done(&tasks_path, &id)?;
            println!("done: {id}");
        }

        Command::Fail { id, reason } => {
            store::mark_failed(&tasks_path, &id, reason.as_deref())?;
            println!("failed: {id}");
        }

        Command::Edit {
            id,
            title,
            tags,
            notes,
            status,
        } => {
            let tags_opt = if tags.is_empty() { None } else { Some(tags) };
            store::edit(
                &tasks_path,
                &id,
                title.as_deref(),
                tags_opt,
                notes.as_deref(),
                status.as_deref(),
            )?;
            println!("updated: {id}");
        }

        Command::SessionStart => {
            run_hook(|| {
                let raw = read_stdin();
                let input = HookInput::parse(&raw).unwrap_or_default();
                if let Some(ctx) = hooks::session_start::run(&input) {
                    println!("{}", json!({ "additionalContext": ctx }));
                }
            });
        }

        Command::Install { dry_run } => {
            install::install(dry_run)?;
        }

        Command::Uninstall { dry_run } => {
            install::uninstall(dry_run)?;
        }

        Command::Lock { action } => match action {
            LockAction::Acquire {
                session_id,
                project,
            } => {
                let pid = std::process::id();
                lock::acquire(&session_id, pid, &project)?;
                println!("lock acquired");
            }
            LockAction::Release => {
                lock::release()?;
                println!("lock released");
            }
            LockAction::Status => {
                match lock::status() {
                    lock::LockStatus::None => println!("none"),
                    lock::LockStatus::Active(info) => {
                        println!("{}", serde_json::to_string_pretty(&info)?);
                    }
                    lock::LockStatus::Stale(info) => {
                        // Print the info with an extra stale field
                        let mut v = serde_json::to_value(&info)?;
                        v.as_object_mut()
                            .unwrap()
                            .insert("stale".to_string(), serde_json::Value::Bool(true));
                        println!("{}", serde_json::to_string_pretty(&v)?);
                    }
                }
            }
        },
    }

    Ok(())
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
