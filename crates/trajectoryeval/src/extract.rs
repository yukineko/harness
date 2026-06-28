//! Ordered tool-call extractor from a Claude Code transcript.
//!
//! A transcript is JSONL: one JSON object per line, each a transcript event. We
//! STREAM it line-by-line (the harness has a hard rule against loading a whole
//! transcript into memory) and collect, in order, the `name` of every
//! `type == "tool_use"` content block. Parsing is defensive: a line that doesn't
//! parse, or a missing field, is skipped — never a panic.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// Stream `path` and return the ordered list of `tool_use` names.
pub fn extract_tools(path: &Path) -> std::io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut tools = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // Defensive: skip lines that aren't valid JSON.
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        collect_from_event(&value, &mut tools);
    }
    Ok(tools)
}

/// Pull tool_use names out of one event, tolerating shape variation.
fn collect_from_event(value: &Value, tools: &mut Vec<String>) {
    // Assistant messages keep their blocks under ["message"]["content"]; some
    // shapes put a content array at the top level. Tolerate both.
    if let Some(content) = value.get("message").and_then(|m| m.get("content")) {
        collect_from_content(content, tools);
    }
    if let Some(content) = value.get("content") {
        collect_from_content(content, tools);
    }
}

/// For each content block that is an object with `"type":"tool_use"`, push its name.
fn collect_from_content(content: &Value, tools: &mut Vec<String>) {
    let Some(arr) = content.as_array() else {
        return;
    };
    for block in arr {
        let is_tool_use = block.get("type").and_then(Value::as_str) == Some("tool_use");
        if is_tool_use {
            if let Some(name) = block.get("name").and_then(Value::as_str) {
                tools.push(name.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_tool_names_in_order() {
        // Temp fixture keyed by pid so parallel test runs don't collide.
        let mut path = std::env::temp_dir();
        path.push(format!(
            "trajectoryeval-fixture-{}.jsonl",
            std::process::id()
        ));

        let mut f = File::create(&path).unwrap();
        // line 1: assistant message with two tool_use blocks (message.content shape)
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"hi"}},{{"type":"tool_use","name":"Read","input":{{}}}},{{"type":"tool_use","name":"Edit","input":{{}}}}]}}}}"#
        )
        .unwrap();
        // line 2: a non-tool event (user message) — contributes nothing
        writeln!(
            f,
            r#"{{"type":"user","message":{{"content":"just text"}}}}"#
        )
        .unwrap();
        // line 3: top-level content shape with one tool_use
        writeln!(f, r#"{{"content":[{{"type":"tool_use","name":"Bash"}}]}}"#).unwrap();
        // line 4: garbage that must be skipped, not panic
        writeln!(f, "not json at all").unwrap();
        f.flush().unwrap();
        drop(f);

        let tools = extract_tools(&path).unwrap();
        assert_eq!(
            tools,
            vec!["Read".to_string(), "Edit".to_string(), "Bash".to_string()]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_io_error() {
        let mut path = std::env::temp_dir();
        path.push(format!("trajectoryeval-nope-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(extract_tools(&path).is_err());
    }
}
