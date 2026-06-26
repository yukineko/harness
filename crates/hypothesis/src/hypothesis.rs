use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// FNV-1a 64-bit hash, returning an 8-digit lowercase hex string.
pub fn new_id(text: &str) -> String {
    let mut fnv: u64 = 14695981039346656037;
    for byte in text.as_bytes() {
        fnv ^= *byte as u64;
        fnv = fnv.wrapping_mul(1099511628211);
    }
    // Take lower 32 bits → 8 hex digits
    format!("{:08x}", fnv as u32)
}

/// Returns current UTC time as an ISO 8601 string (e.g. "2026-06-26T13:00:00Z").
fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Manual conversion from Unix timestamp to date/time components.
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400; // days since 1970-01-01

    // Compute year, month, day from days count.
    let mut year = 1970u32;
    let mut remaining_days = days;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let leap = is_leap_year(year);
    let days_in_month = [31u64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u32;
    for dim in &days_in_month {
        if remaining_days < *dim {
            break;
        }
        remaining_days -= *dim;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, h, m, s)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Validated,
    Rejected,
}

impl Status {
    pub fn is_open(&self) -> bool {
        matches!(self, Status::Open)
    }

    #[allow(dead_code)]
    pub fn is_validated(&self) -> bool {
        matches!(self, Status::Validated)
    }

    #[allow(dead_code)]
    pub fn is_rejected(&self) -> bool {
        matches!(self, Status::Rejected)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Open => write!(f, "open"),
            Status::Validated => write!(f, "validated"),
            Status::Rejected => write!(f, "rejected"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub text: String,
    pub status: Status,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub linked_goal: Option<String>,
    #[serde(default)]
    pub condukt_run: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Hypothesis {
    pub fn new(text: impl Into<String>, linked_goal: Option<String>) -> Self {
        let text = text.into();
        let id = new_id(&text);
        let now = now_iso8601();
        Self {
            id,
            text,
            status: Status::Open,
            evidence: vec![],
            linked_goal,
            condukt_run: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_deterministic() {
        let id1 = new_id("test hypothesis text");
        let id2 = new_id("test hypothesis text");
        assert_eq!(id1, id2);
    }

    #[test]
    fn new_id_differs_for_different_input() {
        let id1 = new_id("hypothesis A");
        let id2 = new_id("hypothesis B");
        assert_ne!(id1, id2);
    }

    #[test]
    fn new_id_is_8_hex_chars() {
        let id = new_id("some text");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn status_predicates() {
        assert!(Status::Open.is_open());
        assert!(!Status::Open.is_validated());
        assert!(!Status::Open.is_rejected());

        assert!(!Status::Validated.is_open());
        assert!(Status::Validated.is_validated());
        assert!(!Status::Validated.is_rejected());

        assert!(!Status::Rejected.is_open());
        assert!(!Status::Rejected.is_validated());
        assert!(Status::Rejected.is_rejected());
    }

    #[test]
    fn status_display() {
        assert_eq!(Status::Open.to_string(), "open");
        assert_eq!(Status::Validated.to_string(), "validated");
        assert_eq!(Status::Rejected.to_string(), "rejected");
    }

    #[test]
    fn hypothesis_new_sets_open_status() {
        let h = Hypothesis::new("our first hypothesis", None);
        assert!(h.status.is_open());
        assert_eq!(h.text, "our first hypothesis");
        assert!(h.evidence.is_empty());
        assert!(h.linked_goal.is_none());
        assert_eq!(h.id.len(), 8);
    }

    #[test]
    fn hypothesis_serialize_deserialize_roundtrip() {
        let h = Hypothesis::new("roundtrip test", Some("goal-abc".to_string()));
        let json = serde_json::to_string(&h).expect("serialize");
        let h2: Hypothesis = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(h.id, h2.id);
        assert_eq!(h.text, h2.text);
        assert_eq!(h.status, h2.status);
        assert_eq!(h.linked_goal, h2.linked_goal);
    }

    #[test]
    fn hypothesis_deserialize_legacy_without_evidence_and_goal() {
        // Old records without evidence/linked_goal fields should deserialize fine
        let json = r#"{
            "id": "abcd1234",
            "text": "legacy hypothesis",
            "status": "open",
            "created_at": "2026-06-26T13:00:00Z",
            "updated_at": "2026-06-26T13:00:00Z"
        }"#;
        let h: Hypothesis = serde_json::from_str(json).expect("deserialize legacy");
        assert!(h.evidence.is_empty());
        assert!(h.linked_goal.is_none());
        assert!(h.status.is_open());
    }

    #[test]
    fn status_serde_snake_case() {
        let s = serde_json::to_string(&Status::Validated).expect("serialize");
        assert_eq!(s, r#""validated""#);
        let s2: Status = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(s2, Status::Validated);
    }
}
