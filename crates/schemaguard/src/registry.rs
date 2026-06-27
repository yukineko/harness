//! Named schema registry — maps well-known schema names to their [`Schema`]
//! descriptors. Add new schemas here; the rest of the codebase discovers them
//! via [`get`] / [`names`].

use crate::schema::{Field, Schema, Ty};

// ── static field slices ──────────────────────────────────────────────────────

// decomposition → tasks items
static DECOMPOSITION_TASK_FIELDS: &[Field] = &[
    Field {
        name: "id",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "title",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "class",
        ty: Ty::String,
        required: true,
        enum_values: &["parallel", "serial", "gated"],
        items: &[],
    },
    Field {
        name: "done_criteria",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "suggested_model",
        ty: Ty::String,
        required: false,
        enum_values: &["haiku", "sonnet", "opus"],
        items: &[],
    },
    Field {
        name: "confidence",
        ty: Ty::String,
        required: false,
        enum_values: &["high", "medium", "low"],
        items: &[],
    },
];

// decomposition top-level fields
static DECOMPOSITION_FIELDS: &[Field] = &[
    Field {
        name: "goal",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "tasks",
        ty: Ty::Array,
        required: true,
        enum_values: &[],
        items: DECOMPOSITION_TASK_FIELDS,
    },
];

// episode top-level fields
static EPISODE_FIELDS: &[Field] = &[
    Field {
        name: "title",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "model",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "pass",
        ty: Ty::Bool,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "class",
        ty: Ty::String,
        required: false,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "role",
        ty: Ty::String,
        required: false,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "cost_usd",
        ty: Ty::Number,
        required: false,
        enum_values: &[],
        items: &[],
    },
];

// playbook top-level fields
static PLAYBOOK_FIELDS: &[Field] = &[
    Field {
        name: "title",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "done_criteria",
        ty: Ty::String,
        required: false,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "class",
        ty: Ty::String,
        required: false,
        enum_values: &[],
        items: &[],
    },
];

// scout-measure top-level fields
static SCOUT_MEASURE_FIELDS: &[Field] = &[
    Field {
        name: "title",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
    Field {
        name: "lens",
        ty: Ty::String,
        required: true,
        enum_values: &["L1", "L2", "L3", "L4", "L5"],
        items: &[],
    },
    Field {
        name: "severity",
        ty: Ty::String,
        required: true,
        enum_values: &["high", "medium", "low"],
        items: &[],
    },
    Field {
        name: "effort",
        ty: Ty::String,
        required: true,
        enum_values: &["xs", "s", "m", "l", "xl"],
        items: &[],
    },
    Field {
        name: "evidence",
        ty: Ty::String,
        required: true,
        enum_values: &[],
        items: &[],
    },
];

// ── public API ───────────────────────────────────────────────────────────────

/// All registered schema names in a stable order (used by `schemaguard list`).
pub fn names() -> Vec<&'static str> {
    vec!["decomposition", "episode", "playbook", "scout-measure"]
}

/// Look up a schema by name. Returns `None` for unknown names.
pub fn get(name: &str) -> Option<Schema> {
    match name {
        "decomposition" => Some(Schema {
            name: "decomposition".to_string(),
            fields: DECOMPOSITION_FIELDS.to_vec(),
        }),
        "episode" => Some(Schema {
            name: "episode".to_string(),
            fields: EPISODE_FIELDS.to_vec(),
        }),
        "playbook" => Some(Schema {
            name: "playbook".to_string(),
            fields: PLAYBOOK_FIELDS.to_vec(),
        }),
        "scout-measure" => Some(Schema {
            name: "scout-measure".to_string(),
            fields: SCOUT_MEASURE_FIELDS.to_vec(),
        }),
        _ => None,
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::validate;
    use serde_json::json;

    // ── decomposition ──────────────────────────────────────────────────────

    #[test]
    fn decomposition_valid() {
        let schema = get("decomposition").unwrap();
        let v = json!({
            "goal": "Build a feature",
            "tasks": [
                {
                    "id": "t1",
                    "title": "Implement API",
                    "class": "parallel",
                    "done_criteria": "tests pass",
                    "suggested_model": "sonnet",
                    "confidence": "high"
                }
            ]
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations.is_empty(),
            "expected valid, got {:?}",
            violations
        );
    }

    #[test]
    fn decomposition_missing_task_title() {
        let schema = get("decomposition").unwrap();
        let v = json!({
            "goal": "Build a feature",
            "tasks": [
                {
                    "id": "t1",
                    "class": "serial",
                    "done_criteria": "done"
                }
            ]
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations
                .iter()
                .any(|vi| vi.path.contains("title") && vi.problem.contains("required")),
            "expected title-missing violation, got {:?}",
            violations
        );
    }

    #[test]
    fn decomposition_invalid_class_enum() {
        let schema = get("decomposition").unwrap();
        let v = json!({
            "goal": "Build a feature",
            "tasks": [
                {
                    "id": "t1",
                    "title": "Do something",
                    "class": "bogus",
                    "done_criteria": "done"
                }
            ]
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations
                .iter()
                .any(|vi| vi.path.contains("class") && vi.problem.contains("not in")),
            "expected class enum violation, got {:?}",
            violations
        );
    }

    // ── episode ────────────────────────────────────────────────────────────

    #[test]
    fn episode_valid() {
        let schema = get("episode").unwrap();
        let v = json!({
            "title": "My session",
            "model": "claude-sonnet",
            "pass": true
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(violations.is_empty(), "got {:?}", violations);
    }

    #[test]
    fn episode_missing_pass() {
        let schema = get("episode").unwrap();
        let v = json!({
            "title": "My session",
            "model": "claude-sonnet"
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations
                .iter()
                .any(|vi| vi.path == "pass" && vi.problem.contains("required")),
            "got {:?}",
            violations
        );
    }

    // ── playbook ───────────────────────────────────────────────────────────

    #[test]
    fn playbook_valid() {
        let schema = get("playbook").unwrap();
        let v = json!({"title": "My playbook"});
        let violations = validate(&v, &schema.fields, "");
        assert!(violations.is_empty(), "got {:?}", violations);
    }

    #[test]
    fn playbook_missing_title() {
        let schema = get("playbook").unwrap();
        let v = json!({"done_criteria": "all tests pass"});
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations
                .iter()
                .any(|vi| vi.path == "title" && vi.problem.contains("required")),
            "got {:?}",
            violations
        );
    }

    // ── scout-measure ──────────────────────────────────────────────────────

    #[test]
    fn scout_measure_valid() {
        let schema = get("scout-measure").unwrap();
        let v = json!({
            "title": "DB perf",
            "lens": "L2",
            "severity": "high",
            "effort": "m",
            "evidence": "slow query log shows P99 > 500ms"
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(violations.is_empty(), "got {:?}", violations);
    }

    #[test]
    fn scout_measure_bad_lens() {
        let schema = get("scout-measure").unwrap();
        let v = json!({
            "title": "DB perf",
            "lens": "L9",
            "severity": "high",
            "effort": "m",
            "evidence": "something"
        });
        let violations = validate(&v, &schema.fields, "");
        assert!(
            violations
                .iter()
                .any(|vi| vi.path == "lens" && vi.problem.contains("not in")),
            "got {:?}",
            violations
        );
    }

    // ── names() / get() ────────────────────────────────────────────────────

    #[test]
    fn names_returns_four_schemas() {
        assert_eq!(names().len(), 4);
    }

    #[test]
    fn unknown_name_returns_none() {
        assert!(get("nonexistent_xyz").is_none());
    }

    #[test]
    fn all_names_are_gettable() {
        for name in names() {
            assert!(
                get(name).is_some(),
                "schema '{}' listed but not gettable",
                name
            );
        }
    }
}
