//! Shared result types produced by checks and consumed by the reporter.

use crate::config::Severity;

/// A single audit finding.
pub struct Issue {
    /// Short ALL-CAPS-ish category label, used in the audit log.
    pub category: String,
    /// Full human-facing message (may be multi-line).
    pub message: String,
    /// Block (affects exit code) or Warn (advisory only).
    pub severity: Severity,
}

impl Issue {
    pub fn block(category: &str, message: String) -> Issue {
        Issue {
            category: category.to_string(),
            message,
            severity: Severity::Block,
        }
    }

    pub fn warn(category: &str, message: String) -> Issue {
        Issue {
            category: category.to_string(),
            message,
            severity: Severity::Warn,
        }
    }
}
