//! Render the normalize prompt from the template + requirement + canon pointers.
//!
//! Like specguard, the template is data: the embedded default is used unless
//! `[prompt].normalize_template` points elsewhere. The canon CONTENT is never
//! copied in — only pointers — so the agent always reads the live canon and the
//! prompt cannot drift against it (DESIGN.md §1).

use crate::parse::MARKER;

pub const DEFAULT_TEMPLATE: &str = include_str!("../../templates/normalize-prompt.md");

/// Placeholders the normalize template must contain (machine contract).
pub const NORMALIZE_PLACEHOLDERS: &[&str] = &[
    "{{PROJECT_NAME}}",
    "{{DATE}}",
    "{{MARKER}}",
    "{{SPEC_ID}}",
    "{{REQUIREMENT}}",
    "{{CANON}}",
];

/// Required placeholders missing from `template` (non-empty = contract broken).
pub fn missing_placeholders(template: &str) -> Vec<&'static str> {
    NORMALIZE_PLACEHOLDERS
        .iter()
        .filter(|p| !template.contains(**p))
        .copied()
        .collect()
}

/// Render the canon-pointer block (pointers only, never content).
fn canon_block(canon: &[String]) -> String {
    if canon.is_empty() {
        return "- (canon ポインタの指定なし — プロジェクト横断の正典を参照。接地できない\n  acceptance は G1 違反として不足に上げること)\n".to_string();
    }
    canon.iter().map(|c| format!("- `{c}`\n")).collect()
}

/// Render the normalize prompt for one requirement bundle.
pub fn render(
    template: &str,
    project_name: &str,
    spec_id: &str,
    requirement: &str,
    canon: &[String],
    date: &str,
) -> String {
    template
        .replace("{{PROJECT_NAME}}", project_name)
        .replace("{{DATE}}", date)
        .replace("{{MARKER}}", MARKER)
        .replace("{{SPEC_ID}}", spec_id)
        .replace("{{CANON}}", &canon_block(canon))
        .replace("{{REQUIREMENT}}", requirement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_satisfies_contract() {
        assert!(missing_placeholders(DEFAULT_TEMPLATE).is_empty());
    }

    #[test]
    fn render_fills_all_placeholders() {
        let out = render(
            DEFAULT_TEMPLATE,
            "Demo",
            "2026-01-01-login",
            "ログインを制限する",
            &["docs/auth.md#rate-limit".to_string()],
            "2026-01-01",
        );
        assert!(out.contains("Demo"));
        assert!(out.contains("2026-01-01-login"));
        assert!(out.contains("ログインを制限する"));
        assert!(out.contains("docs/auth.md#rate-limit"));
        assert!(out.contains(MARKER));
        assert!(!out.contains("{{"));
    }

    #[test]
    fn missing_placeholder_detected() {
        let m = missing_placeholders("only {{PROJECT_NAME}} here");
        assert!(m.contains(&"{{MARKER}}"));
        assert!(m.contains(&"{{REQUIREMENT}}"));
    }
}
