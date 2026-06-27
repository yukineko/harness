//! Shared substrate for the context-injection plugins (`playbook`, `runbook`).
//!
//! Both are UserPromptSubmit hooks that inject context, and they share the same
//! plumbing even though their *roles* differ (playbook auto-selects relevant
//! notes; runbook expands explicit `!name` macros). Only the substrate lives
//! here — each plugin keeps its own `Config` fields and its own selection /
//! expansion logic and composes these helpers:
//!
//! * [`load_layered`] — the 3-layer config resolution (project file → global
//!   file → built-in defaults; the first existing file wins, layers are not
//!   merged), generic over the plugin's own `serde`-deserializable file struct.
//! * [`CharBudget`] — a running char-count cap over injected items: the first
//!   item is always admitted (injection is never empty just because one item is
//!   large), later items are rejected once the running total would exceed `max`.
//! * [`truncate_chars`] — char-boundary truncation with a caller-supplied
//!   ellipsis (used by runbook's per-procedure cap).

use std::path::Path;

use serde::de::DeserializeOwned;

/// Resolve a 3-layer config: project file **over** global file **over** the
/// caller's built-in defaults. The first path that exists is read and parsed
/// into `T`; if neither exists (or the chosen file can't be read or parsed),
/// `T::default()` is returned. Layers are **not** merged — this matches what the
/// plugins do today, where `T` is a struct of `Option` fields that the caller
/// then applies onto its own `Config::default()`.
pub fn load_layered<T>(project: &Path, global: &Path) -> T
where
    T: DeserializeOwned + Default,
{
    let chosen = if project.exists() {
        Some(project)
    } else if global.exists() {
        Some(global)
    } else {
        None
    };
    if let Some(path) = chosen {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(parsed) = toml::from_str::<T>(&text) {
                return parsed;
            }
        }
    }
    T::default()
}

/// A running char-count budget for injected context.
///
/// Items (notes, procedure blocks, …) are offered one at a time. The first item
/// always fits — a knowledge hook that injects nothing because the first item is
/// large is worse than one that overshoots once. Each later item fits only while
/// the running total stays within `max`. Both plugins cap injected text this
/// way: playbook over whole-note selection, runbook over per-procedure blocks.
#[derive(Debug, Clone)]
pub struct CharBudget {
    max: usize,
    used: usize,
    count: usize,
}

impl CharBudget {
    pub fn new(max: usize) -> Self {
        CharBudget {
            max,
            used: 0,
            count: 0,
        }
    }

    /// Would admitting an item of `len` chars exceed the budget? Always `false`
    /// for the first item; `true` once a later item would push `used` past `max`.
    pub fn would_overflow(&self, len: usize) -> bool {
        self.count > 0 && self.used + len > self.max
    }

    /// Record an admitted item of `len` chars.
    pub fn add(&mut self, len: usize) {
        self.used += len;
        self.count += 1;
    }

    /// Chars admitted so far.
    pub fn used(&self) -> usize {
        self.used
    }

    /// Items admitted so far.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Chars left before the cap (saturating).
    pub fn remaining(&self) -> usize {
        self.max.saturating_sub(self.used)
    }
}

/// Truncate `s` to at most `max_chars` **characters** (not bytes), appending
/// `ellipsis` when it was cut. Multibyte-safe (e.g. Japanese), so it never
/// splits a `char`.
pub fn truncate_chars(s: &str, max_chars: usize, ellipsis: &str) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str(ellipsis);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize, PartialEq)]
    struct FileCfg {
        name: Option<String>,
        n: Option<u32>,
    }

    #[test]
    fn load_layered_prefers_project_over_global_over_default() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.toml");
        let global = dir.path().join("global.toml");

        // Neither exists → built-in default.
        assert_eq!(
            load_layered::<FileCfg>(&project, &global),
            FileCfg::default()
        );

        // Only global exists → global wins.
        std::fs::write(&global, "name = \"global\"\nn = 1\n").unwrap();
        assert_eq!(
            load_layered::<FileCfg>(&project, &global),
            FileCfg {
                name: Some("global".into()),
                n: Some(1)
            }
        );

        // Project exists → project wins, global is ignored (no merge).
        std::fs::write(&project, "name = \"project\"\n").unwrap();
        assert_eq!(
            load_layered::<FileCfg>(&project, &global),
            FileCfg {
                name: Some("project".into()),
                n: None
            }
        );
    }

    #[test]
    fn load_layered_falls_back_to_default_on_bad_toml() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.toml");
        std::fs::write(&project, "this is = = not toml").unwrap();
        assert_eq!(
            load_layered::<FileCfg>(&project, &project),
            FileCfg::default()
        );
    }

    #[test]
    fn char_budget_admits_first_then_caps() {
        let mut b = CharBudget::new(100);
        // First item always fits, even if it already exceeds max.
        assert!(!b.would_overflow(120));
        b.add(120);
        assert_eq!(b.count(), 1);
        assert_eq!(b.used(), 120);
        // Now anything non-trivial overflows.
        assert!(b.would_overflow(1));
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn char_budget_fits_until_total_exceeds_max() {
        let mut b = CharBudget::new(100);
        assert!(!b.would_overflow(60));
        b.add(60); // used = 60
        assert!(!b.would_overflow(40)); // 60 + 40 == 100, fits
        b.add(40); // used = 100
        assert!(b.would_overflow(1)); // 100 + 1 > 100
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn truncate_chars_keeps_short_strings_and_cuts_long_ones() {
        assert_eq!(truncate_chars("short", 10, "…"), "short");
        assert_eq!(truncate_chars("abcdef", 3, "…"), "abc…");
        // char-boundary safe on multibyte input
        assert_eq!(truncate_chars("あいうえお", 2, "…"), "あい…");
    }
}
