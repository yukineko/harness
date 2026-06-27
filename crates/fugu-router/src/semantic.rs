//! Lightweight "semantic-ish" normalisation without an embedding service:
//! suffix stemming + a domain concept lexicon. Bridges lexically-different but
//! conceptually-same tasks (login ↔ auth ↔ session) so k-NN retrieves them as
//! neighbours. Deterministic, dependency-free, interpretable — the honest
//! subscription-native stand-in for embeddings.

use std::collections::BTreeSet;

/// Concept tags and the surface words that trigger them. A token matching one
/// also contributes `~<tag>` to the feature set, so two tasks sharing a concept
/// overlap even with no shared words.
const CONCEPTS: &[(&str, &[&str])] = &[
    (
        "auth",
        &[
            "auth",
            "authenticate",
            "authentication",
            "login",
            "logout",
            "signin",
            "signup",
            "session",
            "credential",
            "credentials",
            "oauth",
            "token",
            "jwt",
            "password",
            "permission",
            "permissions",
            "rbac",
        ],
    ),
    (
        "data",
        &[
            "database",
            "db",
            "sql",
            "query",
            "migration",
            "migrate",
            "schema",
            "table",
            "orm",
            "postgres",
            "mysql",
            "sqlite",
            "dataset",
        ],
    ),
    (
        "api",
        &[
            "api",
            "endpoint",
            "endpoints",
            "route",
            "router",
            "handler",
            "controller",
            "rest",
            "graphql",
            "grpc",
            "request",
            "response",
        ],
    ),
    (
        "ui",
        &[
            "ui",
            "frontend",
            "component",
            "render",
            "css",
            "style",
            "styles",
            "layout",
            "button",
            "form",
            "page",
            "view",
            "modal",
        ],
    ),
    (
        "test",
        &[
            "test",
            "tests",
            "spec",
            "assert",
            "fixture",
            "mock",
            "coverage",
            "unit",
            "integration",
            "e2e",
        ],
    ),
    (
        "perf",
        &[
            "performance",
            "perf",
            "latency",
            "throughput",
            "optimize",
            "optimization",
            "cache",
            "caching",
            "bench",
            "benchmark",
            "profil",
        ],
    ),
    (
        "infra",
        &[
            "deploy",
            "deployment",
            "ci",
            "cd",
            "pipeline",
            "docker",
            "kubernetes",
            "k8s",
            "terraform",
            "release",
            "rollout",
        ],
    ),
    (
        "doc",
        &[
            "doc",
            "docs",
            "documentation",
            "readme",
            "comment",
            "comments",
            "changelog",
            "guide",
        ],
    ),
    (
        "parse",
        &[
            "parse",
            "parser",
            "lexer",
            "tokenize",
            "tokenizer",
            "grammar",
            "ast",
            "serialize",
            "deserialize",
            "encode",
            "decode",
        ],
    ),
];

/// Strip a few common English suffixes so morphological variants collapse
/// (refactoring→refactor, renders→render, libraries→library). Longest suffix wins.
pub fn stem(word: &str) -> String {
    const SUF: &[&str] = &[
        "izations", "isations", "ization", "isation", "ations", "ation", "izing", "ising", "ings",
        "ing", "tion", "ies", "ed", "es", "s",
    ];
    for s in SUF {
        if word.len() > s.len() + 2 && word.ends_with(s) {
            let base = &word[..word.len() - s.len()];
            if *s == "ies" {
                return format!("{base}y");
            }
            return base.to_string();
        }
    }
    word.to_string()
}

/// Insert a token's stemmed form plus any concept tags it triggers. Concepts are
/// matched on the raw surface word (the lexicon lists surface forms).
pub fn expand_into(set: &mut BTreeSet<String>, raw: &str) {
    set.insert(stem(raw));
    for (tag, words) in CONCEPTS {
        if words.contains(&raw) {
            set.insert(format!("~{tag}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_collapses_variants() {
        assert_eq!(stem("refactoring"), "refactor");
        assert_eq!(stem("renders"), "render");
        assert_eq!(stem("libraries"), "library");
        assert_eq!(stem("login"), "login"); // no suffix to strip
    }

    #[test]
    fn concept_tags_bridge_synonyms() {
        let mut a = BTreeSet::new();
        expand_into(&mut a, "login");
        let mut b = BTreeSet::new();
        expand_into(&mut b, "authentication");
        // different surface words, same concept tag
        assert!(a.contains("~auth"));
        assert!(b.contains("~auth"));
    }
}
