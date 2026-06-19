//! The knowledge store: atomic markdown notes with TOML frontmatter (`+++`
//! fences, parsed with the `toml` crate — no extra YAML dep). Project notes live
//! under `<store>/<project>/` (cwd basename + a stable hash, so same-named dirs
//! don't collide); shared notes under `<store>/_global/`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Explicit high-weight trigger terms.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// "project" or "global" (informational; location is authoritative).
    #[serde(default)]
    pub scope: String,
    /// Always inject regardless of relevance (core conventions).
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub created: String,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub slug: String,
    /// Source file; kept for tooling/debugging even when unused by retrieval.
    #[allow(dead_code)]
    pub path: PathBuf,
    pub global: bool,
    pub meta: Meta,
    pub body: String,
}

impl Note {
    /// Roughly how many chars this note adds when injected.
    pub fn injected_len(&self) -> usize {
        self.meta.title.chars().count() + self.body.chars().count() + 8
    }
}

pub struct Store {
    pub store_dir: PathBuf,
    pub include_global: bool,
}

fn hash8(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", (h.finish() & 0xffff_ffff) as u32)
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    // Unicode-aware: keep letters/digits of any script (so Japanese titles get a
    // readable, distinct slug instead of all collapsing to the fallback).
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("note");
    }
    out.chars().take(48).collect()
}

impl Store {
    pub fn new(cfg: &Config) -> Self {
        Store {
            store_dir: cfg.store_dir.clone(),
            include_global: cfg.include_global,
        }
    }

    /// `<store>/<basename>-<hash>` for a project root.
    pub fn project_dir(&self, root: &Path) -> PathBuf {
        let base = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        let key = root.to_string_lossy();
        self.store_dir
            .join(format!("{}-{}", slugify(&base), hash8(&key)))
    }

    pub fn global_dir(&self) -> PathBuf {
        self.store_dir.join("_global")
    }

    /// All notes visible from `root`: project notes + (optionally) global notes.
    pub fn load_visible(&self, root: &Path) -> Vec<Note> {
        let mut notes = read_dir_notes(&self.project_dir(root), false);
        if self.include_global {
            notes.extend(read_dir_notes(&self.global_dir(), true));
        }
        notes
    }

    /// Write a note; returns its path. `global` chooses the store.
    pub fn write(
        &self,
        root: &Path,
        slug: &str,
        meta: &Meta,
        body: &str,
        global: bool,
    ) -> std::io::Result<PathBuf> {
        let dir = if global {
            self.global_dir()
        } else {
            self.project_dir(root)
        };
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{slug}.md"));
        std::fs::write(&path, render(meta, body))?;
        Ok(path)
    }

    /// Remove a note by slug (project first, then global). Returns the path.
    pub fn remove(&self, root: &Path, slug: &str) -> Option<PathBuf> {
        for dir in [self.project_dir(root), self.global_dir()] {
            let p = dir.join(format!("{slug}.md"));
            if p.exists() && std::fs::remove_file(&p).is_ok() {
                return Some(p);
            }
        }
        None
    }
}

fn read_dir_notes(dir: &Path, global: bool) -> Vec<Note> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some((meta, body)) = parse(&text) {
            let slug = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push(Note {
                slug,
                path,
                global,
                meta,
                body,
            });
        }
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

/// Parse `+++ <toml> +++ <body>`. Body-only files get an empty meta.
fn parse(text: &str) -> Option<(Meta, String)> {
    let t = text.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("+++") {
        if let Some(end) = rest.find("\n+++") {
            let fm = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n').to_string();
            let meta: Meta = toml::from_str(fm.trim()).ok()?;
            return Some((meta, body.trim().to_string()));
        }
    }
    // No frontmatter: treat the whole file as body with a blank meta.
    Some((Meta::default(), t.trim().to_string()))
}

fn render(meta: &Meta, body: &str) -> String {
    let fm = toml::to_string(meta).unwrap_or_default();
    format!("+++\n{}+++\n\n{}\n", fm, body.trim())
}
