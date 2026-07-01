//! Default [`SpecClassifier`] — load-time spec split + resident-budget check
//! (§8, I3). Splits a spec into NormativeCore (resident) and ReferenceBody
//! (retrieval), then verifies the resident set fits the standing budget.
//!
//! The implementation is deliberately tokenizer-free and deterministic: it
//! splits on ATX markdown headings and classifies each section by keyword match
//! on the heading text (~4 chars/token estimate, no model call, no API key).
//! `split_sections` is `pub(crate)` so `DefaultInjector` can reuse the same
//! heading parser without duplicating it.

use crate::handlers::SpecClassifier;
use crate::types::{ContextItem, ItemBody, ItemId, Lane, Overrun, SpecClass, StandingBudget};

// ── token estimate ────────────────────────────────────────────────────────────

/// Rough, deterministic token estimate (~4 chars/token, tokenizer-free). Any
/// non-empty string is at least 1 token so a tiny section never reads as free.
fn est_tokens(s: &str) -> u32 {
    let chars = s.chars().count();
    if chars == 0 {
        0
    } else {
        u32::try_from(chars.div_ceil(4).max(1)).unwrap_or(u32::MAX)
    }
}

// ── heading parser ────────────────────────────────────────────────────────────

/// A parsed markdown section: the ATX heading text, its depth (`level`), and
/// all body lines up to the next heading (or end of document).
pub(crate) struct Section {
    pub(crate) heading: String,
    pub(crate) level: u8,
    pub(crate) body: String,
}

impl Section {
    /// Full text of the section for token estimation: the reconstructed heading
    /// line followed by the body. The preamble section (empty heading, level 0)
    /// omits the heading line.
    pub(crate) fn full_text(&self) -> String {
        if self.heading.is_empty() {
            self.body.clone()
        } else {
            format!(
                "{} {}\n{}",
                "#".repeat(self.level as usize),
                self.heading,
                self.body
            )
        }
    }
}

/// Parse a single ATX-style heading line per CommonMark: one to six `#`
/// characters at the line start followed by a space and optional title.
/// Returns `(level, heading_text)` or `None` when the line is not a heading.
fn parse_heading_line(line: &str) -> Option<(u8, &str)> {
    if !line.starts_with('#') {
        return None;
    }
    let n = line.chars().take_while(|&c| c == '#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = &line[n..];
    if let Some(title) = rest.strip_prefix(' ') {
        // Trim trailing whitespace from the heading text (CommonMark allows it).
        Some((n as u8, title.trim_end()))
    } else if rest.is_empty() {
        // Bare "###" with no title is a valid (if unusual) ATX heading.
        Some((n as u8, ""))
    } else {
        // "##keyword" without a space — not an ATX heading per CommonMark.
        None
    }
}

/// Split `doc` into sections on ATX headings. Any content before the first
/// heading is returned as a section with an empty heading (`level = 0`),
/// representing the document preamble / front matter — the classifier treats
/// it as NormativeCore (introductory norms and contracts). The preamble section
/// is omitted when it is entirely whitespace, keeping empty docs clean.
pub(crate) fn split_sections(doc: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    // `None` = still accumulating the preamble; `Some` = inside a heading section.
    let mut current_heading: Option<(u8, String)> = None;
    let mut current_body_lines: Vec<&str> = Vec::new();

    for line in doc.lines() {
        if let Some((level, heading_text)) = parse_heading_line(line) {
            let body = current_body_lines.join("\n");
            match current_heading.take() {
                None => {
                    // Preamble: only emit when non-empty (suppress spurious empty sections).
                    if !body.trim().is_empty() {
                        sections.push(Section {
                            heading: String::new(),
                            level: 0,
                            body,
                        });
                    }
                }
                Some((lvl, hdg)) => {
                    sections.push(Section {
                        heading: hdg,
                        level: lvl,
                        body,
                    });
                }
            }
            current_heading = Some((level, heading_text.to_string()));
            current_body_lines = Vec::new();
        } else {
            current_body_lines.push(line);
        }
    }

    // Flush the final accumulation (last heading + its body, or a preamble-only doc).
    let body = current_body_lines.join("\n");
    match current_heading {
        None => {
            if !body.trim().is_empty() {
                sections.push(Section {
                    heading: String::new(),
                    level: 0,
                    body,
                });
            }
        }
        Some((lvl, hdg)) => {
            sections.push(Section {
                heading: hdg,
                level: lvl,
                body,
            });
        }
    }

    sections
}

// ── spec-class decision ───────────────────────────────────────────────────────

/// Heading keywords (checked case-insensitively as substrings) that mark a
/// section as `ReferenceBody` — exhaustive tables, endpoint lists, examples,
/// and appendices that are large but situational (§8).
const REFERENCE_KEYWORDS: &[&str] = &[
    "example",
    "examples",
    "appendix",
    "reference",
    "endpoints",
    "table",
    "schema",
    "glossary",
    // Japanese equivalents (exact-substring; they have no case).
    "付録",
    "例",
    "一覧",
    "エンドポイント",
];

/// `true` when the heading indicates a `ReferenceBody` section. Case-insensitive
/// on ASCII; Japanese keywords are matched as byte substrings (they are
/// unambiguous and do not need case folding).
fn is_reference_body(heading: &str) -> bool {
    let lower = heading.to_lowercase();
    REFERENCE_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

// ── DefaultClassifier ─────────────────────────────────────────────────────────

pub struct DefaultClassifier;

impl SpecClassifier for DefaultClassifier {
    /// Split `doc` into sections and assign each a `SpecClass` based on its
    /// heading. `NormativeCore` → `Lane::Pinned` (resident, counted by
    /// `check_resident`); `ReferenceBody` → `Lane::Evictable` (retrieval).
    /// Items are returned in document order.
    fn classify(&self, doc: &str) -> Vec<(SpecClass, ContextItem)> {
        split_sections(doc)
            .into_iter()
            .enumerate()
            .map(|(idx, section)| {
                let class = if is_reference_body(&section.heading) {
                    SpecClass::ReferenceBody
                } else {
                    SpecClass::NormativeCore
                };
                let lane = match class {
                    SpecClass::NormativeCore => Lane::Pinned,
                    SpecClass::ReferenceBody => Lane::Evictable,
                };
                let text = section.full_text();
                let tokens = est_tokens(&text);
                let item = ContextItem {
                    id: ItemId(idx as u64),
                    lane,
                    tokens,
                    body: ItemBody::Inline(text),
                };
                (class, item)
            })
            .collect()
    }

    /// I3: sum the `tokens` of every `Lane::Pinned` item (the resident normative
    /// set) and compare against `budget.max_resident_tokens`. Uses saturating
    /// arithmetic to avoid overflow on pathological inputs.
    fn check_resident(
        &self,
        items: &[ContextItem],
        budget: &StandingBudget,
    ) -> Result<(), Overrun> {
        let resident_tokens: u32 = items
            .iter()
            .filter(|item| item.lane == Lane::Pinned)
            .fold(0u32, |acc, item| acc.saturating_add(item.tokens));

        if resident_tokens > budget.max_resident_tokens {
            Err(Overrun {
                resident_tokens,
                max_resident_tokens: budget.max_resident_tokens,
            })
        } else {
            Ok(())
        }
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A minimal spec with one normative section and two reference sections.
    const DOC: &str = "\
# Acceptance Criteria
Every request MUST be authenticated.
Timeouts MUST be retried up to 3 times.

# Examples
```
GET /api/users HTTP/1.1
```

# Appendix
Supplementary material here.
";

    #[test]
    fn classify_splits_normative_and_reference_in_order() {
        let items = DefaultClassifier.classify(DOC);
        // 3 sections: Acceptance Criteria (NormativeCore), Examples (ReferenceBody),
        // Appendix (ReferenceBody) — in document order.
        assert_eq!(items.len(), 3, "expected 3 sections");

        let (class0, item0) = &items[0];
        assert_eq!(
            *class0,
            SpecClass::NormativeCore,
            "section 0 should be NormativeCore"
        );
        assert_eq!(item0.lane, Lane::Pinned, "NormativeCore must be Pinned");
        assert!(item0.tokens > 0, "non-empty section must have tokens > 0");

        let (class1, item1) = &items[1];
        assert_eq!(
            *class1,
            SpecClass::ReferenceBody,
            "section 1 (Examples) should be ReferenceBody"
        );
        assert_eq!(
            item1.lane,
            Lane::Evictable,
            "ReferenceBody must be Evictable"
        );

        let (class2, item2) = &items[2];
        assert_eq!(
            *class2,
            SpecClass::ReferenceBody,
            "section 2 (Appendix) should be ReferenceBody"
        );
        assert_eq!(
            item2.lane,
            Lane::Evictable,
            "ReferenceBody must be Evictable"
        );
    }

    #[test]
    fn check_resident_ok_when_pinned_tokens_fit_budget() {
        let items: Vec<ContextItem> = DefaultClassifier
            .classify(DOC)
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        let budget = StandingBudget {
            max_resident_tokens: 10_000,
        };
        assert!(
            DefaultClassifier.check_resident(&items, &budget).is_ok(),
            "should be Ok when budget is generous"
        );
    }

    #[test]
    fn check_resident_err_when_pinned_tokens_exceed_budget() {
        let items: Vec<ContextItem> = DefaultClassifier
            .classify(DOC)
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        let budget = StandingBudget {
            max_resident_tokens: 1, // absurdly small
        };
        let result = DefaultClassifier.check_resident(&items, &budget);
        let err = result.expect_err("should return Overrun when budget is exceeded");
        assert!(
            err.resident_tokens > budget.max_resident_tokens,
            "resident_tokens must exceed max_resident_tokens"
        );
        assert_eq!(
            err.excess(),
            err.resident_tokens.saturating_sub(err.max_resident_tokens),
            "excess() must equal the overage"
        );
        assert!(err.excess() > 0, "excess must be positive");
    }

    proptest! {
        /// I3 / lane-class parity, exhaustively: classify never panics and every
        /// returned item's lane exactly matches its SpecClass.
        /// NormativeCore → Pinned; ReferenceBody → Evictable.
        #[test]
        fn classify_lane_matches_spec_class(doc in ".*") {
            let items = DefaultClassifier.classify(&doc);
            for (class, item) in &items {
                match class {
                    SpecClass::NormativeCore => prop_assert_eq!(
                        item.lane,
                        Lane::Pinned,
                        "NormativeCore must be Pinned; got {:?}",
                        item.lane
                    ),
                    SpecClass::ReferenceBody => prop_assert_eq!(
                        item.lane,
                        Lane::Evictable,
                        "ReferenceBody must be Evictable; got {:?}",
                        item.lane
                    ),
                }
            }
        }
    }
}
