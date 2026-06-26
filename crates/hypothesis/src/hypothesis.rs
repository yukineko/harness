use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub text: String,
    pub status: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub linked_goal: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
