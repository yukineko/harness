//! Build a span tree from a flat span list and render it for the terminal.
//!
//! Spans link child→parent by id. A span whose `parent_id` is absent (or points
//! outside the set — a partial/truncated run) is treated as a root, so a trace
//! always renders even if the interpreter span never landed.

use std::collections::HashMap;

use crate::span::Span;

/// Render a run's spans as an indented tree plus a one-line roll-up. Pure over
/// the span slice (no IO) so it is unit-testable.
pub fn render(run_id: &str, spans: &[Span], skipped: usize) -> String {
    if spans.is_empty() {
        return format!("tracekit: no spans recorded for run {run_id}\n");
    }

    // index by span_id; children keyed by parent_id (preserving record order,
    // then stable-sorted by start time below).
    let known: HashMap<&str, &Span> = spans.iter().map(|s| (s.span_id.as_str(), s)).collect();
    let mut children: HashMap<Option<&str>, Vec<usize>> = HashMap::new();
    for (i, s) in spans.iter().enumerate() {
        // A parent that isn't in this set is a dangling link → treat as root.
        let key = match &s.parent_id {
            Some(p) if known.contains_key(p.as_str()) => Some(p.as_str()),
            _ => None,
        };
        children.entry(key).or_default().push(i);
    }
    for v in children.values_mut() {
        v.sort_by_key(|&i| (spans[i].start_unix_ms(), i));
    }

    let mut out = String::new();
    out.push_str(&format!("trace {run_id}\n"));
    let roots = children.get(&None).cloned().unwrap_or_default();
    for &r in &roots {
        render_node(spans, &children, r, 0, &mut out);
    }

    out.push('\n');
    out.push_str(&summary(spans));
    if skipped > 0 {
        out.push_str(&format!("\n  ⚠ {skipped} malformed span line(s) skipped"));
    }
    out.push('\n');
    out
}

fn render_node(
    spans: &[Span],
    children: &HashMap<Option<&str>, Vec<usize>>,
    idx: usize,
    depth: usize,
    out: &mut String,
) {
    let s = &spans[idx];
    let indent = "  ".repeat(depth);
    let marker = if s.is_error() { "✗" } else { "·" };
    let model = s.model.as_deref().unwrap_or("-");
    let cost = s
        .cost_usd
        .map(|c| format!("${c:.4}"))
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!(
        "{indent}{marker} {name} [{phase}/{model}] {ms}ms {cost} {status}\n",
        name = s.name,
        phase = s.phase,
        ms = s.ms,
        status = s.status,
    ));
    if let Some(kids) = children.get(&Some(s.span_id.as_str())) {
        for &k in kids {
            render_node(spans, children, k, depth + 1, out);
        }
    }
}

/// One-line roll-up: total cost, wall-clock span, slowest phase, error count.
fn summary(spans: &[Span]) -> String {
    let total_cost: f64 = spans.iter().filter_map(|s| s.cost_usd).sum();
    let errors = spans.iter().filter(|s| s.is_error()).count();
    let wall = wall_ms(spans);
    let slowest = spans.iter().max_by_key(|s| s.ms);
    let slow = slowest
        .map(|s| format!("{} ({}ms)", s.name, s.ms))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "  {n} spans · wall {wall}ms · ${cost:.4} · slowest {slow} · {errors} error(s)",
        n = spans.len(),
        cost = total_cost,
    )
}

/// Wall-clock = latest end minus earliest start across all spans.
fn wall_ms(spans: &[Span]) -> u64 {
    let min_start = spans.iter().map(|s| s.start_unix_ms()).min().unwrap_or(0);
    let max_end = spans.iter().map(|s| s.end_unix_ms).max().unwrap_or(0);
    max_end.saturating_sub(min_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    fn span(run: &str, id: &str, parent: Option<&str>, ms: u64, end: u64, status: &str) -> Span {
        Span {
            run_id: run.into(),
            span_id: id.into(),
            parent_id: parent.map(|s| s.to_string()),
            name: id.into(),
            phase: "worker".into(),
            model: Some("sonnet".into()),
            task_id: None,
            ms,
            cost_usd: Some(0.02),
            status: status.into(),
            end_unix_ms: end,
        }
    }

    #[test]
    fn nests_children_under_parent() {
        let spans = vec![
            span("r", "root", None, 500, 1000, "ok"),
            span("r", "child", Some("root"), 200, 700, "ok"),
        ];
        let out = render("r", &spans, 0);
        let root_line = out.lines().find(|l| l.contains("root")).unwrap();
        let child_line = out.lines().find(|l| l.contains("child")).unwrap();
        // child is indented deeper than root.
        let root_indent = root_line.len() - root_line.trim_start().len();
        let child_indent = child_line.len() - child_line.trim_start().len();
        assert!(child_indent > root_indent, "{out}");
    }

    #[test]
    fn dangling_parent_renders_as_root() {
        // parent "ghost" was never recorded → child must still appear.
        let spans = vec![span("r", "orphan", Some("ghost"), 100, 500, "ok")];
        let out = render("r", &spans, 0);
        assert!(out.contains("orphan"), "{out}");
    }

    #[test]
    fn summary_counts_errors_and_marks_them() {
        let spans = vec![
            span("r", "a", None, 100, 200, "ok"),
            span("r", "b", Some("a"), 300, 500, "failed"),
        ];
        let out = render("r", &spans, 0);
        assert!(out.contains("✗ b"), "{out}");
        assert!(out.contains("1 error(s)"), "{out}");
        // slowest is b at 300ms.
        assert!(out.contains("slowest b (300ms)"), "{out}");
    }

    #[test]
    fn empty_is_graceful() {
        let out = render("r", &[], 0);
        assert!(out.contains("no spans recorded"), "{out}");
    }

    #[test]
    fn wall_clock_is_span_of_timeline() {
        let spans = vec![
            span("r", "a", None, 100, 200, "ok"),      // [100,200]
            span("r", "b", Some("a"), 100, 600, "ok"), // [500,600]
        ];
        // earliest start 100, latest end 600 → wall 500.
        assert_eq!(wall_ms(&spans), 500);
    }
}
