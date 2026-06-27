//! Build the advisory routing-memory summary injected at UserPromptSubmit, so
//! the main loop (and the interpreter it briefs) is aware of what models have
//! worked before. Advisory only — never decisive on its own.

use std::collections::BTreeMap;

use crate::store::Episode;

/// True if the prompt looks like coding / orchestration work worth routing.
pub fn looks_actionable(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    [
        "condukt",
        "implement",
        "refactor",
        "fix",
        "add ",
        "build",
        "migrate",
        "feature",
        "実装",
        "修正",
        "リファクタ",
        "追加",
        "機能",
    ]
    .iter()
    .any(|k| p.contains(k))
}

/// One-block per-model pass-rate summary from the store, capped to `limit` bytes.
pub fn summary(episodes: &[Episode], limit: usize) -> Option<String> {
    if episodes.is_empty() {
        return None;
    }
    let mut agg: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    for e in episodes {
        let s = agg.entry(e.model.clone()).or_insert((0, 0, 0.0));
        s.0 += 1;
        if e.pass {
            s.1 += 1;
        }
        s.2 += e.cost_usd;
    }
    let mut lines = vec![format!(
        "[fugu-router] {} routing episode(s) recorded — per-model outcomes:",
        episodes.len()
    )];
    for (m, (n, p, cost)) in &agg {
        lines.push(format!(
            "  {m}: {p}/{n} pass ({:.0}%), avg ${:.4}",
            if *n > 0 {
                *p as f64 / *n as f64 * 100.0
            } else {
                0.0
            },
            if *n > 0 { cost / *n as f64 } else { 0.0 },
        ));
    }
    lines.push(
        "Run `fugu-router route --file <decomp.json>` to set suggested_model per task.".into(),
    );
    let mut out = lines.join("\n");
    if limit > 0 && out.len() > limit {
        out.truncate(limit);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_injects_nothing() {
        assert!(summary(&[], 1500).is_none());
    }

    #[test]
    fn actionable_detection() {
        assert!(looks_actionable("/condukt implement the parser"));
        assert!(looks_actionable("認証を実装して"));
        assert!(!looks_actionable("what time is it"));
    }
}
