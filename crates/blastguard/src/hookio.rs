//! PreToolUse output helper: build the single-line JSON that tells Claude Code
//! to deny a tool call.

/// Serialize a PreToolUse `deny` decision to the one-line JSON Claude Code reads
/// from a hook's stdout.
pub fn deny_json(reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_json_has_required_shape() {
        let s = deny_json("boom");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(v["hookSpecificOutput"]["permissionDecisionReason"], "boom");
        // Must be a single line so it is a valid one-shot hook payload.
        assert!(!s.contains('\n'));
    }
}
