//! Data model shared across subcommands: the task decomposition the LLM produces,
//! the schedule the engine computes, and the generic hook-input envelope.

use serde::{Deserialize, Serialize};

/// How a task may be executed. The LLM classifies; the engine enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Independent, parallel-eligible (subject to file-conflict analysis).
    #[default]
    Parallel,
    /// Must run alone on the main line (shared files / design decisions).
    Serial,
    /// Requires an approval gate (deploy, shared infra). Never auto-run.
    Gated,
    /// A reversible spike/probe whose value is learning, not a deliverable.
    /// Scheduled on its own track and never placed on the auto-merge path.
    Experiment,
}

/// One unit of work in a decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Files (or globs) the task is expected to touch. Drives conflict analysis.
    #[serde(default)]
    pub touched_files: Vec<String>,
    /// Ids of tasks that must complete before this one.
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub class: Class,
    #[serde(default)]
    pub suggested_model: Option<String>,
    #[serde(default)]
    pub done_criteria: Option<String>,
    /// Optional size hint (xs|s|m|l|xl) for downstream tools. Free-form and
    /// permissive: unknown or missing values are accepted and ignored here.
    #[serde(default)]
    pub size: Option<String>,
    /// Symbols (functions/classes) the task is expected to edit — finer than
    /// `touched_files`. The engine does not act on these; carried through so the
    /// skill can forward them to a worker without losing them across `state init`.
    #[serde(default)]
    pub target_symbols: Vec<String>,
    /// A command that reproduces/validates the task's outcome (the TDD anchor).
    /// Like `size`, the engine treats this as a permissive passthrough.
    #[serde(default)]
    pub reproduction_tests: Option<String>,
    /// Self-assessed confidence the task is well-scoped and completable (high|medium|low).
    /// The engine carries this through; SKILL.md uses it to gate clarification and
    /// re-verification. Unknown values are accepted and ignored.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// The full plan the interpreter agent emits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decomposition {
    #[serde(default)]
    pub goal: String,
    pub tasks: Vec<Task>,
}

/// A set of task ids with no pairwise file conflict — safe to run concurrently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub parallel: Vec<String>,
}

/// The deterministic schedule: ordered parallel batches plus the serial/gated lists.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Schedule {
    /// Parallel-eligible work, in dependency order. Each batch runs after the
    /// previous one's tasks are done; tasks within a batch run concurrently.
    pub batches: Vec<Batch>,
    /// Tasks forced onto the main line (class=serial or touching a shared glob),
    /// in dependency order.
    pub serial: Vec<String>,
    /// Tasks that require an approval gate; never scheduled for auto-run.
    pub gated: Vec<String>,
    /// Experiment/spike tasks: reversible probes scheduled on their own track
    /// and never placed on the auto-merge path (batches/serial).
    pub experiment: Vec<String>,
    /// Non-fatal notes (e.g. "task X touches a shared path -> serial").
    pub warnings: Vec<String>,
}

/// Generic hook-input envelope. Every Claude Code hook event posts a JSON object
/// on stdin; all fields are optional so one struct absorbs every event. Most
/// fields are unused today but kept to document the envelope and for future hooks.
#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

impl HookInput {
    /// Parse hook stdin; any malformed input yields defaults (never panics).
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_without_size_still_parses() {
        // Back-compat: decompositions emitted before `size` existed must load.
        let dec: Decomposition = serde_json::from_str(
            r#"{"goal":"g","tasks":[{"id":"a","touched_files":["src/a.rs"]}]}"#,
        )
        .expect("decomposition without size should parse");
        assert_eq!(dec.tasks.len(), 1);
        assert_eq!(dec.tasks[0].size, None);
    }

    #[test]
    fn task_with_size_is_populated() {
        let dec: Decomposition = serde_json::from_str(
            r#"{"goal":"g","tasks":[{"id":"a","size":"m"}]}"#,
        )
        .expect("decomposition with size should parse");
        assert_eq!(dec.tasks[0].size.as_deref(), Some("m"));
    }

    #[test]
    fn task_without_agentic_fields_defaults_empty() {
        // Back-compat: decompositions emitted before the agentic fields existed.
        let dec: Decomposition =
            serde_json::from_str(r#"{"goal":"g","tasks":[{"id":"a"}]}"#).unwrap();
        assert!(dec.tasks[0].target_symbols.is_empty());
        assert_eq!(dec.tasks[0].reproduction_tests, None);
    }

    #[test]
    fn task_carries_target_symbols_and_reproduction_tests() {
        let dec: Decomposition = serde_json::from_str(
            r#"{"goal":"g","tasks":[{"id":"a","target_symbols":["foo","Bar"],"reproduction_tests":"cargo test -p x"}]}"#,
        )
        .expect("decomposition with agentic fields should parse");
        assert_eq!(dec.tasks[0].target_symbols, vec!["foo", "Bar"]);
        assert_eq!(
            dec.tasks[0].reproduction_tests.as_deref(),
            Some("cargo test -p x")
        );
    }

    #[test]
    fn task_without_confidence_defaults_none() {
        // Back-compat: decompositions emitted before `confidence` existed must load.
        let dec: Decomposition =
            serde_json::from_str(r#"{"goal":"g","tasks":[{"id":"a"}]}"#).unwrap();
        assert_eq!(dec.tasks[0].confidence, None);
    }

    #[test]
    fn task_with_confidence_is_populated() {
        let dec: Decomposition = serde_json::from_str(
            r#"{"goal":"g","tasks":[{"id":"a","confidence":"low"}]}"#,
        )
        .expect("decomposition with confidence should parse");
        assert_eq!(dec.tasks[0].confidence.as_deref(), Some("low"));
    }

    #[test]
    fn task_with_experiment_class_parses() {
        let dec: Decomposition = serde_json::from_str(
            r#"{"goal":"g","tasks":[{"id":"x","class":"experiment"}]}"#,
        )
        .expect("decomposition with experiment class should parse");
        assert_eq!(dec.tasks[0].class, Class::Experiment);
    }
}
