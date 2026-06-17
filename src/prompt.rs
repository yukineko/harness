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
use crate::scope::{AreaHit, Scope};

/// The embedded default template. Override via `[prompt].template`.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/audit-prompt.md");

/// Maximum number of sample changed files listed per area in the prompt.
const MAX_SAMPLE_FILES: usize = 12;

/// One audit shard: either a single in-scope area (by index into
/// `scope.in_scope`) or the invariant set. Each shard is rendered into its own
/// focused prompt and audited by a separate agent process (fresh context).
#[derive(Debug, Clone, Copy)]
pub enum Shard {
    Area(usize),
    Invariants,
}

/// Build the shard list for a run: one per in-scope area, plus one invariant
/// shard when any invariants are defined (invariants run every time).
pub fn shards(cfg: &Config, scope: &Scope) -> Vec<Shard> {
    let mut v: Vec<Shard> = (0..scope.in_scope.len()).map(Shard::Area).collect();
    if !cfg.invariants.is_empty() {
        v.push(Shard::Invariants);
    }
    v
}

/// Human label for a shard (area name, or "invariants").
pub fn shard_label(cfg: &Config, scope: &Scope, shard: Shard) -> String {
    match shard {
        Shard::Area(i) => cfg.areas[scope.in_scope[i].area_index].name.clone(),
        Shard::Invariants => "invariants".to_string(),
    }
}

/// Render a single shard's focused prompt. An area shard sees only that area's
/// canon + changed files (invariants deferred to their own shard); the invariant
/// shard sees only the invariants. This keeps each agent context small and
/// homogeneous, mitigating context rot on multi-area runs.
pub fn render_shard(
    template: &str,
    cfg: &Config,
    scope: &Scope,
    shard: Shard,
    date: &str,
) -> String {
    let (areas, invariants, summary) = match shard {
        Shard::Area(i) => {
            let hit = &scope.in_scope[i];
            (
                area_block_one(cfg, hit),
                "(この shard では不変条件を扱わない — 不変条件は別 shard で照合する)\n".to_string(),
                shard_scope_summary(cfg, scope, Some(hit)),
            )
        }
        Shard::Invariants => (
            "(この shard は不変条件のみを照合する。D1 領域監査は別 shard で実施する。)\n".to_string(),
            invariants_block(cfg),
            shard_scope_summary(cfg, scope, None),
        ),
    };
    template
        .replace("{{PROJECT_NAME}}", &cfg.project.name)
        .replace("{{DATE}}", date)
        .replace("{{MARKER}}", MARKER)
        .replace("{{SCOPE_SUMMARY}}", &summary)
        .replace("{{AREAS}}", &areas)
        .replace("{{INVARIANTS}}", &invariants)
}

/// Scope summary for a single shard: the overall baseline, but a target scoped
/// to just this shard so the agent is told exactly what it (and only it) owns.
fn shard_scope_summary(cfg: &Config, scope: &Scope, hit: Option<&AreaHit>) -> String {
    let mut s = String::new();
    s.push_str(&format!("- baseline ref: `{}`\n", scope.baseline));
    if scope.fell_back {
        s.push_str(
            "  - 注意: 設定された baseline が解決できず fallback を使用した (レポートに明記すること)\n",
        );
    }
    s.push_str(&format!(
        "- 変更ファイル数 (リポジトリ全体): {}\n",
        scope.changed_files.len()
    ));
    match hit {
        Some(hit) => s.push_str(&format!(
            "- この shard の監査対象: 領域「{}」({} 件の変更ファイル)\n",
            cfg.areas[hit.area_index].name,
            hit.matched_files.len()
        )),
        None => s.push_str(&format!(
            "- この shard の監査対象: 不変条件 {} 件 (変更の有無に関わらず毎回)\n",
            cfg.invariants.len()
        )),
    }
    s.push_str(
        "- 注記: 他の領域・不変条件は別プロセス (fresh context) で監査される。本 shard はこの対象だけに集中すること。\n",
    );
    s.push_str(
        "\nこの shard が監査した対象は、レポートのスコープ欄に必ず明記すること (網羅偽装の防止)。\n",
    );
    s
}

/// Render the D1 block for a single area (its canon pointers + changed files).
fn area_block_one(cfg: &Config, hit: &AreaHit) -> String {
    let area = &cfg.areas[hit.area_index];
    let mut out = String::new();
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

    fn sample_scope() -> Scope {
        Scope {
            baseline: "abc123".into(),
            fell_back: false,
            changed_files: vec!["logging/sig.py".into()],
            in_scope: vec![AreaHit {
                area_index: 0,
                matched_files: vec!["logging/sig.py".into()],
            }],
            skipped_areas: vec![],
        }
    }

    #[test]
    fn shards_are_one_per_area_plus_invariants() {
        let cfg = sample_cfg();
        let scope = sample_scope();
        let s = shards(&cfg, &scope);
        assert_eq!(s.len(), 2); // one area + one invariant shard
        assert_eq!(shard_label(&cfg, &scope, s[0]), "logging");
        assert_eq!(shard_label(&cfg, &scope, s[1]), "invariants");
    }

    #[test]
    fn area_shard_fills_placeholders_and_defers_invariants() {
        let cfg = sample_cfg();
        let scope = sample_scope();
        let out = render_shard(DEFAULT_TEMPLATE, &cfg, &scope, Shard::Area(0), "2026-06-17");
        assert!(out.contains("Demo"));
        assert!(out.contains("2026-06-17"));
        assert!(out.contains("abc123"));
        assert!(out.contains("logging/SPEC.md")); // the area's canon
        assert!(out.contains(MARKER));
        // The invariant body belongs to a different shard, not this one.
        assert!(!out.contains("all signing via signature.py"));
        assert!(out.contains("不変条件を扱わない"));
        // No unsubstituted placeholders remain.
        assert!(!out.contains("{{"));
    }

    #[test]
    fn invariant_shard_carries_invariants_not_areas() {
        let cfg = sample_cfg();
        let scope = sample_scope();
        let out = render_shard(DEFAULT_TEMPLATE, &cfg, &scope, Shard::Invariants, "2026-06-17");
        assert!(out.contains("all signing via signature.py"));
        assert!(out.contains(MARKER));
        // Area canon is audited by the area shard, not here.
        assert!(out.contains("不変条件のみ"));
        assert!(!out.contains("{{"));
    }
}
