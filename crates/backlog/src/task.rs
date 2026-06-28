use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub project: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub status: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Unix timestamp (seconds) before which this task is deferred.
    /// Absent in older tasks.toml files; treated as None (not deferred).
    #[serde(default)]
    pub defer_until: Option<i64>,
    /// Ordering weight (higher = surfaced sooner within the same priority tier).
    /// Carries a compass opportunity's weight so the source layer's queue order
    /// is driven by opportunity impact, not just priority + insertion time.
    /// Absent in older tasks.toml files; `#[serde(default)]` makes those load as
    /// 0.0, which preserves the legacy `(priority, created_at)` order exactly
    /// (all-equal weight → tie-break falls through to created_at).
    #[serde(default)]
    pub weight: f64,
}

impl Task {
    /// Returns priority derived from tags: "p0"→0, "p1"→1, "p2"→2, none→3.
    pub fn priority(&self) -> u8 {
        for tag in &self.tags {
            match tag.as_str() {
                "p0" => return 0,
                "p1" => return 1,
                "p2" => return 2,
                _ => {}
            }
        }
        3
    }

    /// Returns the first tag starting with "cycle:", if any.
    pub fn cycle_tag(&self) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.starts_with("cycle:"))
            .map(|t| t.as_str())
    }

    /// Returns true if status is "pending" or "failed".
    /// Note: does NOT consider defer_until. Callers combine with is_deferred()
    /// to decide whether to surface a task.
    pub fn is_pending(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "failed")
    }

    /// Returns true when the task is deferred past the given unix timestamp.
    /// A task with defer_until = None is never considered deferred.
    pub fn is_deferred(&self, now: i64) -> bool {
        matches!(self.defer_until, Some(t) if t > now)
    }
}

/// Generate an 8-char hex ID from title and unix timestamp using FNV-1a 32-bit.
pub fn new_id(title: &str, now: i64) -> String {
    let input = format!("{}\x00{}", title, now);
    let hash = fnv1a32(&input);
    format!("{:08x}", hash)
}

fn fnv1a32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(tags: Vec<&str>, status: &str) -> Task {
        Task {
            id: "00000000".to_string(),
            title: "test".to_string(),
            project: "/tmp/proj".to_string(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            status: status.to_string(),
            notes: String::new(),
            created_at: 0,
            updated_at: 0,
            defer_until: None,
            weight: 0.0,
        }
    }

    #[test]
    fn priority_p0() {
        assert_eq!(
            make_task(vec!["p0", "cycle:test-fix"], "pending").priority(),
            0
        );
    }

    #[test]
    fn priority_p1() {
        assert_eq!(make_task(vec!["p1"], "pending").priority(), 1);
    }

    #[test]
    fn priority_p2() {
        assert_eq!(make_task(vec!["p2"], "pending").priority(), 2);
    }

    #[test]
    fn priority_none() {
        assert_eq!(make_task(vec![], "pending").priority(), 3);
    }

    #[test]
    fn cycle_tag_found() {
        let t = make_task(vec!["p1", "cycle:test-fix"], "pending");
        assert_eq!(t.cycle_tag(), Some("cycle:test-fix"));
    }

    #[test]
    fn cycle_tag_none() {
        let t = make_task(vec!["p1"], "pending");
        assert_eq!(t.cycle_tag(), None);
    }

    #[test]
    fn is_pending_true_for_pending_and_failed() {
        assert!(make_task(vec![], "pending").is_pending());
        assert!(make_task(vec![], "failed").is_pending());
    }

    #[test]
    fn is_pending_false_for_others() {
        assert!(!make_task(vec![], "running").is_pending());
        assert!(!make_task(vec![], "done").is_pending());
    }

    #[test]
    fn new_id_returns_8_hex_chars() {
        let id = new_id("hello", 1234567890);
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn new_id_is_deterministic() {
        assert_eq!(new_id("task", 100), new_id("task", 100));
    }

    #[test]
    fn new_id_differs_for_different_inputs() {
        assert_ne!(new_id("task-a", 100), new_id("task-b", 100));
        assert_ne!(new_id("task", 100), new_id("task", 101));
    }

    // --- is_deferred tests ---

    #[test]
    fn is_deferred_none_is_never_deferred() {
        let t = make_task(vec![], "pending");
        assert!(!t.is_deferred(0));
        assert!(!t.is_deferred(9_999_999_999));
    }

    #[test]
    fn is_deferred_future_timestamp_returns_true() {
        let mut t = make_task(vec![], "pending");
        t.defer_until = Some(2_000);
        // now = 1_000 < 2_000  →  deferred
        assert!(t.is_deferred(1_000));
    }

    #[test]
    fn is_deferred_past_timestamp_returns_false() {
        let mut t = make_task(vec![], "pending");
        t.defer_until = Some(500);
        // now = 1_000 >= 500  →  not deferred
        assert!(!t.is_deferred(1_000));
    }

    #[test]
    fn is_deferred_equal_timestamp_returns_false() {
        let mut t = make_task(vec![], "pending");
        t.defer_until = Some(1_000);
        // defer_until == now  →  not deferred (> is strict)
        assert!(!t.is_deferred(1_000));
    }

    #[test]
    fn is_pending_unaffected_by_defer_until() {
        // is_pending must ignore defer_until; callers decide with is_deferred()
        let mut t = make_task(vec![], "pending");
        t.defer_until = Some(9_999_999_999);
        assert!(t.is_pending());
    }

    #[test]
    fn serde_roundtrip_without_defer_until() {
        // Older tasks.toml records that lack defer_until must deserialize fine.
        let json = r#"{
            "id": "abcd1234",
            "title": "old task",
            "project": "/tmp/p",
            "tags": [],
            "status": "pending",
            "notes": "",
            "created_at": 0,
            "updated_at": 0
        }"#;
        let t: Task = serde_json::from_str(json).expect("deserialize without defer_until");
        assert!(t.defer_until.is_none());
        // weight is also absent in this legacy record → defaults to 0.0,
        // which keeps legacy tasks ordering identically to before.
        assert_eq!(t.weight, 0.0);
    }
}
