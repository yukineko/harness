//! Parse the normalize agent's stdout: split the report body (the requirement
//! TOML, or the escalation request) from the machine-readable trailer.
//!
//! Trailer contract (the prompt forces this at the very end):
//!
//! ```text
//! <<<SPEC_DRAFT>>>
//! rigor: <pass|fail>
//! needs_user: <yes|no>
//! summary: <one line>
//! ```
//!
//! Mirrors specguard's parser: the LAST marker wins; a missing marker means the
//! output is incomplete and must NOT be acted on.

pub const MARKER: &str = "<<<SPEC_DRAFT>>>";

#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    /// Everything before the marker line, trailing-trimmed.
    pub body: String,
    /// True when the rigor gate (G1–G4) passed and a draft was emitted.
    pub rigor_pass: bool,
    /// True when a human must review (escalation).
    pub needs_user: bool,
    pub summary: String,
    /// False when no marker — the output is incomplete (do not act).
    pub marker_found: bool,
}

pub fn parse(stdout: &str) -> Parsed {
    let marker_idx = stdout
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(MARKER))
        .map(|(i, _)| i)
        .last();

    let Some(marker_idx) = marker_idx else {
        return Parsed {
            body: stdout.trim_end().to_string(),
            rigor_pass: false,
            needs_user: false,
            summary: String::new(),
            marker_found: false,
        };
    };

    let lines: Vec<&str> = stdout.lines().collect();
    let body = lines[..marker_idx].join("\n").trim_end().to_string();
    let trailer = &lines[marker_idx + 1..];

    let rigor_pass = field(trailer, "rigor")
        .map(|v| {
            v.split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .map(|t| t.starts_with("pass"))
        .unwrap_or(false);

    let needs_user = field(trailer, "needs_user")
        .map(|v| {
            v.split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .map(|t| t.starts_with("yes"))
        .unwrap_or(false);

    let summary = field(trailer, "summary").unwrap_or_default();

    Parsed {
        body,
        rigor_pass,
        needs_user,
        summary,
        marker_found: true,
    }
}

fn field(lines: &[&str], key: &str) -> Option<String> {
    let key_lower = key.to_ascii_lowercase();
    for line in lines {
        let trimmed = line.trim_start();
        if let Some((lhs, rhs)) = trimmed.split_once(':') {
            if lhs.trim().to_ascii_lowercase() == key_lower {
                return Some(rhs.trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pass_with_body() {
        let s = "[[requirement]]\nid = \"R1\"\n\n<<<SPEC_DRAFT>>>\nrigor: pass\nneeds_user: no\nsummary: ok";
        let p = parse(s);
        assert!(p.marker_found);
        assert!(p.rigor_pass);
        assert!(!p.needs_user);
        assert_eq!(p.summary, "ok");
        assert!(p.body.contains("[[requirement]]"));
    }

    #[test]
    fn parses_fail_escalation() {
        let s = "canon が rate-limit について沈黙\n\n<<<SPEC_DRAFT>>>\nrigor: fail\nneeds_user: yes\nsummary: 閾値が未定義";
        let p = parse(s);
        assert!(!p.rigor_pass);
        assert!(p.needs_user);
        assert_eq!(p.summary, "閾値が未定義");
    }

    #[test]
    fn no_marker_is_incomplete() {
        let p = parse("just some text without a trailer");
        assert!(!p.marker_found);
        assert!(!p.rigor_pass);
    }

    #[test]
    fn last_marker_wins() {
        let s = "<<<SPEC_DRAFT>>>\nrigor: pass\nneeds_user: no\nsummary: early\n\
                 body\n<<<SPEC_DRAFT>>>\nrigor: fail\nneeds_user: yes\nsummary: late";
        let p = parse(s);
        assert!(!p.rigor_pass);
        assert_eq!(p.summary, "late");
    }

    #[test]
    fn case_insensitive_keys() {
        let s = "b\n<<<SPEC_DRAFT>>>\nRigor: PASS\nNeeds_User: NO\nSummary: Cap";
        let p = parse(s);
        assert!(p.rigor_pass);
        assert!(!p.needs_user);
        assert_eq!(p.summary, "Cap");
    }
}
