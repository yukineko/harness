//! Cross-session backlog: open issues / TODOs accumulated across sessions.
//!
//! Stored as a single JSON array under the state dir (`backlog.json`) and
//! rendered to ONE global Obsidian note (`<vault>/backlog.md`). Resolved items
//! are removed from the note; in the store their `status` flips to `done` so the
//! history is kept (and a re-`add` of the same text reopens rather than
//! duplicates). Driven by the `/record` flow (`add` / `resolve` / `render`) and
//! surfaced at SessionStart (`brief`).
//!
//! Like the record note, rendering never creates the vault — it is a no-op when
//! the vault directory is absent. The store itself is local and always written.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Config;

const NOTE_NAME: &str = "backlog.md";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub project: String,
    /// "open" | "done".
    #[serde(default = "open_status")]
    pub status: String,
    #[serde(default)]
    pub created_session: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
}

fn open_status() -> String {
    "open".to_string()
}

impl Item {
    pub fn is_open(&self) -> bool {
        self.status == "open"
    }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

fn store_path(cfg: &Config) -> PathBuf {
    cfg.state_dir.join("backlog.json")
}

/// Stable id from (project, normalized text) so re-adding the same item is
/// idempotent. `DefaultHasher::new()` is seeded with fixed keys, so the hash is
/// deterministic across runs and processes.
pub fn make_id(project: &str, text: &str) -> String {
    let mut h = DefaultHasher::new();
    project.trim().hash(&mut h);
    0u8.hash(&mut h);
    text.trim().hash(&mut h);
    format!("bk-{:08x}", h.finish() as u32)
}

pub fn load(cfg: &Config) -> Vec<Item> {
    std::fs::read_to_string(store_path(cfg))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config, items: &[Item]) {
    let p = store_path(cfg);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(p, text);
    }
}

/// Upsert an open item. Idempotent by id: re-adding the same text under the same
/// project updates the text/timestamp; if it had been resolved, it reopens.
/// Returns the item id.
pub fn add(items: &mut Vec<Item>, project: &str, text: &str, session: &str) -> String {
    let text = text.trim().to_string();
    let id = make_id(project, &text);
    let ts = now();
    if let Some(it) = items.iter_mut().find(|i| i.id == id) {
        it.text = text;
        it.status = "open".to_string();
        it.resolved_at = None;
        it.updated_at = ts;
    } else {
        items.push(Item {
            id: id.clone(),
            text,
            project: project.to_string(),
            status: "open".to_string(),
            created_session: session.to_string(),
            created_at: ts.clone(),
            updated_at: ts,
            resolved_at: None,
        });
    }
    id
}

/// Mark the given ids resolved (status → done). Returns the count actually
/// flipped from open to done (already-done / unknown ids are ignored).
pub fn resolve(items: &mut [Item], ids: &[String]) -> usize {
    let ts = now();
    let mut n = 0;
    for it in items.iter_mut() {
        if it.is_open() && ids.iter().any(|id| id == &it.id) {
            it.status = "done".to_string();
            it.resolved_at = Some(ts.clone());
            it.updated_at = ts.clone();
            n += 1;
        }
    }
    n
}

/// Open items, optionally filtered by project, oldest first.
pub fn open_items<'a>(items: &'a [Item], project: Option<&str>) -> Vec<&'a Item> {
    let mut v: Vec<&Item> = items
        .iter()
        .filter(|i| i.is_open())
        .filter(|i| project.map(|p| i.project == p).unwrap_or(true))
        .collect();
    v.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.text.cmp(&b.text))
    });
    v
}

fn date_only(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
}

/// Render the global backlog note body (open items grouped by project). Pure —
/// no I/O — so it is unit-testable.
pub fn render_body(items: &[Item], generated_at: &str) -> String {
    let open = open_items(items, None);
    let mut s = String::new();
    s.push_str("# 課題バックログ\n\n");
    s.push_str(
        "> session-insights が自動生成。解決した項目は除外されます。手で編集しないでください。\n",
    );
    s.push_str(&format!("> 最終更新: {}   未解決: {} 件\n\n", generated_at, open.len()));
    if open.is_empty() {
        s.push_str("未解決の課題はありません。🎉\n");
        return s;
    }
    // Group by project, projects sorted alphabetically.
    let mut projects: Vec<&str> = open.iter().map(|i| i.project.as_str()).collect();
    projects.sort_unstable();
    projects.dedup();
    for proj in projects {
        let heading = if proj.is_empty() { "(no project)" } else { proj };
        s.push_str(&format!("## {}\n\n", heading));
        for it in open.iter().filter(|i| i.project == proj) {
            s.push_str(&format!(
                "- [ ] `{}` {}  <small>(since {})</small>\n",
                it.id,
                it.text,
                date_only(&it.created_at)
            ));
        }
        s.push('\n');
    }
    s
}

/// Write the backlog note to `<vault>/backlog.md`. No-op (returns None) when the
/// vault directory does not exist — we never create the vault.
pub fn render_note(cfg: &Config, items: &[Item]) -> Option<PathBuf> {
    if !cfg.obsidian_vault.is_dir() {
        return None;
    }
    let path = cfg.obsidian_vault.join(NOTE_NAME);
    let body = render_body(items, &now());
    std::fs::write(&path, body).ok()?;
    Some(path)
}

/// Short SessionStart summary for one project (or all). Empty string when there
/// is nothing open, so the hook injects no noise.
pub fn brief(items: &[Item], project: Option<&str>, max: usize) -> String {
    let open = open_items(items, project);
    if open.is_empty() {
        return String::new();
    }
    let label = project.unwrap_or("all");
    let mut s = format!("📋 課題バックログ ({}): 未解決 {} 件\n", label, open.len());
    for it in open.iter().take(max) {
        s.push_str(&format!("  - [{}] {}\n", it.id, it.text));
    }
    if open.len() > max {
        s.push_str(&format!("  …他 {} 件 (backlog.md 参照)\n", open.len() - max));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_is_idempotent_by_text() {
        let mut v = Vec::new();
        let id1 = add(&mut v, "harness", " fix the thing ", "s1");
        let id2 = add(&mut v, "harness", "fix the thing", "s2");
        assert_eq!(id1, id2, "same project+text → same id");
        assert_eq!(v.len(), 1, "no duplicate item");
        // Different project → different id.
        let id3 = add(&mut v, "other", "fix the thing", "s1");
        assert_ne!(id1, id3);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn resolve_removes_from_open_and_note() {
        let mut v = Vec::new();
        let id = add(&mut v, "harness", "ship feature", "s1");
        add(&mut v, "harness", "write docs", "s1");
        assert_eq!(open_items(&v, None).len(), 2);
        let n = resolve(&mut v, std::slice::from_ref(&id));
        assert_eq!(n, 1);
        let open = open_items(&v, None);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].text, "write docs");
        // Store keeps the resolved item for history.
        assert_eq!(v.len(), 2);
        // Resolving an already-done id is a no-op.
        assert_eq!(resolve(&mut v, std::slice::from_ref(&id)), 0);
        // The rendered note must not mention the resolved item.
        let body = render_body(&v, "2026-06-25");
        assert!(body.contains("write docs"));
        assert!(!body.contains("ship feature"));
        assert!(!body.contains(&id));
    }

    #[test]
    fn readd_resolved_text_reopens() {
        let mut v = Vec::new();
        let id = add(&mut v, "harness", "flaky test", "s1");
        resolve(&mut v, std::slice::from_ref(&id));
        assert_eq!(open_items(&v, None).len(), 0);
        let id2 = add(&mut v, "harness", "flaky test", "s2");
        assert_eq!(id, id2);
        assert_eq!(open_items(&v, None).len(), 1, "re-add reopens, no dup");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn brief_is_empty_when_nothing_open() {
        let v: Vec<Item> = Vec::new();
        assert_eq!(brief(&v, Some("harness"), 5), "");
    }

    #[test]
    fn brief_filters_by_project_and_caps() {
        let mut v = Vec::new();
        for i in 0..4 {
            add(&mut v, "harness", &format!("task {i}"), "s1");
        }
        add(&mut v, "other", "elsewhere", "s1");
        let b = brief(&v, Some("harness"), 2);
        assert!(b.contains("未解決 4 件"));
        assert!(b.contains("他 2 件"));
        assert!(!b.contains("elsewhere"));
    }
}
