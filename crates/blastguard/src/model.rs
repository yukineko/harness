//! The verdict type produced by the destructive-operation detector.

/// Outcome of inspecting a single tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Let the tool call proceed (blastguard stays silent).
    Allow,
    /// Block the tool call. The string is a short human-facing reason that is
    /// surfaced to the agent as the PreToolUse `permissionDecisionReason`.
    Deny(String),
}

impl Decision {
    /// Convenience constructor: `Decision::deny("...")`.
    pub fn deny(reason: impl Into<String>) -> Decision {
        Decision::Deny(reason.into())
    }

    pub fn is_deny(&self) -> bool {
        matches!(self, Decision::Deny(_))
    }
}
