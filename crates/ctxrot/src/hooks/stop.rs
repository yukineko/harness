use harness_core::hook::HookInput;

use crate::config::Config;

/// Stop hook core: returns a JSON `{"decision":"block","reason":"..."}` string
/// when context usage exceeds the threshold, `None` to allow the session to end.
pub fn run(input: &HookInput, cfg: &Config) -> Option<String> {
    if input.stop_hook_active {
        return None;
    }
    if !cfg.auto_compact_enabled {
        return None;
    }
    let pct = input
        .context_window
        .as_ref()
        .and_then(|c| c.used_percentage)?;
    let threshold = cfg.auto_compact_at_percentage * 100.0;
    if pct >= threshold {
        let reason = format!(
            "Context at {pct:.0}% (threshold {threshold:.0}%). Please run /compact to free up context before continuing."
        );
        Some(serde_json::json!({ "decision": "block", "reason": reason }).to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use harness_core::hook::{ContextWindow, HookInput};

    use super::*;

    fn cfg(enabled: bool, threshold: f64) -> Config {
        let mut c = Config::default();
        c.auto_compact_enabled = enabled;
        c.auto_compact_at_percentage = threshold;
        c
    }

    fn input_at(pct: f64, stop_hook_active: bool) -> HookInput {
        HookInput {
            stop_hook_active,
            context_window: Some(ContextWindow {
                used_percentage: Some(pct),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn high_usage_blocks() {
        let out = run(&input_at(92.0, false), &cfg(true, 0.90)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["decision"], "block");
        assert!(v["reason"].as_str().unwrap().contains("92%"));
    }

    #[test]
    fn at_threshold_blocks() {
        let out = run(&input_at(90.0, false), &cfg(true, 0.90)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["decision"], "block");
    }

    #[test]
    fn below_threshold_allows() {
        assert!(run(&input_at(89.9, false), &cfg(true, 0.90)).is_none());
    }

    #[test]
    fn stop_hook_active_allows() {
        assert!(run(&input_at(99.0, true), &cfg(true, 0.90)).is_none());
    }

    #[test]
    fn disabled_allows() {
        assert!(run(&input_at(99.0, false), &cfg(false, 0.90)).is_none());
    }

    #[test]
    fn no_context_window_allows() {
        let input = HookInput {
            stop_hook_active: false,
            ..Default::default()
        };
        assert!(run(&input, &cfg(true, 0.90)).is_none());
    }
}
