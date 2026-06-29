//! Default [`ContextInjector`] — heading-addressable retrieval injection
//! (UserPromptSubmit). On each user turn the injector scores spec sections by
//! heading–prompt term overlap and either injects the matched section body or,
//! when no heading scores, a cheap table-of-contents sentinel. Pinned/normative
//! content is never dropped — the injector only *adds* `additionalContext`
//! beside the prompt, never replacing it (I1-adjacent).
//!
//! The implementation is tokenizer-free and deterministic: ASCII word overlap
//! (split on non-alphanumeric, lowercase, len ≥ 2). No model call, no API key,
//! no embeddings. The private `inject_for` core keeps env/process state out of
//! the tested logic; the public trait method resolves the reference doc from
//! `CONTEXT_GOVERNOR_REFERENCE_DOC` and delegates.

use std::collections::HashSet;

use super::classifier::{split_sections, Section};
use crate::handlers::ContextInjector;
use crate::io::HookOutput;
use harness_core::hook::HookInput;

// ── tokenizer ─────────────────────────────────────────────────────────────────

/// Tokenize `text` into a set of lowercase ASCII word tokens with length ≥ 2.
/// Non-alphanumeric characters are word separators. The minimum length of 2
/// prevents ultra-common single-letter tokens from polluting the score.
fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Heading-overlap score: count of prompt tokens that appear in `heading`'s
/// token set. Pure term-overlap — no TF-IDF, no embeddings.
fn score_heading(prompt_tokens: &HashSet<String>, heading: &str) -> usize {
    let heading_tokens = tokenize(heading);
    prompt_tokens
        .iter()
        .filter(|t| heading_tokens.contains(*t))
        .count()
}

// ── ToC builder ───────────────────────────────────────────────────────────────

/// Build a compact table-of-contents string from `sections`. Sections with
/// an empty heading (preamble) are omitted — they have no addressable title.
/// Returns an empty string when no named headings exist.
fn build_toc(sections: &[Section]) -> String {
    let headings: Vec<String> = sections
        .iter()
        .filter(|s| !s.heading.is_empty())
        .map(|s| format!("- {}", s.heading))
        .collect();
    if headings.is_empty() {
        return String::new();
    }
    format!("Reference sections:\n{}", headings.join("\n"))
}

// ── DefaultInjector ───────────────────────────────────────────────────────────

pub struct DefaultInjector;

impl DefaultInjector {
    /// Testable core: given a raw `prompt` string and the full reference `doc`,
    /// select the most relevant section and emit the appropriate `HookOutput`.
    ///
    /// Selection strategy (deterministic, no model call):
    /// 1. Split `doc` into sections via the shared ATX-heading parser.
    /// 2. Score each section by heading–prompt token overlap.
    /// 3. If the best score > 0, inject that section's heading + body as
    ///    `additionalContext` (heading-addressable retrieval).
    /// 4. If no heading scores > 0, inject the table-of-contents sentinel.
    /// 5. If `doc` is empty / produces no sections, return `{}` (no-op).
    ///
    /// The injector only *adds* context; it never replaces the prompt (I1-adjacent).
    pub(crate) fn inject_for(&self, prompt: &str, doc: &str) -> HookOutput {
        let sections = split_sections(doc);
        if sections.is_empty() {
            return HookOutput::default();
        }

        let prompt_tokens = tokenize(prompt);

        // Score every section by heading-token overlap with the prompt.
        // `max_by_key` is stable on ties (returns the last maximum), giving
        // document-order preference to later sections on ties — deterministic.
        let best = sections
            .iter()
            .map(|s| (score_heading(&prompt_tokens, &s.heading), s))
            .max_by_key(|(score, _)| *score);

        match best {
            Some((score, section)) if score > 0 => {
                // Heading-addressable retrieval: the relevant turn gets the matched section.
                let text = if section.heading.is_empty() {
                    section.body.clone()
                } else {
                    format!("{}\n{}", section.heading, section.body)
                };
                HookOutput::inject("UserPromptSubmit", text)
            }
            _ => {
                // Irrelevant turn: cheap ToC sentinel so the model knows what's
                // retrievable without paying the full reference-body cost.
                let toc = build_toc(&sections);
                if toc.is_empty() {
                    HookOutput::default()
                } else {
                    HookOutput::inject("UserPromptSubmit", toc)
                }
            }
        }
    }
}

impl ContextInjector for DefaultInjector {
    /// Resolve the reference doc from `CONTEXT_GOVERNOR_REFERENCE_DOC` (a file
    /// path). If unset or unreadable, returns `{}` — never break a turn (hook
    /// invariant). Otherwise delegates to `inject_for` which contains all
    /// testable logic.
    fn inject(&self, input: &HookInput) -> HookOutput {
        let path = match std::env::var("CONTEXT_GOVERNOR_REFERENCE_DOC") {
            Ok(p) if !p.is_empty() => p,
            _ => return HookOutput::default(),
        };
        let doc = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return HookOutput::default(),
        };
        self.inject_for(&input.prompt, &doc)
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Spec doc with one normative section (API Contracts), two reference
    /// sections (Endpoints, Glossary) and one example section.
    const DOC: &str = "\
# API Contracts
All endpoints must return JSON.
Status codes MUST follow RFC 7231.

# Endpoints
GET /users — list all users
POST /users — create a user

# Examples
```json
{ \"id\": 1, \"name\": \"Alice\" }
```

# Glossary
Token: an opaque string identifying a session.
";

    fn make_input(prompt: &str) -> HookInput {
        serde_json::from_value(serde_json::json!({
            "session_id": "test-session",
            "transcript_path": "",
            "cwd": "",
            "hook_event_name": "UserPromptSubmit",
            "prompt": prompt,
        }))
        .unwrap()
    }

    // ── heading-match path ────────────────────────────────────────────────────

    #[test]
    fn inject_for_matching_heading_returns_section_body() {
        // "endpoints" overlaps with the "Endpoints" heading token set.
        let out = DefaultInjector.inject_for("list all endpoints", DOC);
        let json = out.to_json();

        assert!(
            json.contains("\"additionalContext\""),
            "expected additionalContext envelope; got: {json}"
        );
        assert!(
            json.contains("\"hookEventName\":\"UserPromptSubmit\""),
            "hookEventName must be UserPromptSubmit; got: {json}"
        );
        // The Endpoints section body contains the route lines.
        assert!(
            json.contains("GET /users") || json.contains("POST /users"),
            "matched section body not found in: {json}"
        );
    }

    // ── ToC path ─────────────────────────────────────────────────────────────

    #[test]
    fn inject_for_irrelevant_prompt_returns_toc_not_section_body() {
        // "weather" / "today" do not appear in any heading — ToC path fires.
        let out = DefaultInjector.inject_for("what is the weather like today?", DOC);
        let json = out.to_json();

        assert!(
            json.contains("\"additionalContext\""),
            "expected additionalContext in ToC path; got: {json}"
        );
        assert!(
            json.contains("Reference sections:"),
            "ToC prefix not found in: {json}"
        );
        // Heading names appear in ToC.
        assert!(
            json.contains("API Contracts"),
            "ToC should list headings; got: {json}"
        );
        // Deep section body (route lines) must NOT appear in the ToC path.
        assert!(
            !json.contains("GET /users — list all users"),
            "deep section body must be withheld on the ToC path; got: {json}"
        );
    }

    // ── env-unset path ────────────────────────────────────────────────────────

    #[test]
    fn inject_without_env_var_returns_empty_object() {
        // Guard: ensure the env var is not set for this test.
        std::env::remove_var("CONTEXT_GOVERNOR_REFERENCE_DOC");
        let input = make_input("show me the endpoints");
        let out = DefaultInjector.inject(&input);
        assert_eq!(out.to_json(), "{}", "unset env var must produce no-op {{}}");
    }

    // ── empty doc ─────────────────────────────────────────────────────────────

    #[test]
    fn inject_for_empty_doc_returns_empty_object() {
        let out = DefaultInjector.inject_for("anything", "");
        assert_eq!(out.to_json(), "{}", "empty doc must produce no-op {{}}");
    }

    // ── proptest invariant ────────────────────────────────────────────────────

    proptest! {
        /// inject_for never panics and never emits `updatedToolOutput` (which
        /// would replace the prompt — correctness violation). The output must be
        /// either `{}` (no-op) or an `additionalContext` envelope.
        #[test]
        fn inject_for_never_panics_or_replaces_prompt(prompt in ".*", doc in ".*") {
            let out = DefaultInjector.inject_for(&prompt, &doc);
            let json = out.to_json();
            prop_assert!(
                !json.contains("updatedToolOutput"),
                "inject_for must never emit updatedToolOutput (would replace prompt): {json}"
            );
        }
    }
}
