//! deepwiki — repository architecture wiki for Claude Code.
//!
//! This binary is the deterministic half: `scan` maps the repo, `status`/`stamp`
//! track whether the generated wiki is still fresh against git. The generative
//! half lives in the plugin's `/deepwiki` command, which runs `scan`, hands the
//! map to the `deepwiki-writer` subagent to write `.deepwiki/*.md` pages, then
//! `stamp`s the commit. Keeping generation in a subagent keeps the heavy repo
//! reading out of the main conversation.

mod meta;
mod scan;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "deepwiki",
    version,
    about = "Repository architecture wiki for Claude Code — scan the repo and track wiki freshness."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Map the repository (languages, layout, entry points, key files).
    Scan {
        /// Emit JSON instead of markdown.
        #[arg(long)]
        json: bool,
        /// Repo root (defaults to cwd).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Report whether the generated wiki is fresh vs the current commit.
    Status {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Record the current commit as the wiki's build point (after generating).
    Stamp {
        /// Page filenames written under .deepwiki/ (e.g. overview.md).
        pages: Vec<String>,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Create the .deepwiki/ directory.
    Init {
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.command {
        Command::Scan { json, root } => scan_cmd(root.unwrap_or_else(cwd), json),
        Command::Status { root } => status_cmd(root.unwrap_or_else(cwd)),
        Command::Stamp { pages, root } => meta::stamp(&root.unwrap_or_else(cwd), pages),
        Command::Init { root } => init_cmd(root.unwrap_or_else(cwd)),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn scan_cmd(root: PathBuf, json: bool) -> anyhow::Result<()> {
    let map = scan::scan(&root);
    if json {
        println!("{}", serde_json::to_string_pretty(&map)?);
    } else {
        print!("{}", scan::render_markdown(&map));
    }
    Ok(())
}

fn status_cmd(root: PathBuf) -> anyhow::Result<()> {
    let Some(m) = meta::load(&root) else {
        println!(
            "no wiki yet — run `/deepwiki` to generate one in {}/",
            meta::WIKI_DIR
        );
        return Ok(());
    };
    println!("wiki built: {}  ({} pages)", m.built_at, m.pages.len());
    println!("built at commit: {}", short(&m.sha));
    match meta::head_sha(&root) {
        None => println!("status: not a git repo (can't check freshness)"),
        Some(head) if head == m.sha => println!("status: ✅ fresh (HEAD matches)"),
        Some(head) => {
            let changed = meta::changed_since(&root, &m.sha);
            let src: Vec<&String> = changed.iter().filter(|f| is_sourceish(f)).collect();
            println!("status: ⚠ stale — HEAD is now {}", short(&head));
            println!(
                "{} file(s) changed since, {} source-ish:",
                changed.len(),
                src.len()
            );
            for f in src.iter().take(20) {
                println!("  {f}");
            }
            if src.len() > 20 {
                println!("  … and {} more", src.len() - 20);
            }
            println!("\nrun `/deepwiki` to refresh.");
        }
    }
    Ok(())
}

fn init_cmd(root: PathBuf) -> anyhow::Result<()> {
    let dir = meta::wiki_dir(&root);
    std::fs::create_dir_all(&dir)?;
    println!("created {}", dir.display());
    println!("run `/deepwiki` to generate the wiki pages, or `deepwiki scan` to see the repo map.");
    Ok(())
}

fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}

fn is_sourceish(path: &str) -> bool {
    let p = path.to_lowercase();
    if p.starts_with(".deepwiki/") {
        return false;
    }
    [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rb", ".java", ".kt", ".swift", ".c",
        ".h", ".cc", ".cpp", ".cs", ".php", ".scala", ".sh", ".toml", ".json", ".yaml", ".yml",
        ".sql",
    ]
    .iter()
    .any(|ext| p.ends_with(ext))
}
