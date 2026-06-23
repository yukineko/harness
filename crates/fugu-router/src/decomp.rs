//! Minimal mirror of condukt's decomposition schema — enough to read the task
//! list, rewrite `suggested_model`, and round-trip the JSON back to condukt.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decomposition {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub touched_files: Vec<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub suggested_model: String,
    #[serde(default)]
    pub done_criteria: String,
}
