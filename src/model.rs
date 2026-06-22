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
