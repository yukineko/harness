//! Export a run's span tree as OpenTelemetry GenAI-semconv JSON (OTLP/JSON
//! encoding, file-only — no network). The shape matches the OTLP `TracesData`
//! message so the file can be replayed into any OTLP backend later, and the
//! attributes follow the GenAI agent-span conventions:
//! <https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-agent-spans/>.

use std::hash::{Hash, Hasher};

use serde_json::{json, Value};

use crate::span::Span;

/// Build the OTLP/JSON `TracesData` document for a run's spans.
pub fn to_otlp(service: &str, spans: &[Span]) -> Value {
    let otlp_spans: Vec<Value> = spans.iter().map(span_to_otlp).collect();
    json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [str_attr("service.name", service)]
            },
            "scopeSpans": [{
                "scope": { "name": "tracekit", "version": env!("CARGO_PKG_VERSION") },
                "spans": otlp_spans
            }]
        }]
    })
}

fn span_to_otlp(s: &Span) -> Value {
    let start_nanos = (s.start_unix_ms() as u128) * 1_000_000;
    let end_nanos = (s.end_unix_ms as u128) * 1_000_000;

    let mut attributes = vec![
        str_attr("gen_ai.operation.name", operation_name(&s.phase)),
        str_attr("harness.phase", &s.phase),
    ];
    if let Some(model) = &s.model {
        attributes.push(str_attr("gen_ai.request.model", model));
    }
    if let Some(task) = &s.task_id {
        attributes.push(str_attr("harness.task_id", task));
    }
    if let Some(cost) = s.cost_usd {
        attributes.push(double_attr("gen_ai.usage.cost_usd", cost));
    }
    attributes.push(str_attr("harness.status", &s.status));

    let parent = s.parent_id.as_deref().map(span_hex).unwrap_or_default();

    json!({
        "traceId": trace_hex(&s.run_id),
        "spanId": span_hex(&s.span_id),
        "parentSpanId": parent,
        "name": s.name,
        // SPAN_KIND_INTERNAL — an in-process agent/phase span.
        "kind": 1,
        "startTimeUnixNano": start_nanos.to_string(),
        "endTimeUnixNano": end_nanos.to_string(),
        "attributes": attributes,
        // STATUS_CODE_OK (1) / STATUS_CODE_ERROR (2).
        "status": { "code": if s.is_error() { 2 } else { 1 } }
    })
}

/// GenAI operation name: tool phases are `execute_tool`, everything else
/// (interpreter/worker/verifier) is an `invoke_agent`.
fn operation_name(phase: &str) -> &'static str {
    if phase.eq_ignore_ascii_case("tool") || phase.eq_ignore_ascii_case("execute_tool") {
        "execute_tool"
    } else {
        "invoke_agent"
    }
}

fn str_attr(key: &str, value: &str) -> Value {
    json!({ "key": key, "value": { "stringValue": value } })
}

fn double_attr(key: &str, value: f64) -> Value {
    json!({ "key": key, "value": { "doubleValue": value } })
}

fn hash64(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// 8-byte (16 hex) span id, derived deterministically from the caller's id so
/// parent links resolve to the same hex.
fn span_hex(s: &str) -> String {
    format!("{:016x}", hash64(s))
}

/// 16-byte (32 hex) trace id derived from the run id (two salted hashes).
fn trace_hex(s: &str) -> String {
    let a = hash64(s);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "tracekit-trace-salt".hash(&mut h);
    s.hash(&mut h);
    format!("{a:016x}{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(id: &str, parent: Option<&str>, phase: &str, status: &str) -> Span {
        Span {
            run_id: "RID-1".into(),
            span_id: id.into(),
            parent_id: parent.map(|s| s.to_string()),
            name: id.into(),
            phase: phase.into(),
            model: Some("opus".into()),
            task_id: Some("t1".into()),
            ms: 250,
            cost_usd: Some(0.5),
            status: status.into(),
            end_unix_ms: 2000,
        }
    }

    #[test]
    fn hex_ids_have_otlp_widths_and_are_stable() {
        assert_eq!(span_hex("t1").len(), 16);
        assert_eq!(trace_hex("RID-1").len(), 32);
        assert_eq!(span_hex("t1"), span_hex("t1"));
        assert_ne!(span_hex("t1"), span_hex("t2"));
    }

    #[test]
    fn parent_link_resolves_to_same_hex() {
        let child = span("c", Some("p"), "worker", "ok");
        let v = span_to_otlp(&child);
        assert_eq!(v["parentSpanId"], json!(span_hex("p")));
    }

    #[test]
    fn root_has_empty_parent() {
        let root = span("root", None, "interpreter", "ok");
        let v = span_to_otlp(&root);
        assert_eq!(v["parentSpanId"], json!(""));
    }

    #[test]
    fn operation_name_maps_phase() {
        assert_eq!(operation_name("worker"), "invoke_agent");
        assert_eq!(operation_name("interpreter"), "invoke_agent");
        assert_eq!(operation_name("tool"), "execute_tool");
    }

    #[test]
    fn error_status_maps_to_code_2() {
        let ok = span_to_otlp(&span("a", None, "worker", "ok"));
        let bad = span_to_otlp(&span("b", None, "worker", "failed"));
        assert_eq!(ok["status"]["code"], json!(1));
        assert_eq!(bad["status"]["code"], json!(2));
    }

    #[test]
    fn timestamps_are_nanos_and_start_precedes_end() {
        let v = span_to_otlp(&span("a", None, "worker", "ok"));
        // end 2000ms → 2_000_000_000 ns; start = (2000-250)ms.
        assert_eq!(v["endTimeUnixNano"], json!("2000000000"));
        assert_eq!(v["startTimeUnixNano"], json!("1750000000"));
    }

    #[test]
    fn document_has_resource_and_scope() {
        let doc = to_otlp("condukt", &[span("a", None, "worker", "ok")]);
        assert_eq!(
            doc["resourceSpans"][0]["scopeSpans"][0]["scope"]["name"],
            json!("tracekit")
        );
        let svc = &doc["resourceSpans"][0]["resource"]["attributes"][0];
        assert_eq!(svc["key"], json!("service.name"));
        assert_eq!(svc["value"]["stringValue"], json!("condukt"));
    }
}
