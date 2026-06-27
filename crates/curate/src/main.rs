//! curate — promote fugu-router playbooks into versioned golden eval datasets.
//!
//! The supply side of the offline eval loop: evalkit consumes goldens, but
//! nothing produced them from real verified work. fugu-router's playbook store
//! is an append-only log of verified tasks; `curate promote` distils a chosen
//! entry into an evalkit golden case, fixed into a versioned, deduplicated
//! `evals/curated/<name>.jsonl` (which evalkit discovers recursively). Mechanical
//! acceptance criteria become runnable cases; the rest become drafts a human
//! fills.
//!
//! A plain CLI (run by a human, or suggested by condukt's Phase-6 after record),
//! not a lifecycle hook.

mod derive;
mod seed;

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;

#[derive(Parser)]
#[command(
    name = "curate",
    version,
    about = "Promote fugu-router playbooks into versioned golden eval datasets for evalkit."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List promotable playbook entries (mechanical = auto-derivable to a cmd).
    Candidates(CandidatesArgs),
    /// Promote one playbook into a golden dataset.
    Promote(PromoteArgs),
}

#[derive(Args)]
struct CandidatesArgs {
    /// Playbook store to read (default: ~/.fugu-router/playbooks.jsonl).
    #[arg(long)]
    store: Option<PathBuf>,
    /// Max entries to list.
    #[arg(long, default_value_t = 20)]
    k: usize,
}

#[derive(Args)]
struct PromoteArgs {
    /// Title substring selecting the playbook (case-insensitive); most recent
    /// match wins. Omit with --latest.
    selector: Option<String>,
    /// Dataset name → evals/curated/<name>.jsonl.
    #[arg(long, default_value = "promoted")]
    dataset: String,
    /// Promote the most recently recorded playbook regardless of title.
    #[arg(long)]
    latest: bool,
    /// Emit a draft even if the criterion is mechanical (review before trust).
    #[arg(long)]
    draft: bool,
    /// Playbook store to read (default: ~/.fugu-router/playbooks.jsonl).
    #[arg(long)]
    store: Option<PathBuf>,
    /// Project root the dataset path resolves against (default: CWD).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Eval dir under root that holds curated/ (default: evals).
    #[arg(long, default_value = "evals")]
    evals_dir: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.command {
        Command::Candidates(a) => cmd_candidates(a),
        Command::Promote(a) => cmd_promote(a),
    };
    if let Err(e) = r {
        eprintln!("curate: {e:#}");
        exit(1);
    }
}

fn cmd_candidates(a: CandidatesArgs) -> Result<()> {
    let store = a.store.unwrap_or_else(seed::default_store);
    let seeds = seed::load(&store);
    if seeds.is_empty() {
        eprintln!("curate: no playbooks found in {}", store.display());
        return Ok(());
    }
    // newest first
    let mut idx: Vec<usize> = (0..seeds.len()).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse((seeds[i].ts, i)));
    for &i in idx.iter().take(a.k) {
        let s = &seeds[i];
        let kind = if derive::is_mechanical(&s.done_criteria) {
            "mech "
        } else {
            "draft"
        };
        let dc = s.done_criteria.trim();
        let dc_short = dc.chars().take(60).collect::<String>();
        println!("{kind}  {}  ::  {dc_short}", s.title);
    }
    Ok(())
}

fn cmd_promote(a: PromoteArgs) -> Result<()> {
    if a.selector.is_none() && !a.latest {
        anyhow::bail!("provide a title selector or --latest to choose a playbook");
    }
    let store = a.store.unwrap_or_else(seed::default_store);
    let seeds = seed::load(&store);
    if seeds.is_empty() {
        anyhow::bail!("no playbooks found in {}", store.display());
    }
    let idx = seed::select(&seeds, a.selector.as_deref(), a.latest).ok_or_else(|| {
        anyhow::anyhow!(
            "no playbook title matched {:?}",
            a.selector.as_deref().unwrap_or("<latest>")
        )
    })?;
    let golden = derive::derive_golden(&seeds[idx], a.draft);
    let id = golden["id"].as_str().unwrap_or_default().to_string();

    let root = a.root.unwrap_or_else(|| PathBuf::from("."));
    let dataset = root
        .join(&a.evals_dir)
        .join("curated")
        .join(format!("{}.jsonl", sanitize(&a.dataset)));

    if existing_ids(&dataset).contains(&id) {
        eprintln!(
            "curate: \"{}\" already promoted (id {id}) in {} — skipping",
            seeds[idx].title,
            dataset.display()
        );
        return Ok(());
    }

    append_golden(&dataset, &golden).with_context(|| format!("writing {}", dataset.display()))?;
    let kind = if golden.get("draft").is_some() {
        "draft (fill the assertion, then evalkit runs it)"
    } else {
        "runnable"
    };
    eprintln!(
        "curate: promoted \"{}\" → {} [{kind}]",
        seeds[idx].title,
        dataset.display()
    );
    Ok(())
}

/// Case ids already present in a dataset (for dedup). Missing file → empty.
fn existing_ids(path: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return ids;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

/// Append one golden case as a JSON line, creating the curated dir on first use.
fn append_golden(path: &Path, golden: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", serde_json::to_string(golden)?)?;
    Ok(())
}

/// Keep a dataset name filesystem-safe.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_ids_collects_and_skips_comments() {
        let dir = std::env::temp_dir().join(format!("curate-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("d.jsonl");
        std::fs::write(
            &p,
            "// comment\n{\"id\":\"a\",\"draft\":true}\n\n{\"id\":\"b\",\"file\":\"x\",\"assert\":{}}\n",
        )
        .unwrap();
        let ids = existing_ids(&p);
        assert!(ids.contains("a") && ids.contains("b"));
        assert_eq!(ids.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sanitize_keeps_safe_chars() {
        assert_eq!(sanitize("auth-flow_v2"), "auth-flow_v2");
        assert_eq!(sanitize("a/b c"), "a_b_c");
    }
}
