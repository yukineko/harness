use std::path::Path;

/// Read the progress file (bounded by `limit` bytes). Returns None if missing.
pub fn read_file(path: &Path, limit: usize) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    if limit == 0 || raw.len() <= limit {
        Some(raw)
    } else {
        // Truncate at a newline boundary.
        let truncated = &raw[..limit];
        let end = truncated.rfind('\n').unwrap_or(limit);
        let mut out = raw[..end].to_string();
        out.push_str("\n\n*(truncated — see full file for details)*");
        Some(out)
    }
}

/// Build the additionalContext string to inject at SessionStart.
pub fn build_context(path: &Path, limit: usize) -> Option<String> {
    let content = read_file(path, limit)?;
    Some(format!(
        "## Progress file ({path})\n\n{content}",
        path = path.display(),
        content = content,
    ))
}
