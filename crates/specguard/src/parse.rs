//! Parse the agent's stdout: split the human-readable report from the
//! machine-readable trailer the prompt forces the agent to emit.
//!
//! Trailer contract (the prompt instructs the agent to print exactly this at
//! the very end):
//!
//! ```text
//! <<<SPEC_AUDIT>>>
//! needs_user: <yes|no>
//! summary: <one line>
//! ```

/// The marker token that delimits the trailer. Kept identical between the
/// prompt template and the parser.
pub const MARKER: &str = "<<<SPEC_AUDIT>>>";

#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    /// Report body (everything before the marker line), trailing-trimmed.
    pub report: String,
    /// True when at least one finding needs human review.
    pub needs_user: bool,
    /// One-line summary from the trailer (empty when absent).
    pub summary: String,
    /// False when no marker was found — the report is incomplete and the
    /// caller must NOT raise a sentinel (avoids false positives).
    pub marker_found: bool,
}

/// Parse agent stdout. When the marker is missing, `marker_found` is false and
/// the whole text is returned as the report so nothing is lost.
pub fn parse(stdout: &str) -> Parsed {
    // Use the LAST marker line, mirroring the reference bash runner: if the
    // model echoes the contract earlier, only the final emission counts.
    let marker_idx = stdout
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(MARKER))
        .map(|(i, _)| i)
        .last();

    let Some(marker_idx) = marker_idx else {
        return Parsed {
            report: stdout.trim_end().to_string(),
            needs_user: false,
            summary: String::new(),
            marker_found: false,
        };
    };

    let lines: Vec<&str> = stdout.lines().collect();
    let report = lines[..marker_idx].join("\n").trim_end().to_string();
    let trailer = &lines[marker_idx + 1..];

    let needs_user = field(trailer, "needs_user")
        // Take only the first whitespace token so "yes (3 findings)" -> "yes".
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
        report,
        needs_user,
        summary,
        marker_found: true,
    }
}

/// Find `key:` in the trailer (case-insensitive) and return its value, trimmed.
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
    fn parses_yes_with_report() {
        let s =
            "# Report\n\nbody line\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: fix the thing";
        let p = parse(s);
        assert!(p.marker_found);
        assert!(p.needs_user);
        assert_eq!(p.summary, "fix the thing");
        assert_eq!(p.report, "# Report\n\nbody line");
    }

    #[test]
    fn yes_with_trailing_tokens_still_yes() {
        let s = "r\n<<<SPEC_AUDIT>>>\nneeds_user: yes (3 findings)\nsummary: x";
        assert!(parse(s).needs_user);
    }

    #[test]
    fn no_marker_means_not_found_and_no_pending() {
        let s = "# Report\nbody without trailer\n";
        let p = parse(s);
        assert!(!p.marker_found);
        assert!(!p.needs_user);
        assert_eq!(p.report, "# Report\nbody without trailer");
    }

    #[test]
    fn needs_user_no_is_false() {
        let s = "r\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: none";
        let p = parse(s);
        assert!(p.marker_found);
        assert!(!p.needs_user);
    }

    #[test]
    fn last_marker_wins() {
        let s = "<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: early\n\
                 real report\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: late";
        let p = parse(s);
        assert!(p.needs_user);
        assert_eq!(p.summary, "late");
    }

    #[test]
    fn case_insensitive_keys() {
        let s = "r\n<<<SPEC_AUDIT>>>\nNeeds_User: YES\nSummary: Cap";
        let p = parse(s);
        assert!(p.needs_user);
        assert_eq!(p.summary, "Cap");
    }
}
