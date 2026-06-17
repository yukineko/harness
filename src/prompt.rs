//! Render the audit prompt from the template + resolved scope.
//!
//! The template is data, not code: by default the embedded
//! `templates/audit-prompt.md` is used, but a project can point
//! `[prompt].template` at its own file. Project specifics (areas, canon
//! pointers, invariants) are injected as placeholders so the prompt stays
//! generic while the canon itself is never copied in — the agent always reads
//! the live canon files, which keeps the prompt from drifting against them.

use crate::config::Config;
use crate::parse::MARKER;
use crate::scope::Scope;

/// The embedded default template. Override via `[prompt].template`.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/audit-prompt.md");

/// Maximum number of sample changed files listed per area in the prompt.
const MAX_SAMPLE_FILES: usize = 12;

/// Render the final prompt. `date` is the run date string for the header.
pub fn render(template: &str, cfg: &Config, scope: &Scope, date: &str) -> String {
    template
        .replace("{{PROJECT_NAME}}", &cfg.project.name)
        .replace("{{DATE}}", date)
        .replace("{{MARKER}}", MARKER)
        .replace("{{SCOPE_SUMMARY}}", &scope_summary(cfg, scope))
        .replace("{{AREAS}}", &areas_block(cfg, scope))
        .replace("{{INVARIANTS}}", &invariants_block(cfg))
}

fn scope_summary(cfg: &Config, scope: &Scope) -> String {
    let in_scope_names: Vec<&str> = scope
        .in_scope
        .iter()
        .map(|h| cfg.areas[h.area_index].name.as_str())
        .collect();

    let mut s = String::new();
    s.push_str(&format!("- baseline ref: `{}`\n", scope.baseline));
    if scope.fell_back {
        s.push_str(
            "  - 注意: 設定された baseline が解決できず fallback を使用した (レポートに明記すること)\n",
        );
    }
    s.push_str(&format!(
        "- 変更ファイル数: {}\n",
        scope.changed_files.len()
    ));
    s.push_str(&format!(
        "- in-scope 領域 (変更あり): {}\n",
        join_or_none(&in_scope_names)
    ));
    s.push_str(&format!(
        "- skip した領域 (変更なし): {}\n",
        join_or_none(&scope.skipped_areas.iter().map(|s| s.as_str()).collect::<Vec<_>>())
    ));
    s.push_str(
        "\n監査した領域と skip した領域は、最終レポートに必ず明記すること (網羅偽装の防止)。\n",
    );
    s
}

fn areas_block(cfg: &Config, scope: &Scope) -> String {
    if scope.in_scope.is_empty() {
        return "(変更領域なし。D1 領域監査は skip。下記の不変条件のみ照合する。)\n".to_string();
    }
    let mut out = String::new();
    for hit in &scope.in_scope {
        let area = &cfg.areas[hit.area_index];
        out.push_str(&format!("### 領域: {}\n", area.name));
        out.push_str("参照すべき正典 (ポインタ。中身でなく「どこを読むか」):\n");
        if area.canon.is_empty() {
            out.push_str("- (このエリアに canon 指定なし — プロジェクト横断の正典を参照)\n");
        } else {
            for c in &area.canon {
                out.push_str(&format!("- `{c}`\n"));
            }
        }
        out.push_str("変更ファイル (この領域):\n");
        for f in hit.matched_files.iter().take(MAX_SAMPLE_FILES) {
            out.push_str(&format!("- `{f}`\n"));
        }
        if hit.matched_files.len() > MAX_SAMPLE_FILES {
            out.push_str(&format!(
                "- … ほか {} 件\n",
                hit.matched_files.len() - MAX_SAMPLE_FILES
            ));
        }
        out.push('\n');
    }
    out
}

fn invariants_block(cfg: &Config) -> String {
    if cfg.invariants.is_empty() {
        return "(不変条件の定義なし)\n".to_string();
    }
    let mut out = String::new();
    for inv in &cfg.invariants {
        out.push_str(&format!("- **{}**", inv.name));
        if !inv.description.trim().is_empty() {
            out.push_str(&format!(": {}", inv.description));
        }
        out.push('\n');
        if !inv.canon.is_empty() {
            let pointers: Vec<String> = inv.canon.iter().map(|c| format!("`{c}`")).collect();
            out.push_str(&format!("  - 正典: {}\n", pointers.join(", ")));
        }
    }
    out
}

fn join_or_none(items: &[&str]) -> String {
    if items.is_empty() {
        "なし".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{AreaHit, Scope};

    fn sample_cfg() -> Config {
        toml::from_str(
            r#"
            [project]
            name = "Demo"

            [[area]]
            name = "logging"
            globs = ["logging/**"]
            canon = ["logging/SPEC.md", "docs/signing.md"]

            [[invariant]]
            name = "signing"
            description = "all signing via signature.py"
            canon = ["docs/signing.md"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn render_fills_placeholders() {
        let cfg = sample_cfg();
        let scope = Scope {
            baseline: "abc123".into(),
            fell_back: false,
            changed_files: vec!["logging/sig.py".into()],
            in_scope: vec![AreaHit {
                area_index: 0,
                matched_files: vec!["logging/sig.py".into()],
            }],
            skipped_areas: vec![],
        };
        let out = render(DEFAULT_TEMPLATE, &cfg, &scope, "2026-06-17");
        assert!(out.contains("Demo"));
        assert!(out.contains("2026-06-17"));
        assert!(out.contains("abc123"));
        assert!(out.contains("logging/SPEC.md"));
        assert!(out.contains("all signing via signature.py"));
        assert!(out.contains(MARKER));
        // No unsubstituted placeholders remain.
        assert!(!out.contains("{{"));
    }

    #[test]
    fn render_no_changes_notes_skip() {
        let cfg = sample_cfg();
        let scope = Scope {
            baseline: "abc".into(),
            fell_back: false,
            changed_files: vec![],
            in_scope: vec![],
            skipped_areas: vec!["logging".into()],
        };
        let out = render(DEFAULT_TEMPLATE, &cfg, &scope, "2026-06-17");
        assert!(out.contains("変更領域なし") || out.contains("D1 領域監査は skip"));
        assert!(!out.contains("{{"));
    }
}
